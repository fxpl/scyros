// Copyright 2025 Andrea Gilot
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#![doc = include_str!("../docs/duplicate_files.md")]

use std::cmp::{max, min};
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::iter::FromIterator;

use anyhow::{ensure, Context, Result};
use blake3::Hash;
use clap::{Arg, ArgAction, Command};
use either::Either;
use indicatif::ProgressBar;
use polars::frame::DataFrame;
use polars::prelude::{DataFrameJoinOps as _, DataType, Field, Schema};
use tracing::info;

use crate::utils::bow::{Bow, Word};
use crate::utils::dataframes::has_column;
use crate::utils::fs::*;
use crate::utils::logger::{log_output_file, log_write_dataframe, log_write_rows, Logger};
use crate::utils::parallel::parallel_pipeline;
use crate::utils::regex::Matcher;

/// Command line arguments parsing.
pub fn cli() -> Command {
    Command::new("duplicate_files")
        .about("Detects duplicate files in a dataset, returning only unique files.")
        .long_about(include_str!("../docs/duplicate_files.md"))
        .disable_version_flag(true)
        .arg(
            Arg::new("input")
                .short('i')
                .long("input")
                .value_name("INPUT_FILE.csv")
                .help("Path to the input csv file storing the file paths.")
                .required(true),
        )
        .arg(
            Arg::new("output")
                .short('o')
                .long("output")
                .value_name("OUTPUT_FILE.csv")
                .help("Path to the output csv file to store unique files metadata.")
                .required(false),
        )
        .arg(
            Arg::new("map")
                .short('m')
                .long("map")
                .value_name("MAP_FILE.csv")
                .help("Path to the map csv file to store the mapping of clones to their originals.")
                .required(false),
        )
        .arg(
            Arg::new("force")
                .short('f')
                .long("force")
                .help("Override the output CSV file if it already exists.")
                .default_value("false")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("threads")
                .short('n')
                .help("Number of threads to use.")
                .default_value("1")
                .value_parser(clap::value_parser!(usize)),
        )
        .arg(
            Arg::new("similarity")
                .short('s')
                .help("Similarity criterion for duplicate detection.")
                .default_value("exact")
                .value_parser(["exact", "bow", "overlap"]),
        )
        .arg(
            Arg::new("threshold")
                .long("threshold")
                .help("Similarity threshold for duplicate detection when using overlap similarity.")
                .default_value("0.8")
                .value_parser(clap::value_parser!(f64)),
        )
        .arg(
            Arg::new("languages")
                .short('l')
                .long("languages")
                .num_args(1..)
                .action(ArgAction::Append)
                .value_name("LANGUAGES_FILES.json")
                .help("List of files containing the list of languages and extensions to keep. The files must be in JSON format.\n\
                       The files must have the following structure:\n    \
                        {\n\
                            \"languages\": [\n\
                                {\n\
                                \"name\": \"LanguageName\",\n\
                                \"extensions\": [\".ext1\", \".ext2\", ...]\n\
                                },\n\
                                ...\n\
                            ]\n\
                        }")
                .required(true)
        )
        .arg(
            Arg::new("header")
                .long("header")
                .help("Name of column storing file paths in the input CSV file.")
                .default_value("name"),
        )
}

type FileId = usize;
const MAX_FILE_SIZE: u64 = 1024 * 1024 * 1024;

/// Detects duplicate files in a dataset, returning only unique files.
///
/// # Arguments
///
/// * `input_path` - The path to the input CSV file storing the file paths.
/// * `output_path` - The optional path to the output CSV file to store unique files metadata.
/// * `map_path` - The optional path to the map CSV file to store the mapping of clones to their originals.
/// * `force` - Whether to override the output file if it already exists.
/// * `similarity` - The similarity criterion for duplicate detection (exact match or invariant to token order and whitespaces).
/// * `threshold` - The similarity threshold for duplicate detection when using overlap similarity.
/// * `languages_file_paths` - The list of paths to the files containing the list of languages and extensions to keep.
/// * `threads` - The number of threads to use.
/// * `input_header` - The name of the column storing file paths in the input CSV file.
/// * `logger` - The logger displaying the progress.
///
/// # Returns
///
/// A result indicating success or failure of the operation.
pub fn run(
    input_path: &str,
    output_path: Option<&str>,
    map_path: Option<&str>,
    force: bool,
    similarity: &str,
    threshold: f64,
    // languages_file_paths: &[&str],
    threads: usize,
    input_header: &str,
    logger: &Logger,
) -> Result<()> {
    let default_output_path: String = format!("{input_path}.unique.csv");
    let default_map_path: String = format!("{input_path}.duplicates_map.csv");
    let output_path: &str = output_path.unwrap_or(&default_output_path);
    let map_path: &str = map_path.unwrap_or(&default_map_path);

    check_path(input_path)?;
    log_output_file(output_path, false, force)?;

    let files: DataFrame = open_csv(
        input_path,
        Some(Schema::from_iter(vec![
            Field::new(input_header.into(), DataType::String),
            Field::new("extension".into(), DataType::String),
            Field::new("loc".into(), DataType::UInt32),
            Field::new("words".into(), DataType::UInt32),
        ])),
        None,
    )?;

    ensure!(
        has_column(&files, input_header),
        "File {input_path} does not contain column '{input_header}'."
    );

    let file_count: usize = files.height();

    info!("{} files found.", file_count);

    // Split the dataset into chunks for each thread.
    let items: Vec<(FileId, &str)> = files
        .column(input_header)?
        .str()?
        .into_iter()
        .flatten()
        .enumerate()
        .collect();

    info!("Starting file processing...\n");

    if similarity == "bow" || similarity == "exact" {
        let workers: Vec<Matcher> = (0..threads).map(|_| Matcher::words_matcher()).collect();
        let progress = ProgressBar::new(file_count as u64);
        progress.set_style(
            indicatif::ProgressStyle::default_bar().template("{elapsed} {wide_bar} {percent}%")?,
        );

        let mut hash_map: HashMap<Hash, (usize, &str, u32)> = std::collections::HashMap::new();
        let mut clone_map: HashMap<&str, &str> = HashMap::new();
        let mut big_files: usize = 0;

        parallel_pipeline(
            &items,
            workers,
            |matcher: &mut Matcher,
             (idx, name): &(FileId, &str)|
             -> Result<(FileId, &str, Option<Hash>)> {
                match load_file(name, MAX_FILE_SIZE)? {
                    Ok(file_content) => {
                        let hash: Hash = if similarity == "exact" {
                            blake3::hash(&file_content)
                        } else {
                            blake3::hash(&matcher.bag_of_words(&file_content, true).serialize())
                        };
                        Ok((*idx, name, Some(hash)))
                    }
                    Err(_) => Ok((*idx, name, None)),
                }
            },
            |(new_idx, new_name, opt_hash)| {
                match opt_hash {
                    None => big_files += 1,
                    Some(hash) => {
                        let (original_idx, original_name, count) = match hash_map.get(&hash) {
                            Some((idx, orig_name, cnt)) => (*idx, *orig_name, *cnt),
                            None => (new_idx, new_name, 0),
                        };
                        hash_map.insert(hash, (original_idx, original_name, count + 1));
                        clone_map.insert(new_name, original_name);
                        progress.inc(1);
                    }
                }
                Ok(())
            },
        )?;

        progress.finish();

        let small_files = file_count - big_files;
        let big_files_percentage = (big_files as f64 / file_count as f64) * 100.0;

        info!(
            "Ignored large files: {} / {:.2} %",
            big_files, big_files_percentage
        );
        info!(
            "Remaining files: {} / {:.2} %",
            small_files,
            100.0 - big_files_percentage
        );

        let unique_files = hash_map.len();
        let unique_file_percentage = (unique_files as f64 / small_files as f64) * 100.0;

        info!(
            "Unique files: {} / {:.2} %",
            unique_files, unique_file_percentage
        );
        info!(
            "Duplicate files: {} / {:.2} %",
            small_files - unique_files,
            100.0 - unique_file_percentage
        );

        let names: Vec<&str> = hash_map.values().map(|(_, name, _)| *name).collect();
        let counts: Vec<u32> = hash_map.values().map(|(_, _, count)| *count).collect();

        let most_duplicated_file: u32 = *counts
            .iter()
            .max()
            .with_context(|| "Empty cluster counts")?;

        let clusters = DataFrame::new(vec![
            polars::prelude::Column::new(input_header.into(), names),
            polars::prelude::Column::new("count".into(), counts),
        ])?;

        let most_duplicated_file_percentage =
            (most_duplicated_file as f64 / small_files as f64) * 100.0;

        info!(
            "Most duplicated file: {} times / {:.2} %",
            most_duplicated_file, most_duplicated_file_percentage
        );

        log_write_rows(
            logger,
            map_path,
            [input_header, "original"],
            clone_map.into_iter().map(|(k, v)| [k, v]),
        )?;

        let mut output_df = files.join(
            &clusters,
            [input_header],
            [input_header],
            polars::prelude::JoinType::Inner.into(),
            None,
        )?;

        log_write_dataframe(logger, output_path, &mut output_df)
    } else {
        let (global_bow, file_table) = global_bow(&items, threads)?;
        let token_rankings = global_bow.token_rankings();
        let vector_of_indices =
            index_builder(&file_table, &token_rankings, 10, threshold, threads)?;
        let clone_map = detect_clones(&token_rankings, &vector_of_indices, threshold, &file_table)?;
        let unique_files: usize = clone_map
            .values()
            .filter(|v| matches!(v, Either::Left(_)))
            .count();
        let unique_file_percentage = (unique_files as f64 / file_count as f64) * 100.0;
        info!(
            "Unique files: {} / {:.2} %",
            unique_files, unique_file_percentage
        );
        Ok(())
    }
}

struct FileTable {
    paths: Vec<Box<str>>,
    lengths: Vec<u32>,
}
impl FileTable {
    fn length(&self, f: FileId) -> u32 {
        self.lengths[f]
    }
    fn path(&self, f: FileId) -> &str {
        &self.paths[f]
    }
    fn ids(&self) -> impl Iterator<Item = FileId> {
        0..self.paths.len()
    }
}

/// Builds a global bag of words for the entire dataset and a table storing the length of each
/// function.
///
/// # Arguments
/// * `items` - The list of file paths to process, along with their indices.
/// * `threads` - The number of threads to use for parallel processing.
fn global_bow(items: &Vec<(FileId, &str)>, threads: usize) -> Result<(Bow, FileTable)> {
    let mut global_bow: Bow = Bow::new(true);
    let mut file_lengths: Vec<u32> = Vec::with_capacity(items.len());
    let workers: Vec<Matcher> = (0..threads).map(|_| Matcher::words_matcher()).collect();

    parallel_pipeline(
        items,
        workers,
        |matcher: &mut Matcher,
         (file_id, name): &(FileId, &str)|
         -> Result<Option<(FileId, Bow)>> {
            match load_file(name, MAX_FILE_SIZE)? {
                Ok(file_content) => {
                    let file_bow: Bow = matcher.bag_of_words(&file_content, true);
                    Ok(Some((*file_id, file_bow)))
                }
                Err(_) => Ok(None),
            }
        },
        |res_opt| {
            if let Some((file_id, file_bow)) = res_opt {
                file_lengths[file_id] = file_bow.sum();
                global_bow.extend(file_bow);
            }
            Ok(())
        },
    )?;
    let file_table = FileTable {
        paths: items.iter().map(|(_, path)| (*path).into()).collect(),
        lengths: file_lengths,
    };
    Ok((global_bow, file_table))
}

fn index_builder<'w>(
    file_table: &FileTable,
    token_rankings: &'w HashMap<Word, usize>,
    p_prefix: usize,
    threshold: f64,
    threads: usize,
) -> Result<Vec<InvertedIndex<'w>>> {
    let mut vector_of_indices: Vec<InvertedIndex<'w>> =
        (0..p_prefix).map(|_| InvertedIndex::new()).collect();
    let workers: Vec<Matcher> = (0..threads).map(|_| Matcher::words_matcher()).collect();

    parallel_pipeline(
        &file_table.ids().collect::<Vec<_>>(),
        workers,
        |matcher: &mut Matcher,
         file_id: &FileId|
         -> Result<Option<(FileId, Vec<(&'w Word, u32, u32)>)>> {
            match load_file(file_table.path(*file_id), MAX_FILE_SIZE)? {
                Ok(file_content) => {
                    let file_bow: Bow = matcher.bag_of_words(&file_content, true);
                    Ok(Some((*file_id, file_bow.sort_by(token_rankings)?)))
                }
                Err(_) => Ok(None),
            }
        },
        |res_opt| {
            if let Some((file_id, vector_bow)) = res_opt {
                let mut p: usize = 0;
                let prefix_length: u32 = prefix_length(file_table.length(file_id), threshold);
                for (idx, (token, count, cumulative)) in vector_bow.into_iter().enumerate() {
                    vector_of_indices[p].add(
                        token,
                        Posting {
                            function: file_id,
                            occurrences: count,
                            cursor: Cursor {
                                position: idx,
                                cumulative,
                            },
                        },
                    );
                    if cumulative >= prefix_length {
                        p += 1;
                        if p == p_prefix {
                            break;
                        }
                    }
                }
            }
            Ok(())
        },
    )?;
    Ok(vector_of_indices)
}

/// Retrieves the rank of a token from the global ranking, returning an error if the token is not found.
///
/// # Arguments
///
/// * `token` - The token for which to retrieve the global rank.
/// * `token_rankings` - The mapping of tokens to their frequency in the global corpus, used to determine their rank.
fn get_global_rank(token: &Word, token_rankings: &HashMap<Word, usize>) -> Result<usize> {
    token_rankings.get(token).copied().with_context(|| {
        format!(
            "Token not found in global ranking: {}",
            String::from_utf8_lossy(token)
        )
    })
}

/// Prefix-filtering cutoff: the number of tokens we index/compare per function to decide whether to verify a candidate pair.
///
/// # Arguments
///
/// * `word_count` - The total number of tokens in the function.
/// * `threshold` - The similarity threshold for duplicate detection.
fn prefix_length(word_count: u32, threshold: f64) -> u32 {
    word_count - ((word_count as f64) * threshold).ceil() as u32 + 1
}

fn compute_threshold(origin_word_count: u32, candidate_word_count: u32, threshold: f64) -> u32 {
    (max(origin_word_count, candidate_word_count) as f64 * threshold).round() as u32
}

fn weighted_prefix_end(sorted_bow: &[(&Word, u32, u32)], prefix_length: u32) -> Result<usize> {
    for (idx, (_, _, cumulative)) in sorted_bow.iter().enumerate() {
        if *cumulative >= prefix_length {
            //+1 to convert from index to length
            return Ok(idx + 1);
        }
    }
    anyhow::bail!(
        "Unreachable: Prefix length {} is greater than the total number of tokens in the function.",
        prefix_length
    )
}

fn delta_filter_cost(
    token: &Word,
    vector_of_indices: &[InvertedIndex],
    p_prefix: usize,
    new: bool,
) -> u32 {
    let mut cost = 0;
    if new {
        //if the token is new to the prefix, we need to count its frequency in all previous delta indices
        for p in 1..=p_prefix {
            cost += vector_of_indices[p - 1].token_frequency(token, false);
        }
    } else {
        //just count the frequency in the new delta index
        cost += vector_of_indices[p_prefix - 1].token_frequency(token, false);
    }
    cost
}

fn detect_clones(
    token_rankings: &HashMap<Word, usize>,
    vector_of_indices: &[InvertedIndex],
    threshold: f64,
    file_table: &FileTable,
) -> Result<CloneMap> {
    let mut clone_map: CloneMap = HashMap::new();

    let word_matcher: Matcher = Matcher::words_matcher();
    let p_prefix = vector_of_indices.len();

    for origin in file_table.ids() {
        let function_code = load_file(file_table.path(origin), MAX_FILE_SIZE)?
            .map_err(|_| anyhow::anyhow!("File too large at path '{}'", file_table.path(origin)))?;
        let origin_word_count: u32 = file_table.length(origin);
        let origin_vectored_bow = word_matcher
            .bag_of_words(&function_code, true)
            .sort_by(token_rankings)?;
        let mut candidate_map = CandidateMap::new();

        let init_prefix_end = weighted_prefix_end(
            &origin_vectored_bow,
            prefix_length(origin_word_count, threshold),
        )?;
        //cost of prefix scheme 1 is calculated from an empty prefix, so the initial cost is 0
        let mut filter_cost: u32 = 0;
        //total cost is initially set to max since so 0-prefix can never be chosen as the best prefix scheme
        let mut total_cost: u32 = u32::MAX;
        // big loop, will be used for the different prefix schemes
        let mut origin_cursor: Cursor = Cursor {
            position: 0,
            cumulative: 0,
        };
        'prefix_schemes: for (p, inv_index) in vector_of_indices.iter().enumerate() {
            let prefix_end = init_prefix_end + p; //the prefix end for the current scheme is at least the prefix end of the first scheme + the number of tokens in the prefix
            let scheme_end = min(prefix_end, origin_vectored_bow.len());
            for (position, (token, freq, cumulative)) in origin_vectored_bow
                .iter()
                .enumerate()
                .take(scheme_end)
                .skip(origin_cursor.position)
            {
                origin_cursor.cumulative += freq;
                filter_cost +=
                    delta_filter_cost(token, vector_of_indices, p + 1, position + 1 == prefix_end);
                for candidate_posting in inv_index.get(token).unwrap_or(&Vec::new()) {
                    if clone_map.contains_key(&candidate_posting.function) {
                        continue;
                    }
                    let candidate_word_count: u32 = file_table.length(candidate_posting.function);

                    //skip candidates that are too small to reach the threshold
                    if candidate_word_count
                        < compute_threshold(origin_word_count, origin_word_count, threshold)
                    {
                        continue;
                    }

                    let new_matches: u32 = min(*freq, candidate_posting.occurrences);
                    let current_threshold =
                        compute_threshold(origin_word_count, candidate_word_count, threshold);
                    let upper_bound = min(
                        origin_word_count - cumulative,
                        candidate_word_count - candidate_posting.cursor.cumulative,
                    );
                    if candidate_map.get_token_matches(candidate_posting.function)
                        + upper_bound
                        + new_matches
                        >= current_threshold
                    {
                        candidate_map.add_pending_update(
                            candidate_posting.function,
                            new_matches,
                            candidate_posting.cursor.position,
                            candidate_posting.cursor.cumulative,
                        );
                    }
                }
            }
            origin_cursor.position = scheme_end;
            if p == 0 {
                //apply updates for the first prefix scheme before estimating costs since it relies on min/max length
                candidate_map.apply_pending_updates(file_table)?;
            }
            let new_total_cost = filter_cost
                + candidate_map.verification_cost_estimate((p + 1) as u32, origin_word_count);

            if new_total_cost > total_cost {
                verify_candidates(
                    origin,
                    &origin_vectored_bow,
                    origin_cursor,
                    &mut candidate_map,
                    &mut clone_map,
                    p,
                    token_rankings,
                    threshold,
                    file_table,
                )?;
                break 'prefix_schemes;
            } else {
                candidate_map.apply_pending_updates(file_table)?;
                if p == p_prefix - 1 {
                    verify_candidates(
                        origin,
                        &origin_vectored_bow,
                        origin_cursor,
                        &mut candidate_map,
                        &mut clone_map,
                        p_prefix,
                        token_rankings,
                        threshold,
                        file_table,
                    )?;
                    break 'prefix_schemes;
                }
            }
            total_cost = new_total_cost;
        }
    }
    Ok(clone_map)
}

fn verify_candidates(
    origin_function: FileId,
    origin_vectored_bow: &[(&Word, u32, u32)],
    prefix_origin_last_token_seen_cursor: Cursor,
    candidate_map: &mut CandidateMap,
    clone_map: &mut CloneMap,
    p_prefix: usize,
    token_rankings: &HashMap<Word, usize>,
    threshold: f64,
    file_table: &FileTable,
) -> Result<()> {
    // This function will take the candidate map for a function and verify the candidates that have enough matches
    // to be considered clones based on their full token vectors.
    // The clone_map is updated with the results, mapping original function ids to sets of clone function ids.
    let word_matcher: Matcher = Matcher::words_matcher();
    let origin_word_count = file_table.length(origin_function);
    let origin_unique_tokens = origin_vectored_bow.len();
    let candidates_to_verify =
        candidate_map.candidates_with_n_matches(p_prefix as u32, MatchMode::AtLeast); // (token_position, cumulative_count)
    for candidate in candidates_to_verify {
        if clone_map.contains_key(&candidate) {
            continue;
        }
        if candidate == origin_function {
            continue; //skip comparing the function to itself
        }
        let mut origin_last_token_seen_cursor = prefix_origin_last_token_seen_cursor;
        let candidate_code = load_file(file_table.path(candidate), MAX_FILE_SIZE)?
            .map_err(|e| anyhow::anyhow!("Error loading file '{}': {}.", candidate, e))?;

        // load function, sort tokens by global frequency, calculate similarity, if above threshold add to clone map
        let vectored_candidate_bow = word_matcher
            .bag_of_words(&candidate_code, true)
            .sort_by(token_rankings)?;
        let candidate_word_count: u32 = file_table.length(candidate);
        let candidate_unique_tokens: usize = vectored_candidate_bow.len();
        let current_threshold: u32 =
            compute_threshold(origin_word_count, candidate_word_count, threshold);
        let mut candidate_last_token_seen_cursor: Cursor =
            candidate_map.get_last_token_seen_cursor(candidate)?;
        let mut new_matches: u32 = 0;
        let prefix_matches: u32 = candidate_map.get_token_matches(candidate);
        while origin_last_token_seen_cursor.position < origin_unique_tokens
            && candidate_last_token_seen_cursor.position + 1 < candidate_unique_tokens
        {
            let upper_bound = min(
                origin_word_count - origin_last_token_seen_cursor.cumulative,
                candidate_word_count - candidate_last_token_seen_cursor.cumulative,
            );
            let current_matches: u32 = prefix_matches + new_matches;
            let origin_token_tuple = origin_vectored_bow[origin_last_token_seen_cursor.position];
            let (candidate_token, candidate_token_count, _) =
                vectored_candidate_bow[candidate_last_token_seen_cursor.position + 1];

            let candidate_current_token_pos = candidate_last_token_seen_cursor.position + 1;
            // let candidate_current_token_cumul_count =  candidate_last_token_seen_cumul_count + *candidate_token_count;

            let origin_rank = get_global_rank(origin_token_tuple.0, token_rankings)?;
            let candidate_rank = get_global_rank(candidate_token, token_rankings)?;

            if current_matches >= current_threshold {
                break;
            } else if upper_bound + current_matches >= current_threshold {
                if origin_token_tuple.0 == candidate_token {
                    new_matches += min(origin_token_tuple.1, candidate_token_count);
                    candidate_last_token_seen_cursor.position = candidate_current_token_pos;
                    origin_last_token_seen_cursor.position += 1;
                    origin_last_token_seen_cursor.cumulative += origin_token_tuple.1;
                } else if origin_rank > candidate_rank {
                    candidate_last_token_seen_cursor.position = candidate_current_token_pos;
                } else {
                    origin_last_token_seen_cursor.position += 1;
                    origin_last_token_seen_cursor.cumulative += candidate_token_count;
                }
            } else {
                break;
            }
        }
        candidate_map.add_candidate(
            candidate,
            file_table,
            new_matches,
            candidate_last_token_seen_cursor,
        )?;
        if candidate_map.get_token_matches(candidate) >= current_threshold {
            insert_clone_relation(clone_map, origin_function, candidate);
        }
    }
    Ok(())
}

fn insert_clone_relation(clone_map: &mut CloneMap, origin_function: FileId, candidate: FileId) {
    let origin_entry = clone_map
        .entry(origin_function)
        .or_insert_with(|| Either::Left(HashSet::new()));

    // Origin must always store the set of its clones as Left(HashSet<_>).
    if let Either::Left(clones) = origin_entry {
        clones.insert(candidate);
    } else {
        *origin_entry = Either::Left(HashSet::from([candidate]));
    }

    // Clone points back to its origin as Right(origin_hash).
    clone_map.insert(candidate, Either::Right(origin_function));
}

/// A position within a single function's rank-sorted bag of words.
/// It marks how far we've walked into a sorted bow, carrying both the
/// index and the running word count.
#[derive(Debug, Clone, Copy)]
struct Cursor {
    /// Index of the last token consumed, into the bow's rank-sorted token list.
    position: usize,
    /// Total words seen up to *and including* the token at `position`,
    /// counting duplicates.
    cumulative: u32,
}

/// A posting in the inverted index, representing the occurrence of a token in a function, along with its frequency and positional information.
struct Posting {
    /// The id of the function this posting belongs to
    function: FileId,
    /// The number of occurrences of this token in this function
    occurrences: u32,
    /// The position of the token in the function's rank-sorted bag of words, along with the cumulative count of words up to that position.
    cursor: Cursor,
}

/// Inverted index data structure mapping tokens in a global corpus to the functions they appear in, along with the count of occurrences and positional information.
struct InvertedIndex<'w> {
    map: HashMap<&'w Word, Vec<Posting>>,
}

impl<'w> Default for InvertedIndex<'w> {
    fn default() -> Self {
        InvertedIndex::new()
    }
}

impl<'w> InvertedIndex<'w> {
    fn new() -> Self {
        InvertedIndex {
            map: HashMap::default(),
        }
    }

    fn add(&mut self, token: &'w Word, posting: Posting) {
        //token_position is the index of the token.
        // cumulative_count is the number of words seen up to and including this token including duplicates
        self.map.entry(token).or_default().push(posting);
    }

    fn get(&self, token: &Word) -> Option<&Vec<Posting>> {
        self.map.get(token)
    }

    fn token_frequency(&self, token: &Word, count_duplicates: bool) -> u32 {
        if let Some(functions) = self.get(token) {
            if count_duplicates {
                functions.iter().map(|posting| posting.occurrences).sum()
            } else {
                functions.len() as u32
            }
        } else {
            0
        }
    }
}

type CloneMap = HashMap<FileId, Either<HashSet<FileId>, FileId>>;

#[allow(dead_code)]
enum MatchMode {
    Exact,
    AtLeast,
}

struct CandidateEntry {
    matches: u32,
    last_token_seen_cursor: Cursor,
}

struct CandidateMap {
    entries: HashMap<FileId, CandidateEntry>,
    match_histogram: HashMap<u32, HashSet<FileId>>,
    pending_updates: Vec<(FileId, CandidateEntry)>,
    min_length: u32,
    max_length: u32,
}

impl Default for CandidateMap {
    fn default() -> Self {
        Self::new()
    }
}

impl CandidateMap {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            match_histogram: HashMap::new(),
            min_length: u32::MAX,
            max_length: 0,
            pending_updates: Vec::new(),
        }
    }

    pub fn get_token_matches(&self, function: FileId) -> u32 {
        self.entries
            .get(&function)
            .map(|entry| entry.matches)
            .unwrap_or(0)
    }

    pub fn add_pending_update(
        &mut self,
        function: FileId,
        new_matches: u32,
        last_token_seen_pos: usize,
        last_token_seen_cumul_count: u32,
    ) {
        self.pending_updates.push((
            function,
            CandidateEntry {
                matches: new_matches,
                last_token_seen_cursor: Cursor {
                    position: last_token_seen_pos,
                    cumulative: last_token_seen_cumul_count,
                },
            },
        ));
    }

    pub fn apply_pending_updates(&mut self, file_table: &FileTable) -> Result<()> {
        let updates = self.pending_updates.drain(..).collect::<Vec<_>>();
        for (function, candidate_entry) in updates {
            self.add_candidate(
                function,
                file_table,
                candidate_entry.matches,
                candidate_entry.last_token_seen_cursor,
            )?;
        }
        Ok(())
    }

    pub fn add_candidate(
        &mut self,
        function: FileId,
        file_table: &FileTable,
        new_matches: u32,
        last_token_seen_cursor: Cursor,
    ) -> Result<()> {
        let entry = match self.entries.entry(function) {
            Entry::Occupied(occupied) => occupied.into_mut(),
            Entry::Vacant(vacant) => {
                let length: u32 = file_table.length(function);
                let last_token_seen_pos = 0; // Initialize to 0 for new candidates
                let last_token_seen_cumul_count = 0; // Initialize to 0 for new candidates
                self.min_length = self.min_length.min(length);
                self.max_length = self.max_length.max(length);
                vacant.insert(CandidateEntry {
                    matches: 0,
                    last_token_seen_cursor: Cursor {
                        position: last_token_seen_pos,
                        cumulative: last_token_seen_cumul_count,
                    },
                })
            }
        };

        // Update the match histogram
        if entry.matches > 0 {
            if let Some(bucket) = self.match_histogram.get_mut(&entry.matches) {
                bucket.remove(&function);
            }
        }

        entry.matches += new_matches;
        entry.last_token_seen_cursor = last_token_seen_cursor;
        self.match_histogram
            .entry(entry.matches)
            .or_default()
            .insert(function);
        Ok(())
    }

    pub fn length_range(&self) -> Option<(u32, u32)> {
        if self.entries.is_empty() {
            None
        } else {
            Some((self.min_length, self.max_length))
        }
    }

    pub fn candidates_with_n_matches(&self, n: u32, mode: MatchMode) -> Vec<FileId> {
        match mode {
            MatchMode::Exact => self
                .match_histogram
                .get(&n)
                .into_iter()
                .flatten()
                .copied()
                .collect(),
            MatchMode::AtLeast => self
                .match_histogram
                .iter()
                .filter(|(&m, _)| m >= n)
                .flat_map(|(_, bucket)| bucket.iter().copied())
                .collect(),
        }
    }

    pub fn get_last_token_seen_cursor(&self, function: FileId) -> Result<Cursor> {
        Ok(self
            .entries
            .get(&function)
            .with_context(|| {
                format!(
                    "Candidate function '{}' not found in candidate map.",
                    function
                )
            })?
            .last_token_seen_cursor)
    }

    pub fn count_candidates_with_n_matches(&self, n: u32, mode: MatchMode) -> u32 {
        match mode {
            MatchMode::Exact => self
                .match_histogram
                .get(&n)
                .map(|bucket| bucket.len())
                .unwrap_or(0) as u32,
            MatchMode::AtLeast => self
                .match_histogram
                .iter()
                .filter(|(&matches, _)| matches >= n)
                .map(|(_, bucket)| bucket.len() as u32)
                .sum(),
        }
    }

    pub fn verification_cost_estimate(&self, n: u32, origin_word_count: u32) -> u32 {
        let mut number_of_candidates: u32 =
            self.count_candidates_with_n_matches(n, MatchMode::AtLeast); //the candidates that have already reached n matches

        let mut survivors: u32 = 0;
        for candidate in &self.pending_updates {
            let function = candidate.0;
            let current_matches = self.get_token_matches(function);
            if n > 1 && current_matches == n - 1 {
                // if n==1 the pending list is empty as they have already been applied
                survivors += 1;
            }
        }
        number_of_candidates += survivors; //add the candidates that are about to reach n matches
                                           // I am disregarding the candidates with less than n-1 matches that will also reach n_matches due to new_matches>1
                                           // But as I understand it they should always satisfy property 1
                                           // A candidate doesn't get to come back after being eliminated once
                                           // Also it's a very rare edge case
        let length_range = self.length_range().unwrap_or((u32::MAX, 0));
        let average_length = if length_range.0 == u32::MAX {
            0
        } else {
            (length_range.0 + length_range.1) / 2
        };
        number_of_candidates * (origin_word_count + average_length)
    }
}

#[cfg(test)]
mod tests {

    use polars::prelude::SortMultipleOptions;

    use crate::utils::logger::test_logger;

    use super::*;

    const TEST_DATA: &str = "tests/data/phases/duplicate_files/";

    fn test_duplicate_files(input_path: &str, similarity: &str) -> Result<()> {
        let default_output_path = format!("{input_path}.unique.csv");
        let default_map_path = format!("{input_path}.duplicates_map.csv");
        delete_file(&default_output_path, true)?;
        delete_file(&default_map_path, true)?;
        run(
            input_path,
            None,
            None,
            false,
            similarity,
            1.0,
            1,
            "name",
            test_logger(),
        )?;

        let expected_df = open_csv(&format!("{default_output_path}.expected"), None, None)?;

        let output_df = open_csv(&default_output_path, None, None)?;

        let sorted_expected_df = expected_df.sort(vec!["name"], SortMultipleOptions::new())?;
        let sorted_output_df = output_df.sort(vec!["name"], SortMultipleOptions::new())?;
        assert_eq!(sorted_expected_df, sorted_output_df);

        delete_file(&default_output_path, false)?;

        let expected_map = open_csv(&format!("{default_map_path}.expected"), None, None)?;

        let map_df = open_csv(&default_map_path, None, None)?;

        let sorted_expected_map = expected_map.sort(vec!["name"], SortMultipleOptions::new())?;
        let sorted_map_df = map_df.sort(vec!["name"], SortMultipleOptions::new())?;
        ensure!(
            sorted_expected_map.equals(&sorted_map_df),
            "Duplicate map CSV file does not match expected output."
        );

        delete_file(&default_map_path, false)
    }

    #[test]
    fn exact_files() -> Result<()> {
        test_duplicate_files(&format!("{TEST_DATA}/duplicate_files.csv"), "exact")?;
        test_duplicate_files(&format!("{TEST_DATA}/duplicate_files_bow.csv"), "bow")
    }

    #[test]
    fn missing_input_duplicate_files() {
        assert!(run(
            "nonexistent.csv",
            None,
            None,
            false,
            "exact",
            1.0,
            1,
            "name",
            test_logger()
        )
        .is_err());
    }

    #[test]
    fn output_exists_no_force_duplicate_files() -> Result<()> {
        let input = format!("{TEST_DATA}/duplicate_files.csv");
        let output = format!("{TEST_DATA}/out_no_force.csv");
        let map = format!("{TEST_DATA}/map_no_force.csv");
        write_file(&output, b"")?;
        let result = run(
            &input,
            Some(&output),
            Some(&map),
            false,
            "exact",
            1.0,
            1,
            "name",
            test_logger(),
        );
        delete_file(&output, false)?;
        delete_file(&map, true)?;
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn force_overwrites_duplicate_files() -> Result<()> {
        let input = format!("{TEST_DATA}/duplicate_files.csv");
        let output = format!("{TEST_DATA}/out_force.csv");
        let map = format!("{TEST_DATA}/map_force.csv");
        write_file(&output, b"")?;
        delete_file(&map, true)?;
        run(
            &input,
            Some(&output),
            Some(&map),
            true,
            "exact",
            1.0,
            1,
            "name",
            test_logger(),
        )?;
        let expected_df = open_csv(&format!("{input}.unique.csv.expected"), None, None)?;
        let output_df = open_csv(&output, None, None)?;
        let sorted_expected = expected_df.sort(vec!["name"], SortMultipleOptions::new())?;
        let sorted_output = output_df.sort(vec!["name"], SortMultipleOptions::new())?;
        assert_eq!(sorted_expected, sorted_output);
        delete_file(&output, false)?;
        delete_file(&map, false)
    }

    #[test]
    fn wrong_header_duplicate_files() -> Result<()> {
        let input = format!("{TEST_DATA}/duplicate_files.csv");
        let output = format!("{TEST_DATA}/out_wrong_header.csv");
        let result = run(
            &input,
            Some(&output),
            None,
            false,
            "exact",
            1.0,
            1,
            "wrongcol",
            test_logger(),
        );
        delete_file(&output, true)?;
        assert!(result.is_err());
        Ok(())
    }
}
