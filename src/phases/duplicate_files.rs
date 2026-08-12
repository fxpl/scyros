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
use tracing::{info, warn};

use crate::utils::bow::{Bow, Token};
use crate::utils::dataframes::has_column;
use crate::utils::fs::*;
use crate::utils::logger::{log_output_file, log_write_dataframe, log_write_rows, Logger};
use crate::utils::parallel::parallel_pipeline;
use crate::utils::regex::{KeywordFiles, Matcher};

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
            Arg::new("prefix_depth")
                .short('p')
                .long("prefix-depth")
                .value_name("DEPTH")
                .help("Maximum prefix depth for the overlap criterion. A depth of 1 filters candidates on the rarest \
                       tokens alone; deeper prefixes reject more candidates before checking that they are actually clones, at the cost of more token comparisons.")
                .default_value("1")
                .value_parser(clap::value_parser!(usize)),
        )
        .arg(
            Arg::new("languages")
                .short('l')
                .long("languages")
                .num_args(1..)
                .action(ArgAction::Append)
                .value_name("LANGUAGES_FILES.json")
                .help("List of files mapping file extensions to languages. Only used by the 'overlap' criterion, \
                       which compares files within a language and never across languages. Files whose extension \
                       is listed in none of them are left uncompared. The files must be in JSON format.\n\
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
                .required(false)
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
/// * `prefix_depth` - The maximum prefix depth for the overlap criterion.
/// * `languages_file_paths` - The list of paths to the files mapping file extensions to languages.
///   Only used by the overlap criterion, which compares files within a language and never across.
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
    prefix_depth: usize,
    languages_file_paths: &[&str],
    threads: usize,
    input_header: &str,
    logger: &Logger,
) -> Result<()> {
    let default_output_path: String = format!("{input_path}.unique.csv");
    let default_map_path: String = format!("{input_path}.duplicates_map.csv");
    let output_path: &str = output_path.unwrap_or(&default_output_path);
    let map_path: &str = map_path.unwrap_or(&default_map_path);

    ensure!(
        threshold > 0.0 && threshold <= 1.0,
        "Similarity threshold must be greater than 0 and at most 1, got {threshold}."
    );

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
        // The first three clone types are syntactically similar and so are written in the same
        // language. Files are therefore compared only against others of their own language, which
        // also keeps each index and each candidate search small.
        let keyword_files: KeywordFiles = logger.run_task("Loading languages", || {
            KeywordFiles::new(false).add_files(languages_file_paths, true)
        })?;

        let mut groups: HashMap<String, Vec<&str>> = HashMap::new();
        let mut unclassified: Vec<&str> = Vec::new();
        if languages_file_paths.is_empty() {
            warn!("No language file given: every file is compared against every other, regardless of language.");
            groups.insert(
                "all".to_string(),
                items.iter().map(|(_, name)| *name).collect(),
            );
        } else {
            for (_, name) in &items {
                match keyword_files.file_language(name) {
                    Some(language) => groups.entry(language).or_default().push(name),
                    None => unclassified.push(name),
                }
            }
        }

        let progress = ProgressBar::new(file_count as u64);
        progress.set_style(
            indicatif::ProgressStyle::default_bar().template("{elapsed} {wide_bar} {percent}%")?,
        );

        // Every file is mapped to the representative of its group. A file that is nobody's clone
        // represents itself, and the size of its group is one plus the clones found for it.
        let mut clone_to_original: Vec<[String; 2]> = Vec::with_capacity(file_count);
        let mut names: Vec<String> = Vec::new();
        let mut counts: Vec<u32> = Vec::new();

        for (language, paths) in &groups {
            info!("{}: {} files", language, paths.len());
            // Identifiers are local to a group, so the rarity ranking and the index are too.
            let group: Vec<(FileId, &str)> = paths.iter().copied().enumerate().collect();
            let (group_bow, file_table) = global_bow(&group, threads)?;
            let token_rankings = group_bow.token_rankings();
            let delta_inverted_index = DeltaInvertedIndex::new(
                &file_table,
                &token_rankings,
                prefix_depth,
                threshold,
                threads,
            )?;
            let clone_map = detect_clones(
                &token_rankings,
                &delta_inverted_index,
                threshold,
                &file_table,
                &progress,
            )?;

            for file in file_table.ids() {
                let original: FileId = match clone_map.get(&file) {
                    Some(Either::Right(original)) => *original,
                    group => {
                        let clones = match group {
                            Some(Either::Left(clones)) => clones.len() as u32,
                            _ => 0,
                        };
                        names.push(file_table.path(file).to_string());
                        counts.push(clones + 1);
                        file
                    }
                };
                clone_to_original.push([
                    file_table.path(file).to_string(),
                    file_table.path(original).to_string(),
                ]);
            }
        }

        // Files whose extension belongs to no known language are never compared, so each is left
        // standing on its own.
        if !unclassified.is_empty() {
            info!("Unknown language, left uncompared: {}", unclassified.len());
        }
        for name in unclassified {
            names.push(name.to_string());
            counts.push(1);
            clone_to_original.push([name.to_string(), name.to_string()]);
            progress.inc(1);
        }
        progress.finish();

        let unique_files: usize = names.len();
        let unique_file_percentage = (unique_files as f64 / file_count as f64) * 100.0;

        info!(
            "Unique files: {} / {:.2} %",
            unique_files, unique_file_percentage
        );
        info!(
            "Duplicate files: {} / {:.2} %",
            file_count - unique_files,
            100.0 - unique_file_percentage
        );

        let most_duplicated_file: u32 = counts.iter().max().copied().unwrap_or_default();
        info!(
            "Most duplicated file: {} times / {:.2} %",
            most_duplicated_file,
            (most_duplicated_file as f64 / file_count as f64) * 100.0
        );

        log_write_rows(
            logger,
            map_path,
            [input_header, "original"],
            clone_to_original,
        )?;

        let clusters = DataFrame::new(vec![
            polars::prelude::Column::new(input_header.into(), names),
            polars::prelude::Column::new("count".into(), counts),
        ])?;

        let mut output_df = files.join(
            &clusters,
            [input_header],
            [input_header],
            polars::prelude::JoinType::Inner.into(),
            None,
        )?;

        log_write_dataframe(logger, output_path, &mut output_df)
    }
}

/// Map between code blocks to their paths on disk and their lengths in tokens.
struct FileTable {
    /// Map between file identifiers and their paths on disk.
    paths: Vec<Box<str>>,
    /// Map between file identifiers and their lengths in tokens.
    lengths: Vec<u32>,
}
impl FileTable {
    /// Returns the length in tokens of a code block given its identifier.
    ///
    /// # Arguments
    ///
    /// * `f` - The identifier of the file for which to retrieve the length.
    fn length(&self, f: FileId) -> u32 {
        self.lengths[f]
    }

    /// Returns the path on disk of a code block given its identifier.
    ///
    /// # Arguments
    ///
    /// * `f` - The identifier of the file for which to retrieve the path.
    fn path(&self, f: FileId) -> &str {
        &self.paths[f]
    }

    /// Returns an iterator over the file identifiers in the table.
    fn ids(&self) -> impl Iterator<Item = FileId> {
        0..self.paths.len()
    }
}

/// Builds a global bag of words for the entire dataset and a table storing the length of each
/// code block. The global bag of words is used to calculate the global token rankings,
/// which are used to build the prefix indices and to sort the token vectors for candidate
/// verification, as described in Section 3.3.1 of:
///
/// Hitesh Sajnani, Vaibhav Saini, Jeffrey Svajlenko, Chanchal K. Roy, and Cristina V. Lopes. 2016.
/// SourcererCC: scaling code clone detection to big-code.
/// In Proceedings of the 38th International Conference on Software Engineering (ICSE '16).
/// Association for Computing Machinery, New York, NY, USA, 1157–1168.
/// [https://doi.org/10.1145/2884781.2884877]
///
/// # Arguments
/// * `items` - The list of file paths to process, along with their indices.
/// * `threads` - The number of threads to use for parallel processing.
fn global_bow(items: &Vec<(FileId, &str)>, threads: usize) -> Result<(Bow, FileTable)> {
    let mut global_bow: Bow = Bow::new(true);
    let mut file_lengths: Vec<u32> = vec![0u32; items.len()];
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

/// Sorts a code block's bag of words by global token frequency, returning a vector of tokens
/// with their frequencies and cumulative frequencies, or an error if the file is too large to
/// process or if a token is not found in the global rankings.
/// Described in Section 3.3.1 of:
///
/// Hitesh Sajnani, Vaibhav Saini, Jeffrey Svajlenko, Chanchal K. Roy, and Cristina V. Lopes. 2016.
/// SourcererCC: scaling code clone detection to big-code.
/// In Proceedings of the 38th International Conference on Software Engineering (ICSE '16).
/// Association for Computing Machinery, New York, NY, USA, 1157–1168.
/// [https://doi.org/10.1145/2884781.2884877]
///
/// # Arguments
///
/// * `word_matcher` - The matcher used to compute the bag of words for the code block.
/// * `file_table` - The table storing the file paths and lengths for the dataset.
/// * `file` - The identifier of the file to process.
/// * `token_rankings` - The mapping of tokens to their frequency in the global corpus, used to determine their rank.
///
fn sorted_bow<'w>(
    word_matcher: &Matcher,
    file_table: &FileTable,
    file: FileId,
    token_rankings: &'w HashMap<Token, usize>,
) -> Result<Vec<(&'w Token, u32, u32)>> {
    let codeblock_code = load_file(file_table.path(file), MAX_FILE_SIZE)?
        .map_err(|_| anyhow::anyhow!("File too large at path '{}'", file_table.path(file)))?;
    word_matcher
        .bag_of_words(&codeblock_code, true)
        .sort_by(token_rankings)
}

/// Retrieves the rank of a token from the global ranking, returning an error if the token is not found.
/// The role of the global ranking and token ranks in the clone detection process is described in Section 3.3.1 of:
///
/// Hitesh Sajnani, Vaibhav Saini, Jeffrey Svajlenko, Chanchal K. Roy, and Cristina V. Lopes. 2016.
/// SourcererCC: scaling code clone detection to big-code.
/// In Proceedings of the 38th International Conference on Software Engineering (ICSE '16).
/// Association for Computing Machinery, New York, NY, USA, 1157–1168.
/// [https://doi.org/10.1145/2884781.2884877]
///
/// # Arguments
///
/// * `token` - The token for which to retrieve the global rank.
/// * `token_rankings` - The mapping of tokens to their frequency in the global corpus, used to determine their rank.
fn global_rank(token: &Token, token_rankings: &HashMap<Token, usize>) -> Result<usize> {
    token_rankings.get(token).copied().with_context(|| {
        format!(
            "Token not found in global ranking: {}",
            String::from_utf8_lossy(token)
        )
    })
}

/// Length of a 1-prefix for a code block, as described in Section 3.3.1 of:
///
/// Hitesh Sajnani, Vaibhav Saini, Jeffrey Svajlenko, Chanchal K. Roy, and Cristina V. Lopes. 2016.
/// SourcererCC: scaling code clone detection to big-code.
/// In Proceedings of the 38th International Conference on Software Engineering (ICSE '16).
/// Association for Computing Machinery, New York, NY, USA, 1157–1168.
/// [https://doi.org/10.1145/2884781.2884877]
///
/// and Section 2.2
///
/// Jiannan Wang, Guoliang Li, and Jianhua Feng. 2012.
/// Can we beat the prefix filtering? an adaptive framework for similarity join and search.
/// In Proceedings of the 2012 ACM SIGMOD International Conference on Management of Data (SIGMOD '12).
/// Association for Computing Machinery, New York, NY, USA, 85–96.
/// [https://doi.org/10.1145/2213836.2213847]
///
/// # Arguments
///
/// * `token_count` - The total number of tokens in the code block.
/// * `threshold` - The similarity threshold for duplicate detection (0.0 to 1.0).
fn prefix_length(token_count: u32, threshold: f64) -> u32 {
    token_count - ((token_count as f64) * threshold).ceil() as u32 + 1
}

/// Computes the minimum number of token matches required for a candidate to be considered a clone, as described in Section 3.1 of:
///
/// Hitesh Sajnani, Vaibhav Saini, Jeffrey Svajlenko, Chanchal K. Roy, and Cristina V. Lopes. 2016.
/// SourcererCC: scaling code clone detection to big-code.
/// In Proceedings of the 38th International Conference on Software Engineering (ICSE '16).
/// Association for Computing Machinery, New York, NY, USA, 1157–1168.
/// [https://doi.org/10.1145/2884781.2884877]
///
/// # Arguments
///
/// * `origin_token_count` - The total number of tokens in the original code block.
/// * `candidate_token_count` - The total number of tokens in the candidate code block.
/// * `threshold` - The similarity threshold for duplicate detection (0.0 to 1.0).
fn clone_pair_threshold(
    origin_token_count: u32,
    candidate_token_count: u32,
    threshold: f64,
) -> u32 {
    (max(origin_token_count, candidate_token_count) as f64 * threshold).ceil() as u32
}

/// Returns how many of the leading entries in `sorted_bow` are needed for their
/// combined cumulative frequency to reach `prefix_length`.
///
/// # Arguments
///
/// * `sorted_bow` - The rank-sorted bag of words for a code block with cumulative frequencies.
/// * `prefix_length` - The target cumulative frequency to reach with the prefix.
fn weighted_prefix_end(sorted_bow: &[(&Token, u32, u32)], prefix_length: u32) -> Result<usize> {
    for (idx, (_, _, cumulative)) in sorted_bow.iter().enumerate() {
        if *cumulative >= prefix_length {
            // +1 to convert from index to length
            return Ok(idx + 1);
        }
    }
    anyhow::bail!(
        "Unreachable: Prefix length {} is greater than the total number of tokens in the code block.",
        prefix_length
    )
}

/// Finds every clone pair in the corpus using adaptive prefix filtering.
///
/// Each code block takes a turn as the origin. Its prefix starts one token deep and grows one
/// token per step, and each step queries a new slice of the delta index for candidates.
///
/// A deeper prefix throws out more candidates but costs more lookups. So after each step we
/// estimate what that step cost and stop growing once the cost stops falling. The candidate map of
/// the last step worth taking is the one we verify. Section 4.2 of:
///
/// Manziba Akanda Nishi, Kostadin Damevski,
/// Scalable code clone detection and search based on adaptive prefix filtering,
/// Journal of Systems and Software, Volume 137, 2018, Pages 130-142, ISSN 0164-1212,
/// [https://doi.org/10.1016/j.jss.2017.11.039]
///
/// # Arguments
///
/// * `token_rankings` - The rank of each token in the global corpus.
/// * `delta_inverted_index` - The index built over the prefixes of the corpus.
/// * `threshold` - The similarity threshold for duplicate detection (0.0 to 1.0).
/// * `file_table` - The table storing the file paths and lengths for the dataset.
/// * `progress` - Advanced once per code block; pass a hidden bar to silence it.
fn detect_clones(
    token_rankings: &HashMap<Token, usize>,
    delta_inverted_index: &DeltaInvertedIndex,
    threshold: f64,
    file_table: &FileTable,
    progress: &ProgressBar,
) -> Result<CloneMap> {
    let word_matcher: Matcher = Matcher::words_matcher();
    let mut clone_map: CloneMap = HashMap::new();

    for origin in file_table.ids() {
        progress.inc(1);
        // A block already known to be a clone of an earlier one does not need a search of its own,
        // since its own clones are found through that earlier block.
        if clone_map.contains_key(&origin) {
            continue;
        }
        let origin_token_count: u32 = file_table.length(origin);
        // A block with no tokens has no prefix to filter on, and no overlap to measure against
        // anything. It is reported unique rather than compared.
        if origin_token_count == 0 {
            continue;
        }
        let sorted_tokens: Vec<(&Token, u32, u32)> =
            sorted_bow(&word_matcher, file_table, origin, token_rankings)?;
        let mut candidate_map = CandidateMap::new();

        // Token where the 1-prefix ends, used to determine the starting point for the prefix schemes.
        let initial_prefix_end: usize =
            weighted_prefix_end(&sorted_tokens, prefix_length(origin_token_count, threshold))?;
        //cost of prefix scheme 1 is calculated from an empty prefix, so the initial cost is 0
        let mut filtering_cost: u32 = 0;
        //total cost is initially set to max since so 0-prefix can never be chosen as the best prefix scheme
        let mut total_cost: u32 = u32::MAX;
        // Deepest scheme kept. A rejected scheme still walks its tokens, but its matches are
        // thrown away, so verification has to be told how deep the candidate map actually got.
        let mut best_prefix: usize = 0;
        for (scheme, _) in delta_inverted_index.iter() {
            //the prefix end for the current scheme is at least the prefix end of the first scheme + the number of tokens in the prefix
            let prefix_end: usize = initial_prefix_end + scheme - 1;
            let scheme_end: usize = min(prefix_end, sorted_tokens.len());
            // The whole prefix is walked again at every scheme, not just the token this scheme
            // added. An older token can enter a candidate's prefix for the first time at this
            // depth, and that posting sits in the slice we have not read yet.
            let mut origin_cursor: Cursor = Cursor::new();
            for (position, (token, freq, cumulative)) in
                sorted_tokens.iter().enumerate().take(scheme_end)
            {
                origin_cursor.advance(*freq);
                let new_token: bool = position + 1 == prefix_end;
                filtering_cost +=
                    delta_inverted_index.token_filtering_cost(token, scheme, new_token);
                for candidate_posting in delta_inverted_index
                    .slices_to_scan(scheme, new_token)
                    .iter()
                    .filter_map(|index| index.get(token))
                    .flatten()
                {
                    if clone_map.contains_key(&candidate_posting.codeblock) {
                        continue;
                    }
                    let candidate_token_count: u32 = file_table.length(candidate_posting.codeblock);

                    //skip candidates that are too small to reach the threshold
                    if candidate_token_count
                        < clone_pair_threshold(origin_token_count, origin_token_count, threshold)
                    {
                        continue;
                    }

                    let new_matches: u32 = min(*freq, candidate_posting.occurrences);
                    let current_threshold =
                        clone_pair_threshold(origin_token_count, candidate_token_count, threshold);
                    let upper_bound = min(
                        origin_token_count - cumulative,
                        candidate_token_count - candidate_posting.cursor.cumulative,
                    );
                    if candidate_map.get_token_matches(candidate_posting.codeblock)
                        + upper_bound
                        + new_matches
                        >= current_threshold
                    {
                        candidate_map.add_pending_update(
                            candidate_posting.codeblock,
                            new_matches,
                            candidate_posting.cursor.position,
                            candidate_posting.cursor.cumulative,
                        );
                    }
                }
            }
            if scheme == 1 {
                //apply updates for the first prefix scheme before estimating costs since it relies on min/max length
                candidate_map.apply_pending_updates(file_table);
            }
            let new_total_cost =
                filtering_cost + candidate_map.verification_cost(scheme as u32, origin_token_count);

            // This scheme costs more than the one before it, so growing the prefix no longer pays
            // off. Drop its pending updates and keep the previous scheme.
            if new_total_cost > total_cost {
                break;
            }

            // Worth it: commit the matches and remember this as the scheme to verify.
            total_cost = new_total_cost;
            candidate_map.apply_pending_updates(file_table);
            best_prefix = scheme;
        }

        verify_candidates(
            origin,
            &sorted_tokens,
            &mut candidate_map,
            &mut clone_map,
            best_prefix,
            token_rankings,
            threshold,
            file_table,
            &word_matcher,
        )?;
    }
    Ok(clone_map)
}

/// Confirms or rejects the candidates of one origin by comparing the code blocks in full.
///
/// Filtering only ever looks at prefixes, so a surviving candidate still has to be compared over all
/// of its tokens. Comparison stops early as soon as the threshold is reached or becomes unreachable.
///
///
/// # Arguments
///
/// * `origin_codeblock` - The code block whose candidates are being verified.
/// * `sorted_tokens` - The origin's rank-sorted tokens with their frequencies.
/// * `candidate_map` - The candidates that survived filtering, updated with the matches found here.
/// * `clone_map` - Receives every confirmed clone pair.
/// * `p_prefix` - The prefix scheme the candidate map was built with; candidates below this many
///   matches cannot reach the threshold and are skipped.
/// * `token_rankings` - The rank of each token in the global corpus.
/// * `threshold` - The similarity threshold for duplicate detection (0.0 to 1.0).
/// * `file_table` - The table storing the file paths and lengths for the dataset.
/// * `word_matcher` - The tokenizer
fn verify_candidates(
    origin_codeblock: FileId,
    sorted_tokens: &[(&Token, u32, u32)],
    candidate_map: &mut CandidateMap,
    clone_map: &mut CloneMap,
    p_prefix: usize,
    token_rankings: &HashMap<Token, usize>,
    threshold: f64,
    file_table: &FileTable,
    word_matcher: &Matcher,
) -> Result<()> {
    let origin_token_count = file_table.length(origin_codeblock);
    let origin_unique_tokens = sorted_tokens.len();
    for candidate in candidate_map
        .candidates_with_n_matches(p_prefix as u32, MatchMode::AtLeast)
        .collect::<HashSet<FileId>>()
    {
        if clone_map.contains_key(&candidate) {
            continue;
        }
        if candidate == origin_codeblock {
            continue; //skip comparing the code block to itself
        }
        let mut origin_last_seen_token = Cursor::new();

        // load code block, sort tokens by global frequency, calculate similarity, if above threshold add to clone map
        let vectored_candidate_bow =
            sorted_bow(word_matcher, file_table, candidate, token_rankings)?;
        let candidate_token_count: u32 = file_table.length(candidate);
        let candidate_unique_tokens: usize = vectored_candidate_bow.len();
        let current_threshold: u32 =
            clone_pair_threshold(origin_token_count, candidate_token_count, threshold);
        let mut candidate_last_seen_token: Cursor = candidate_map.last_seen_token(candidate)?;
        let mut new_matches: u32 = 0;
        let prefix_matches: u32 = candidate_map.get_token_matches(candidate);
        while origin_last_seen_token.position < origin_unique_tokens
            && candidate_last_seen_token.position + 1 < candidate_unique_tokens
        {
            let upper_bound = min(
                origin_token_count - origin_last_seen_token.cumulative,
                candidate_token_count - candidate_last_seen_token.cumulative,
            );
            let current_matches: u32 = prefix_matches + new_matches;
            let (origin_token, origin_token_freq, _) =
                sorted_tokens[origin_last_seen_token.position];
            let (candidate_token, candidate_token_freq, _) =
                vectored_candidate_bow[candidate_last_seen_token.position + 1];

            let origin_rank = global_rank(origin_token, token_rankings)?;
            let candidate_rank = global_rank(candidate_token, token_rankings)?;

            if current_matches >= current_threshold {
                break;
            } else if upper_bound + current_matches >= current_threshold {
                if origin_token == candidate_token {
                    new_matches += min(origin_token_freq, candidate_token_freq);
                    candidate_last_seen_token.advance(candidate_token_freq);
                    origin_last_seen_token.advance(origin_token_freq);
                } else if origin_rank > candidate_rank {
                    // The candidate holds the rarer token, so the origin cannot match it.
                    candidate_last_seen_token.advance(candidate_token_freq);
                } else {
                    origin_last_seen_token.advance(origin_token_freq);
                }
            } else {
                break;
            }
        }
        candidate_map.add_candidate(
            candidate,
            file_table,
            new_matches,
            candidate_last_seen_token,
        );
        if candidate_map.get_token_matches(candidate) >= current_threshold {
            insert_clone_relation(clone_map, origin_codeblock, candidate);
        }
    }
    Ok(())
}

fn insert_clone_relation(clone_map: &mut CloneMap, origin_codeblock: FileId, candidate: FileId) {
    let origin_entry = clone_map
        .entry(origin_codeblock)
        .or_insert_with(|| Either::Left(HashSet::new()));

    // Origin must always store the set of its clones as Left(HashSet<_>).
    if let Either::Left(clones) = origin_entry {
        clones.insert(candidate);
    } else {
        *origin_entry = Either::Left(HashSet::from([candidate]));
    }

    // Clone points back to its origin as Right(origin_hash).
    clone_map.insert(candidate, Either::Right(origin_codeblock));
}

/// A position within a single code block's rank-sorted bag of words.
/// It marks how far we've walked into a sorted bow, carrying both the
/// index and the running cumulative frequency up to that index.
#[derive(Debug, Clone, Copy, Default)]
struct Cursor {
    /// Index of the last token consumed, into the bow's rank-sorted token list.
    position: usize,
    /// Sum of frequencies of tokens seen up to *and including* the token at `position`,
    /// counting duplicates.
    cumulative: u32,
}

impl Cursor {
    /// Creates a new cursor at the beginning of the sorted bag of words.
    fn new() -> Self {
        Cursor {
            position: 0,
            cumulative: 0,
        }
    }

    /// Advances the cursor by consuming a token
    ///
    /// # Arguments
    ///
    /// * `token_freq` - The frequency of the token being consumed, used to update the cumulative frequency.
    fn advance(&mut self, token_freq: u32) {
        self.position += 1;
        self.cumulative += token_freq;
    }
}

/// A posting in the inverted index, representing the occurrence of a token in a code block, along with its frequency and positional information.
struct Posting {
    /// The id of the code block this posting belongs to
    codeblock: FileId,
    /// The number of occurrences of this token in this code block
    occurrences: u32,
    /// The position of the token in the code block's rank-sorted bag of words, along with the cumulative frequency of tokens up to that position.
    cursor: Cursor,
}

/// Inverted index data structure mapping tokens in a global corpus to the prefix of code blocks they appear in, along with the count of occurrences and positional information, as described in Section 3.4 of:
///
/// Hitesh Sajnani, Vaibhav Saini, Jeffrey Svajlenko, Chanchal K. Roy, and Cristina V. Lopes. 2016.
/// SourcererCC: scaling code clone detection to big-code.
/// In Proceedings of the 38th International Conference on Software Engineering (ICSE '16).
/// Association for Computing Machinery, New York, NY, USA, 1157–1168.
/// [https://doi.org/10.1145/2884781.2884877]
///
/// and Section 2.2.3 of:
///
/// Jiannan Wang, Guoliang Li, and Jianhua Feng. 2012.
/// Can we beat the prefix filtering? an adaptive framework for similarity join and search.
/// In Proceedings of the 2012 ACM SIGMOD International Conference on Management of Data (SIGMOD '12).
/// Association for Computing Machinery, New York, NY, USA, 85–96.
/// [https://doi.org/10.1145/2213836.2213847]
struct PartialInvertedIndex<'w> {
    map: HashMap<&'w Token, Vec<Posting>>,
}

impl<'w> Default for PartialInvertedIndex<'w> {
    fn default() -> Self {
        PartialInvertedIndex::new()
    }
}

impl<'w> PartialInvertedIndex<'w> {
    /// Creates a new empty inverted index.
    fn new() -> Self {
        PartialInvertedIndex {
            map: HashMap::default(),
        }
    }

    /// Adds a posting to the inverted index for a given token.
    ///
    /// # Arguments
    ///
    /// * `token` - The token to which the posting corresponds.
    /// * `posting` - The code block in which the token appears, along with its frequency and positional information.
    fn add(&mut self, token: &'w Token, posting: Posting) {
        self.map.entry(token).or_default().push(posting);
    }

    /// Retrieves the list of postings for a given token, if it exists in the index.
    ///
    /// # Arguments
    ///
    /// * `token` - The token for which to retrieve the postings.
    fn get(&self, token: &Token) -> Option<&Vec<Posting>> {
        self.map.get(token)
    }

    /// Returns the number of code-block prefixes in which a given token appears
    ///
    /// # Arguments
    ///
    /// * `token` - The token
    fn count(&self, token: &Token) -> u32 {
        self.get(token)
            .map(|postings| postings.len() as u32)
            .unwrap_or_default()
    }
}

/// Delta inverted index data structure consisting of multiple partial inverted indices, each corresponding to a different part of the prefix of code blocks, as described in Section 4.1 of:
///
/// Manziba Akanda Nishi, Kostadin Damevski,
/// Scalable code clone detection and search based on adaptive prefix filtering,
/// Journal of Systems and Software, Volume 137, 2018, Pages 130-142, ISSN 0164-1212,
/// [https://doi.org/10.1016/j.jss.2017.11.039]
///
/// and Section 4.3 of:
///
/// Jiannan Wang, Guoliang Li, and Jianhua Feng. 2012.
/// Can we beat the prefix filtering? an adaptive framework for similarity join and search.
/// In Proceedings of the 2012 ACM SIGMOD International Conference on Management of Data (SIGMOD '12).
/// Association for Computing Machinery, New York, NY, USA, 85–96.
/// [https://doi.org/10.1145/2213836.2213847]
#[derive(Default)]
struct DeltaInvertedIndex<'w> {
    /// A vector of partial inverted indices, where each index corresponds to a different prefix scheme (1-prefix, 2-prefix, etc.)
    /// and contains the tokens that appear in that prefix only along with their postings.
    /// We also refer to the partial indices as "slices" of the delta index.
    partial_indices: Vec<PartialInvertedIndex<'w>>,
}

impl<'w> DeltaInvertedIndex<'w> {
    /// Builds a delta inverted index for the given dataset
    ///
    /// # Arguments
    ///
    /// * `file_table` - The table storing the file paths and lengths for the dataset.
    /// * `token_rankings` - The mapping of tokens to their frequency in the global corpus, used to
    ///   determine their rank and build the prefix schemes.
    /// * `max_scheme` - The maximum prefix scheme to build in the delta index (e.g., 10 for 1-prefix to 10-prefix).
    /// * `threshold` - The similarity threshold for duplicate detection (0.0 to 1.0), used to
    ///   determine the length of the prefixes.
    /// * `threads` - The number of threads to use for parallel processing when building the index.
    fn new(
        file_table: &FileTable,
        token_rankings: &'w HashMap<Token, usize>,
        max_scheme: usize,
        threshold: f64,
        threads: usize,
    ) -> Result<DeltaInvertedIndex<'w>> {
        ensure!(
            max_scheme >= 1,
            "The delta index needs at least one prefix scheme, got {max_scheme}."
        );
        let mut res: DeltaInvertedIndex = DeltaInvertedIndex {
            partial_indices: (0..max_scheme)
                .map(|_| PartialInvertedIndex::new())
                .collect(),
        };
        let workers: Vec<Matcher> = (0..threads).map(|_| Matcher::words_matcher()).collect();

        parallel_pipeline(
            &file_table.ids().collect::<Vec<_>>(),
            workers,
            |matcher: &mut Matcher,
             file_id: &FileId|
             -> Result<Option<(FileId, Vec<(&'w Token, u32, u32)>)>> {
                Ok(Some((
                    *file_id,
                    sorted_bow(matcher, file_table, *file_id, token_rankings)?,
                )))
            },
            |res_opt| {
                if let Some((file_id, vector_bow)) = res_opt {
                    let mut scheme: usize = 1;
                    let prefix_length: u32 = prefix_length(file_table.length(file_id), threshold);
                    for (idx, (token, count, cumulative)) in vector_bow.into_iter().enumerate() {
                        res.add(
                            scheme,
                            token,
                            Posting {
                                codeblock: file_id,
                                occurrences: count,
                                cursor: Cursor {
                                    position: idx,
                                    cumulative,
                                },
                            },
                        );
                        if cumulative >= prefix_length {
                            scheme += 1;
                            if scheme > max_scheme {
                                break;
                            }
                        }
                    }
                }
                Ok(())
            },
        )?;
        Ok(res)
    }

    /// Adds a token in the delta index
    ///
    /// # Arguments
    ///
    /// * `scheme` - The prefix scheme to which the token belongs, starting from 1.
    /// * `token` - The token to add to the index.
    /// * `posting` - The code block in which the token appears, along with its frequency and positional information.
    fn add(&mut self, scheme: usize, token: &'w Token, posting: Posting) {
        self.partial_indices[scheme - 1].add(token, posting);
    }

    /// The slices still to be read for one token of the `scheme`-prefix.
    ///
    /// Every posting lives in exactly one slice, the one for the scheme that first pulled it into
    /// a prefix, so which slices are left depends on when the token joined the prefix:
    ///
    /// * new at this scheme: never read, so all of `1..=scheme` are left
    /// * carried over from a shallower scheme: `1..scheme` were read already, so only `scheme` is left
    ///
    /// Both the filtering cost and the candidate lookup go through here, which keeps them counting
    /// and reading the same lists.
    ///
    /// # Arguments
    ///
    /// * `scheme` - The prefix scheme being evaluated, starting from 1.
    /// * `new` - Whether the token joins the prefix at this scheme or an earlier one.
    fn slices_to_scan(&self, scheme: usize, new: bool) -> &[PartialInvertedIndex<'w>] {
        let first_unread: usize = if new { 0 } else { scheme - 1 };
        &self.partial_indices[first_unread..scheme]
    }

    /// Cost of looking up one token of the origin's prefix, counted as the number of postings read.
    ///  Summed over the prefix, this gives the scheme's filter cost, as described in Section 4.2 of:
    ///
    /// Manziba Akanda Nishi, Kostadin Damevski,
    /// Scalable code clone detection and search based on adaptive prefix filtering,
    /// Journal of Systems and Software, Volume 137, 2018, Pages 130-142, ISSN 0164-1212,
    /// [https://doi.org/10.1016/j.jss.2017.11.039]
    ///
    /// # Arguments
    ///
    /// * `token` - The prefix token being looked up.
    /// * `delta_inverted_index` - The index whose list lengths are summed.
    /// * `scheme` - The prefix scheme being evaluated, starting from 1.
    /// * `new` - Whether the token joins the prefix at this scheme or an earlier one.
    fn token_filtering_cost(&self, token: &Token, scheme: usize, new: bool) -> u32 {
        self.slices_to_scan(scheme, new)
            .iter()
            .map(|index| index.count(token))
            .sum()
    }

    /// Returns an iterator over the partial inverted indices in the delta index, along with their corresponding prefix scheme numbers starting from 1.
    fn iter(&self) -> impl Iterator<Item = (usize, &PartialInvertedIndex<'w>)> {
        self.partial_indices
            .iter()
            .enumerate()
            .map(|(i, index)| (i + 1, index))
    }
}

type CloneMap = HashMap<FileId, Either<HashSet<FileId>, FileId>>;

#[allow(dead_code)]
enum MatchMode {
    Exact,
    AtLeast,
}

#[derive(Default)]
struct CandidateEntry {
    matches: u32,
    last_seen_token: Cursor,
}

/// A map of candidate code blocks that have been found to share tokens with an origin code block
struct CandidateMap {
    /// The candidates found so far, each with its match count and its position in its own tokens.
    entries: HashMap<FileId, CandidateEntry>,
    /// A histogram mapping the number of matches to the set of code block IDs that have that many matches, used for efficient retrieval of candidates with a specific number of matches.
    match_histogram: HashMap<u32, HashSet<FileId>>,
    /// A list of pending updates to be applied to the candidate map
    pending_updates: Vec<(FileId, CandidateEntry)>,
    /// The length of the shortest code block in the candidate map
    min_length: u32,
    /// The length of the longest code block in the candidate map
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

    pub fn get_token_matches(&self, codeblock: FileId) -> u32 {
        self.entries
            .get(&codeblock)
            .map(|entry| entry.matches)
            .unwrap_or(0)
    }

    pub fn add_pending_update(
        &mut self,
        codeblock: FileId,
        new_matches: u32,
        last_token_seen_pos: usize,
        last_token_seen_cumul_freq: u32,
    ) {
        self.pending_updates.push((
            codeblock,
            CandidateEntry {
                matches: new_matches,
                last_seen_token: Cursor {
                    position: last_token_seen_pos,
                    cumulative: last_token_seen_cumul_freq,
                },
            },
        ));
    }

    pub fn apply_pending_updates(&mut self, file_table: &FileTable) {
        let updates = self.pending_updates.drain(..).collect::<Vec<_>>();
        for (codeblock, candidate_entry) in updates {
            self.add_candidate(
                codeblock,
                file_table,
                candidate_entry.matches,
                candidate_entry.last_seen_token,
            );
        }
    }

    pub fn add_candidate(
        &mut self,
        codeblock: FileId,
        file_table: &FileTable,
        new_matches: u32,
        last_seen_token: Cursor,
    ) {
        let entry = match self.entries.entry(codeblock) {
            Entry::Occupied(occupied) => occupied.into_mut(),
            Entry::Vacant(vacant) => {
                let length: u32 = file_table.length(codeblock);
                self.min_length = self.min_length.min(length);
                self.max_length = self.max_length.max(length);
                vacant.insert(CandidateEntry::default())
            }
        };

        // Update the match histogram
        if entry.matches > 0 {
            if let Some(bucket) = self.match_histogram.get_mut(&entry.matches) {
                bucket.remove(&codeblock);
            }
        }

        entry.matches += new_matches;
        entry.last_seen_token = last_seen_token;
        self.match_histogram
            .entry(entry.matches)
            .or_default()
            .insert(codeblock);
    }

    pub fn mid_length(&self) -> u32 {
        if self.entries.is_empty() {
            0
        } else {
            (self.min_length + self.max_length) / 2
        }
    }

    /// Returns a vector of code block IDs that have exactly `n` matches or at least `n` matches, depending on the specified `mode`.
    ///
    /// # Arguments
    ///
    /// * `n` - The number of matches to filter candidates by.
    /// * `mode` - Whether to return candidates with exactly `n` matches or at least `n` matches.
    pub fn candidates_with_n_matches(
        &self,
        n: u32,
        mode: MatchMode,
    ) -> Box<dyn Iterator<Item = FileId> + '_> {
        match mode {
            MatchMode::Exact => {
                Box::new(self.match_histogram.get(&n).into_iter().flatten().copied())
            }

            MatchMode::AtLeast => Box::new(
                self.match_histogram
                    .iter()
                    .filter(move |(&matches, _)| matches >= n)
                    .flat_map(|(_, bucket)| bucket.iter().copied()),
            ),
        }
    }

    pub fn last_seen_token(&self, codeblock: FileId) -> Result<Cursor> {
        Ok(self
            .entries
            .get(&codeblock)
            .with_context(|| {
                format!(
                    "Candidate code block '{}' not found in candidate map.",
                    codeblock
                )
            })?
            .last_seen_token)
    }

    /// Returns the estimated cost of verifying candidates with at least `n` matches
    ///
    /// # Arguments
    ///
    /// * `n` - The number of matches to consider for verification.
    /// * `origin_token_count` - The total number of tokens in the original code block
    pub fn verification_cost(&self, n: u32, origin_token_count: u32) -> u32 {
        let number_of_candidates: u32 = self
            .candidates_with_n_matches(n, MatchMode::AtLeast)
            .count() as u32; //the candidates that have already reached n matches

        let mut survivors: u32 = 0;
        for (codeblock, _) in &self.pending_updates {
            let current_matches = self.get_token_matches(*codeblock);
            if n > 1 && current_matches == n - 1 {
                // if n==1 the pending list is empty as they have already been applied
                survivors += 1;
            }
        }
        // Add the candidates that are about to reach n matches
        (number_of_candidates + survivors) * (origin_token_count + self.mid_length())
    }
}

#[cfg(test)]
mod tests {

    use polars::prelude::SortMultipleOptions;

    use crate::utils::logger::test_logger;

    use rand::{Rng, SeedableRng};

    use super::*;

    const TEST_DATA: &str = "tests/data/phases/duplicate_files/";

    // ---- helpers ----

    fn make_posting(
        codeblock: FileId,
        occurrences: u32,
        position: usize,
        cumulative: u32,
    ) -> Posting {
        Posting {
            codeblock,
            occurrences,
            cursor: Cursor {
                position,
                cumulative,
            },
        }
    }

    fn make_file_table(lengths: Vec<u32>) -> FileTable {
        FileTable {
            paths: lengths
                .iter()
                .enumerate()
                .map(|(i, _)| format!("file{i}.rs").into_boxed_str())
                .collect(),
            lengths,
        }
    }

    // --- tests ---

    #[test]
    fn cursor_advance_increments_position_and_cumulative() {
        let mut c = Cursor::default();
        c.advance(5);
        assert_eq!(c.position, 1);
        assert_eq!(c.cumulative, 5);
        c.advance(3);
        assert_eq!(c.position, 2);
        assert_eq!(c.cumulative, 8);
    }

    #[test]
    fn prefix_length_random() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(0);

        for _ in 0..10_000 {
            let token_count: u32 = rng.gen_range(1..=10_000);
            let threshold: f64 = rng.gen_range(f64::MIN_POSITIVE..=1.0);
            let result = prefix_length(token_count, threshold);
            assert!(result >= 1);
            assert!(result <= token_count);

            let result_thresh_1 = prefix_length(token_count, 1.0);
            assert_eq!(result_thresh_1, 1);
            let result_thresh_0 = prefix_length(token_count, 0.0);
            assert_eq!(result_thresh_0, token_count + 1);

            let result_wc_1 = prefix_length(1, threshold);
            assert_eq!(result_wc_1, 1);
        }
    }

    #[test]
    fn prefix_length_partial_threshold() {
        assert_eq!(prefix_length(10, 0.8), 3);
        assert_eq!(prefix_length(10, 0.5), 6);
    }

    #[test]
    fn clone_pair_threshold_random() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(0);

        for _ in 0..10_000 {
            let token_count1: u32 = rng.gen_range(1..=10_000);
            let token_count2: u32 = rng.gen_range(1..=10_000);
            let threshold: f64 = rng.gen_range(f64::MIN_POSITIVE..=1.0);
            let result = clone_pair_threshold(token_count1, token_count2, threshold);
            let result_sym = clone_pair_threshold(token_count2, token_count1, threshold);
            assert!(result >= 1);
            assert_eq!(result, result_sym);
            assert!(result <= token_count1.max(token_count2));

            let result_thresh_1 = clone_pair_threshold(token_count1, token_count2, 1.0);
            assert_eq!(result_thresh_1, token_count1.max(token_count2));
            let result_thresh_0 = clone_pair_threshold(token_count1, token_count2, 0.0);
            assert_eq!(result_thresh_0, 0);

            let result_wc_1 = clone_pair_threshold(1, 1, threshold);
            assert_eq!(result_wc_1, 1);
        }
    }
    #[test]
    fn clone_pair_threshold_det() {
        assert_eq!(clone_pair_threshold(10, 10, 0.8), 8);
        assert_eq!(clone_pair_threshold(10, 8, 0.8), 8);
        assert_eq!(clone_pair_threshold(10, 10, 0.75), 8);
    }
    // ---- weighted_prefix_end ----

    #[test]
    fn weighted_prefix_end_first_element() -> Result<()> {
        let w1: Token = b"foo".to_vec();
        let bow: Vec<(&Token, u32, u32)> = vec![(&w1, 3, 3)];
        // cumulative=3 >= prefix_length=3 at idx 0 → return 1
        assert_eq!(weighted_prefix_end(&bow, 3)?, 1);
        Ok(())
    }

    #[test]
    fn weighted_prefix_end_second_element() -> Result<()> {
        let w1: Token = b"foo".to_vec();
        let w2: Token = b"bar".to_vec();
        let bow: Vec<(&Token, u32, u32)> = vec![(&w1, 3, 3), (&w2, 2, 5)];
        // cumulative=3 < 4, cumulative=5 >= 4 at idx 1 → return 2
        assert_eq!(weighted_prefix_end(&bow, 4)?, 2);
        Ok(())
    }

    #[test]
    fn weighted_prefix_end_unreachable_returns_error() {
        let w1: Token = b"foo".to_vec();
        let bow: Vec<(&Token, u32, u32)> = vec![(&w1, 3, 3)];
        assert!(weighted_prefix_end(&bow, 10).is_err());
    }

    #[test]
    fn global_rank_found() -> Result<()> {
        let token: Token = b"hello".to_vec();
        let rankings: HashMap<Token, usize> = HashMap::from([(token.clone(), 42)]);
        assert_eq!(global_rank(&token, &rankings)?, 42);
        Ok(())
    }

    #[test]
    fn global_rank_missing() {
        let token: Token = b"missing".to_vec();
        let rankings: HashMap<Token, usize> = HashMap::new();
        assert!(global_rank(&token, &rankings).is_err());
    }

    #[test]
    fn inverted_index_new() {
        let idx: PartialInvertedIndex = PartialInvertedIndex::new();
        let token: Token = b"foo".to_vec();
        assert!(idx.get(&token).is_none());
        assert_eq!(idx.count(&token), 0);
    }

    #[test]
    fn inverted_index_add_then_get() {
        let token: Token = b"foo".to_vec();
        let token2: Token = b"bar".to_vec();
        let mut idx: PartialInvertedIndex = PartialInvertedIndex::new();
        idx.add(&token, make_posting(0, 3, 0, 3));
        idx.add(&token2, make_posting(1, 5, 0, 5));
        let postings = idx.get(&token).unwrap();
        assert_eq!(postings.len(), 1);
        assert_eq!(postings[0].codeblock, 0);
        assert_eq!(postings[0].occurrences, 3);
    }

    #[test]
    fn inverted_index_frequency() {
        let token: Token = b"foo".to_vec();
        let token2: Token = b"bar".to_vec();
        let mut idx: PartialInvertedIndex = PartialInvertedIndex::new();
        idx.add(&token, make_posting(0, 3, 0, 3));
        idx.add(&token, make_posting(1, 5, 1, 5));
        idx.add(&token2, make_posting(2, 2, 0, 2));
        assert_eq!(idx.count(&token), 2);
    }

    // ---- token_filtering_cost ----

    #[test]
    fn token_filtering_cost_new_token_sums_all_previous_indices() {
        let token: Token = b"foo".to_vec();
        let mut idx0: PartialInvertedIndex = PartialInvertedIndex::new();
        let mut idx1: PartialInvertedIndex = PartialInvertedIndex::new();
        idx0.add(&token, make_posting(0, 1, 0, 1));
        idx0.add(&token, make_posting(1, 1, 0, 1));
        idx1.add(&token, make_posting(2, 1, 0, 1));
        let indices = DeltaInvertedIndex {
            partial_indices: vec![idx0, idx1],
        };
        // new=true, p_prefix=2: sums indices[0] (2 code blocks) + indices[1] (1 code block) = 3
        assert_eq!(indices.token_filtering_cost(&token, 2, true), 3);
    }

    #[test]
    fn token_filtering_cost_existing_token_uses_last_index_only() {
        let token: Token = b"foo".to_vec();
        let mut idx0: PartialInvertedIndex = PartialInvertedIndex::new();
        let mut idx1: PartialInvertedIndex = PartialInvertedIndex::new();
        idx0.add(&token, make_posting(0, 1, 0, 1));
        idx1.add(&token, make_posting(1, 1, 0, 1));
        idx1.add(&token, make_posting(2, 1, 0, 1));
        let indices = DeltaInvertedIndex {
            partial_indices: vec![idx0, idx1],
        };
        assert_eq!(indices.token_filtering_cost(&token, 2, false), 2);
    }

    #[test]
    fn slices_to_scan_covers_all_slices_only_for_a_new_token() {
        let indices = DeltaInvertedIndex {
            partial_indices: (0..4).map(|_| PartialInvertedIndex::new()).collect(),
        };
        // A new token has never been looked up, so every slice up to the scheme is still to read.
        assert_eq!(indices.slices_to_scan(1, true).len(), 1);
        assert_eq!(indices.slices_to_scan(3, true).len(), 3);
        // A carried over token was read against the shallower slices already.
        assert_eq!(indices.slices_to_scan(3, false).len(), 1);
        // At scheme 1 there is only one slice, so both cases agree.
        assert_eq!(indices.slices_to_scan(1, false).len(), 1);
    }

    /// Reproduces the filter cost of Table 5 of Nishi & Damevski for their example code block CB1,
    /// walking the prefix the way `detect_clones` does. Guards against the filter reading only the
    /// newest slice, which under-counts the cost and silently drops candidates.
    #[test]
    fn filtering_cost_matches_published_worked_example() {
        // Delta index of Fig. 1 of the paper, restricted to the tokens of CB1's 3-prefix, which
        // are the only lists CB1's filter cost reads. Only list lengths matter, so the posting
        // payloads are placeholders.
        let if_t: Token = b"if".to_vec();
        let static_t: Token = b"static".to_vec();
        let public_t: Token = b"public".to_vec();
        let return_t: Token = b"return".to_vec();
        let factorial_t: Token = b"factorial".to_vec();
        let one_t: Token = b"1".to_vec();
        let placeholder = |cb: FileId| make_posting(cb, 1, 0, 1);

        let mut delta_1: PartialInvertedIndex = PartialInvertedIndex::new();
        for cb in [1, 3, 5] {
            delta_1.add(&if_t, placeholder(cb));
        }
        for cb in [1, 2, 3] {
            delta_1.add(&static_t, placeholder(cb));
        }
        for cb in [1, 2, 3, 5] {
            delta_1.add(&public_t, placeholder(cb));
        }
        delta_1.add(&return_t, placeholder(1));

        let mut delta_2: PartialInvertedIndex = PartialInvertedIndex::new();
        for cb in [2, 5] {
            delta_2.add(&return_t, placeholder(cb));
        }
        delta_2.add(&factorial_t, placeholder(1));

        let mut delta_3: PartialInvertedIndex = PartialInvertedIndex::new();
        for cb in [2, 5] {
            delta_3.add(&factorial_t, placeholder(cb));
        }
        delta_3.add(&return_t, placeholder(3));
        delta_3.add(&static_t, placeholder(4));
        delta_3.add(&one_t, placeholder(1));

        let indices = DeltaInvertedIndex {
            partial_indices: vec![delta_1, delta_2, delta_3],
        };

        // CB1's tokens in global order. Its 1-prefix ends after `return`, and every deeper scheme
        // adds one more token.
        let prefix = [&if_t, &static_t, &public_t, &return_t, &factorial_t, &one_t];
        let initial_prefix_end: usize = 4;

        let mut filtering_cost: u32 = 0;
        let mut cost_per_scheme: Vec<u32> = Vec::new();
        for scheme in 1..=3 {
            let prefix_end: usize = initial_prefix_end + scheme - 1;
            for (position, token) in prefix.iter().copied().enumerate().take(prefix_end) {
                filtering_cost +=
                    indices.token_filtering_cost(token, scheme, position + 1 == prefix_end);
            }
            cost_per_scheme.push(filtering_cost);
        }

        assert_eq!(cost_per_scheme, vec![11, 14, 19]);
    }

    #[test]
    fn file_table_accessors() {
        let ft = make_file_table(vec![10, 20, 15]);
        assert_eq!(ft.length(0), 10);
        assert_eq!(ft.length(1), 20);
        assert_eq!(ft.length(2), 15);
        assert_eq!(ft.path(0), "file0.rs");
        assert_eq!(ft.path(1), "file1.rs");
        assert_eq!(ft.path(2), "file2.rs");
        let ids: Vec<_> = ft.ids().collect();
        assert_eq!(ids, vec![0, 1, 2]);
    }

    // ---- CandidateMap ----

    #[test]
    fn candidate_map_new_is_empty() {
        let cm = CandidateMap::new();
        assert_eq!(cm.get_token_matches(0), 0);
        assert_eq!(cm.mid_length(), 0);
        assert!(cm
            .candidates_with_n_matches(1, MatchMode::Exact)
            .collect::<HashSet<FileId>>()
            .is_empty());
        assert!(cm
            .candidates_with_n_matches(1, MatchMode::AtLeast)
            .collect::<HashSet<FileId>>()
            .is_empty());
    }

    #[test]
    fn candidate_map_last_seen_token_missing() {
        let cm = CandidateMap::new();
        assert!(cm.last_seen_token(99).is_err());
    }

    #[test]
    fn candidate_map_add_candidate_stores_matches_and_cursor() -> Result<()> {
        let ft = make_file_table(vec![10]);
        let mut cm = CandidateMap::new();
        cm.add_candidate(
            0,
            &ft,
            3,
            Cursor {
                position: 2,
                cumulative: 3,
            },
        );
        assert_eq!(cm.get_token_matches(0), 3);
        let cursor = cm.last_seen_token(0)?;
        assert_eq!(cursor.position, 2);
        assert_eq!(cursor.cumulative, 3);
        Ok(())
    }

    #[test]
    fn candidate_map_add_candidate_accumulates_matches() -> Result<()> {
        let ft = make_file_table(vec![10]);
        let mut cm = CandidateMap::new();
        cm.add_candidate(
            0,
            &ft,
            3,
            Cursor {
                position: 2,
                cumulative: 3,
            },
        );
        cm.add_candidate(
            0,
            &ft,
            2,
            Cursor {
                position: 4,
                cumulative: 5,
            },
        );
        assert_eq!(cm.get_token_matches(0), 5);
        let cursor = cm.last_seen_token(0)?;
        assert_eq!(cursor.position, 4);
        assert_eq!(cursor.cumulative, 5);
        Ok(())
    }

    #[test]
    fn candidate_map_mid_length_tracks_min_and_max() -> Result<()> {
        let ft = make_file_table(vec![10, 8, 15]);
        let mut cm = CandidateMap::new();
        cm.add_candidate(0, &ft, 1, Cursor::default()); // length 10
        cm.add_candidate(2, &ft, 1, Cursor::default()); // length 15
        assert_eq!(cm.mid_length(), 12);
        Ok(())
    }

    #[test]
    fn candidate_map_candidates_exact_match() -> Result<()> {
        let ft = make_file_table(vec![10, 10, 10]);
        let mut cm = CandidateMap::new();
        cm.add_candidate(0, &ft, 3, Cursor::default());
        cm.add_candidate(1, &ft, 3, Cursor::default());
        cm.add_candidate(2, &ft, 5, Cursor::default());
        let exact3 = cm
            .candidates_with_n_matches(3, MatchMode::Exact)
            .collect::<HashSet<FileId>>();
        assert_eq!(exact3.len(), 2);
        assert!(exact3.contains(&0) && exact3.contains(&1));
        let exact5 = cm
            .candidates_with_n_matches(5, MatchMode::Exact)
            .collect::<HashSet<FileId>>();
        assert_eq!(exact5.len(), 1);
        assert!(exact5.contains(&2));
        Ok(())
    }

    #[test]
    fn candidate_map_candidates_at_least_match() -> Result<()> {
        let ft = make_file_table(vec![10, 10, 10]);
        let mut cm = CandidateMap::new();
        cm.add_candidate(0, &ft, 3, Cursor::default());
        cm.add_candidate(1, &ft, 5, Cursor::default());
        assert_eq!(
            cm.candidates_with_n_matches(3, MatchMode::AtLeast).count(),
            2
        );
        assert_eq!(
            cm.candidates_with_n_matches(5, MatchMode::AtLeast).count(),
            1
        );
        assert_eq!(
            cm.candidates_with_n_matches(6, MatchMode::AtLeast).count(),
            0
        );
        Ok(())
    }

    #[test]
    fn candidate_map_histogram_updated_on_accumulation() -> Result<()> {
        let ft = make_file_table(vec![10]);
        let mut cm = CandidateMap::new();
        cm.add_candidate(0, &ft, 3, Cursor::default());
        // Bucket 3 should have code block 0 before the second call.
        assert_eq!(cm.candidates_with_n_matches(3, MatchMode::Exact).count(), 1);
        cm.add_candidate(0, &ft, 2, Cursor::default());
        // After accumulation, code block 0 should be in bucket 5, not 3.
        assert_eq!(cm.candidates_with_n_matches(3, MatchMode::Exact).count(), 0);
        assert_eq!(cm.candidates_with_n_matches(5, MatchMode::Exact).count(), 1);
        Ok(())
    }

    #[test]
    fn candidate_map_pending_updates_applied() -> Result<()> {
        let ft = make_file_table(vec![10, 8]);
        let mut cm = CandidateMap::new();
        cm.add_pending_update(0, 3, 2, 3);
        cm.add_pending_update(1, 2, 1, 2);
        // Not yet applied.
        assert_eq!(cm.get_token_matches(0), 0);
        cm.apply_pending_updates(&ft);
        assert_eq!(cm.get_token_matches(0), 3);
        assert_eq!(cm.get_token_matches(1), 2);
        Ok(())
    }

    // ---- verification_cost ----

    #[test]
    fn verification_cost_empty_map_is_zero() {
        let cm = CandidateMap::new();
        assert_eq!(cm.verification_cost(1, 10), 0);
    }

    #[test]
    fn verification_cost_no_pending_updates() -> Result<()> {
        // candidates lengths 10 and 20 → average = 15
        let ft = make_file_table(vec![10, 20]);
        let mut cm = CandidateMap::new();
        cm.add_candidate(0, &ft, 3, Cursor::default()); // length 10, 3 matches
        cm.add_candidate(1, &ft, 5, Cursor::default()); // length 20, 5 matches
                                                        // n=3: both have >= 3 matches → 2 candidates, no survivors
                                                        // average_length = (10 + 20) / 2 = 15
                                                        // cost = 2 * (10 + 15) = 50
        assert_eq!(cm.verification_cost(3, 10), 50);
        Ok(())
    }

    #[test]
    fn verification_cost_counts_survivors_from_pending() -> Result<()> {
        // candidate 0 already has 2 matches, a pending update will push it to 3
        let ft = make_file_table(vec![10]);
        let mut cm = CandidateMap::new();
        cm.add_candidate(0, &ft, 2, Cursor::default());
        cm.add_pending_update(0, 1, 2, 2);
        // n=3: count_at_least(3) = 0, but pending makes candidate 0 a survivor
        // average_length = (10 + 10) / 2 = 10
        // cost = (0 + 1) * (10 + 10) = 20
        assert_eq!(cm.verification_cost(3, 10), 20);
        Ok(())
    }

    #[test]
    fn verification_cost_n1_never_counts_survivors() -> Result<()> {
        // When n==1 the survivor branch is disabled (n > 1 guard).
        let mut cm = CandidateMap::new();
        // candidate 0 has 0 matches (== n-1 == 0) and a pending update
        cm.add_pending_update(0, 1, 0, 0);
        // n=1: no committed candidates with >=1 matches, survivors not counted
        // average_length stays 0 (no committed entries)
        // cost = 0 * (5 + 0) = 0
        assert_eq!(cm.verification_cost(1, 5), 0);
        Ok(())
    }

    // ---- insert_clone_relation ----

    #[test]
    fn insert_clone_relation_sets_forward_and_backward_links() {
        let mut clone_map: CloneMap = HashMap::new();
        insert_clone_relation(&mut clone_map, 0, 1);
        match clone_map.get(&0).unwrap() {
            Either::Left(clones) => assert!(clones.contains(&1)),
            Either::Right(_) => panic!("expected Left for origin"),
        }
        match clone_map.get(&1).unwrap() {
            Either::Right(orig) => assert_eq!(*orig, 0),
            Either::Left(_) => panic!("expected Right for clone"),
        }
    }

    #[test]
    fn insert_clone_relation_multiple_clones_accumulate() {
        let mut clone_map: CloneMap = HashMap::new();
        insert_clone_relation(&mut clone_map, 0, 1);
        insert_clone_relation(&mut clone_map, 0, 2);
        match clone_map.get(&0).unwrap() {
            Either::Left(clones) => {
                assert_eq!(clones.len(), 2);
                assert!(clones.contains(&1) && clones.contains(&2));
            }
            Either::Right(_) => panic!("expected Left for origin"),
        }
    }

    // ---- global_bow ----

    const FILES: &str = "tests/data/phases/duplicate_files/files";

    fn items_from_paths(paths: &[String]) -> Vec<(FileId, &str)> {
        paths
            .iter()
            .enumerate()
            .map(|(i, p)| (i, p.as_str()))
            .collect()
    }

    #[test]
    fn global_bow_builds_file_table_with_correct_lengths() -> Result<()> {
        let paths = vec![
            format!("{FILES}/foo.java"),
            format!("{FILES}/c_float.json"),
            format!("{FILES}/empty.java"),
        ];
        let items = items_from_paths(&paths);
        let (bow, file_table) = global_bow(&items, 1)?;
        assert_eq!(file_table.ids().count(), 3);
        assert_eq!(file_table.path(0), paths[0]);
        assert_eq!(file_table.path(1), paths[1]);
        assert_eq!(file_table.path(2), paths[2]);
        assert!(file_table.length(0) > 0, "foo.java should have tokens");
        assert!(file_table.length(1) > 0, "c_float.json should have tokens");
        assert_eq!(file_table.length(2), 0, "empty.java should have no tokens");
        assert!(bow.sum() > 0, "global bow should be non-empty");
        Ok(())
    }

    #[test]
    fn global_bow_identical_files_have_same_length() -> Result<()> {
        let paths = vec![
            format!("{FILES}/c_float.json"),
            format!("{FILES}/c_float.copy"),
        ];
        let items = items_from_paths(&paths);
        let (_, file_table) = global_bow(&items, 1)?;
        assert_eq!(file_table.length(0), file_table.length(1));
        Ok(())
    }

    // ---- sorted_bow ----

    #[test]
    fn sorted_bow_tokens_are_sorted_by_rank() -> Result<()> {
        let paths = vec![format!("{FILES}/foo.java")];
        let items = items_from_paths(&paths);
        let (bow, file_table) = global_bow(&items, 1)?;
        let rankings = bow.token_rankings();
        let matcher = Matcher::words_matcher();
        let sorted = sorted_bow(&matcher, &file_table, 0, &rankings)?;
        assert!(!sorted.is_empty());
        // Ranks must be non-decreasing.
        for w in sorted.windows(2) {
            assert!(rankings[w[0].0] <= rankings[w[1].0], "not sorted by rank");
        }
        Ok(())
    }

    #[test]
    fn sorted_bow_cumulative_counts_are_non_decreasing() -> Result<()> {
        let paths = vec![format!("{FILES}/foo.java")];
        let items = items_from_paths(&paths);
        let (bow, file_table) = global_bow(&items, 1)?;
        let rankings = bow.token_rankings();
        let matcher = Matcher::words_matcher();
        let sorted = sorted_bow(&matcher, &file_table, 0, &rankings)?;
        for w in sorted.windows(2) {
            assert!(
                w[0].2 <= w[1].2,
                "cumulative frequencies not non-decreasing"
            );
        }
        // Final cumulative equals total token count.
        if let Some((_, _, last_cumul)) = sorted.last() {
            assert_eq!(*last_cumul, file_table.length(0));
        }
        Ok(())
    }

    // ---- index_builder ----

    #[test]
    fn index_builder_first_index_is_non_empty() -> Result<()> {
        let paths = vec![format!("{FILES}/foo.java")];
        let items = items_from_paths(&paths);
        let (bow, file_table) = global_bow(&items, 1)?;
        let rankings = bow.token_rankings();
        let indices = DeltaInvertedIndex::new(&file_table, &rankings, 5, 0.8, 1)?;
        // At least one token from foo.java should appear in the first delta index.
        let first_has_entries = rankings
            .keys()
            .any(|t| indices.partial_indices[0].get(t).is_some());
        assert!(first_has_entries);
        Ok(())
    }

    // ---- detect_clones ----

    #[test]
    fn detect_clones_identical_files_are_clones() -> Result<()> {
        // c_float.json and c_float.copy have the same content.
        let paths = vec![
            format!("{FILES}/c_float.json"),
            format!("{FILES}/c_float.copy"),
            format!("{FILES}/foo.java"),
        ];
        let items = items_from_paths(&paths);
        let threshold = 0.8;
        let (bow, file_table) = global_bow(&items, 1)?;
        let rankings = bow.token_rankings();
        let indices = DeltaInvertedIndex::new(&file_table, &rankings, 10, threshold, 1)?;
        let clone_map = detect_clones(
            &rankings,
            &indices,
            threshold,
            &file_table,
            &ProgressBar::hidden(),
        )?;
        // The two identical files (ids 0 and 1) must appear together in the clone map.
        let in_map = clone_map.contains_key(&0) || clone_map.contains_key(&1);
        assert!(in_map, "identical files not detected as clones");
        // If 0 is origin, 1 must point back to 0, and vice-versa.
        if let Some(entry) = clone_map.get(&0) {
            match entry {
                Either::Left(clones) => assert!(clones.contains(&1)),
                Either::Right(orig) => assert_eq!(*orig, 1),
            }
        }
        Ok(())
    }

    #[test]
    fn detect_clones_distinct_files_are_not_clones() -> Result<()> {
        let paths = vec![format!("{FILES}/foo.java"), format!("{FILES}/c_float.json")];
        let items = items_from_paths(&paths);
        let threshold = 0.95;
        let (bow, file_table) = global_bow(&items, 1)?;
        let rankings = bow.token_rankings();
        let indices = DeltaInvertedIndex::new(&file_table, &rankings, 10, threshold, 1)?;
        let clone_map = detect_clones(
            &rankings,
            &indices,
            threshold,
            &file_table,
            &ProgressBar::hidden(),
        )?;
        // foo.java and c_float.json share very few tokens; neither should be cloned at 0.95.
        let paired = clone_map.contains_key(&0) && clone_map.contains_key(&1) && {
            match (&clone_map[&0], &clone_map[&1]) {
                (Either::Left(s), Either::Right(o)) => s.contains(&1) && *o == 0,
                (Either::Right(o), Either::Left(s)) => s.contains(&0) && *o == 1,
                _ => false,
            }
        };
        assert!(!paired, "distinct files incorrectly detected as clones");
        Ok(())
    }

    // ---- Nishi & Damevski worked example ----
    //
    // The five code blocks of Table 1 of the paper, kept as fixtures so the published numbers in
    // Tables 2 to 7 and Fig. 1 can be asserted directly. The listings are stored without their
    // comment lines, because the paper's tokenizer strips comments and this one does not.
    //
    // Files are loaded in the order cb1..cb5, so a FileId is the index of the code block: CB1 is
    // 0 and CB5 is 4.

    const ND_FILES: &str = "tests/data/phases/duplicate_files/nishi_damevski";
    const ND_THRESHOLD: f64 = 0.8;

    fn nishi_damevski_paths() -> Vec<String> {
        (1..=5).map(|n| format!("{ND_FILES}/cb{n}.java")).collect()
    }

    /// Table 2: the size |t| of each code block, summed from its local token frequencies.
    #[test]
    fn nishi_damevski_block_sizes_match_table_2() -> Result<()> {
        let paths = nishi_damevski_paths();
        let items = items_from_paths(&paths);
        let (_, file_table) = global_bow(&items, 1)?;
        let sizes: Vec<u32> = file_table.ids().map(|f| file_table.length(f)).collect();
        assert_eq!(sizes, vec![16, 21, 28, 23, 16]);
        Ok(())
    }

    /// Table 2: tokens sorted by global frequency, rarest first, ties broken lexicographically.
    #[test]
    fn nishi_damevski_token_order_matches_table_2() -> Result<()> {
        let paths = nishi_damevski_paths();
        let items = items_from_paths(&paths);
        let (bow, file_table) = global_bow(&items, 1)?;
        let rankings = bow.token_rankings();
        let matcher = Matcher::words_matcher();

        let sorted = sorted_bow(&matcher, &file_table, 0, &rankings)?;
        let tokens: Vec<String> = sorted
            .iter()
            .map(|(token, _, _)| String::from_utf8_lossy(token).into_owned())
            .collect();
        // CB1's row of Table 2.
        assert_eq!(
            tokens,
            vec![
                "if",
                "static",
                "public",
                "return",
                "factorial",
                "1",
                "int",
                "result"
            ]
        );

        // Local frequencies of that row, and their running total, which is |t| = 16.
        let frequencies: Vec<u32> = sorted.iter().map(|(_, freq, _)| *freq).collect();
        assert_eq!(frequencies, vec![1, 1, 1, 2, 2, 3, 2, 4]);
        assert_eq!(
            sorted.last().map(|(_, _, cumulative)| *cumulative),
            Some(16)
        );
        Ok(())
    }

    /// Table 3: the 1-prefix is |t| - ceil(theta * |t|) + 1 tokens long, counting duplicates.
    #[test]
    fn nishi_damevski_prefix_sizes_match_table_3() {
        assert_eq!(prefix_length(16, ND_THRESHOLD), 4);
        assert_eq!(prefix_length(21, ND_THRESHOLD), 5);
    }

    /// Fig. 1: every posting sits in the slice of the scheme that first pulls it into a prefix.
    #[test]
    fn nishi_damevski_delta_index_matches_figure_1() -> Result<()> {
        let paths = nishi_damevski_paths();
        let items = items_from_paths(&paths);
        let (bow, file_table) = global_bow(&items, 1)?;
        let rankings = bow.token_rankings();
        let indices = DeltaInvertedIndex::new(&file_table, &rankings, 3, ND_THRESHOLD, 1)?;

        let blocks = |scheme: usize, token: &str| -> Vec<FileId> {
            let token: Token = token.as_bytes().to_vec();
            let mut found: Vec<FileId> = indices.partial_indices[scheme - 1]
                .get(&token)
                .map(|postings| postings.iter().map(|p| p.codeblock).collect())
                .unwrap_or_default();
            found.sort();
            found
        };

        // Slice 1, the 1-prefixes.
        assert_eq!(blocks(1, "if"), vec![0, 2, 4]);
        assert_eq!(blocks(1, "static"), vec![0, 1, 2]);
        assert_eq!(blocks(1, "public"), vec![0, 1, 2, 4]);
        assert_eq!(blocks(1, "return"), vec![0]);
        assert_eq!(blocks(1, "for"), vec![1, 2]);
        assert_eq!(blocks(1, "n"), vec![1]);
        assert_eq!(blocks(1, "0"), vec![2, 4]);
        assert_eq!(blocks(1, "5"), vec![3]);
        assert_eq!(blocks(1, "else"), vec![4]);

        // Slice 2, the tokens the 2-prefix adds. The paper walks through return in Section 4.1.
        assert_eq!(blocks(2, "return"), vec![1, 4]);
        assert_eq!(blocks(2, "for"), vec![3]);
        assert_eq!(blocks(2, "n"), vec![2]);
        assert_eq!(blocks(2, "factorial"), vec![0]);

        // Slice 3, the deepest one. An off-by-one in the builder used to leave it empty.
        assert_eq!(blocks(3, "return"), vec![2]);
        assert_eq!(blocks(3, "factorial"), vec![1, 4]);
        assert_eq!(blocks(3, "1"), vec![0]);
        assert_eq!(blocks(3, "static"), vec![3]);
        Ok(())
    }

    /// Section 4.1: the inverted list of a token is the union of its slices up to that scheme.
    #[test]
    fn nishi_damevski_inverted_lists_are_the_union_of_their_slices() -> Result<()> {
        let paths = nishi_damevski_paths();
        let items = items_from_paths(&paths);
        let (bow, file_table) = global_bow(&items, 1)?;
        let rankings = bow.token_rankings();
        let indices = DeltaInvertedIndex::new(&file_table, &rankings, 3, ND_THRESHOLD, 1)?;

        let token: Token = b"return".to_vec();
        let union = |scheme: usize| -> Vec<FileId> {
            let mut found: Vec<FileId> = indices
                .slices_to_scan(scheme, true)
                .iter()
                .filter_map(|index| index.get(&token))
                .flatten()
                .map(|posting| posting.codeblock)
                .collect();
            found.sort();
            found
        };
        // I1(return) = {CB1}, I2(return) = {CB1, CB2, CB5}, I3(return) = {CB1, CB2, CB3, CB5}.
        assert_eq!(union(1), vec![0]);
        assert_eq!(union(2), vec![0, 1, 4]);
        assert_eq!(union(3), vec![0, 1, 2, 4]);
        Ok(())
    }

    /// Section 3.2 and 3.3: at theta = 0.8 the only pair sharing enough tokens is CB1 and CB5,
    /// which share 14 of the 13 required. CB1 and CB2 share only 12 of a required 17 and are
    /// rejected, the rejection the paper walks through in Section 3.2.
    #[test]
    fn nishi_damevski_detects_only_the_cb1_cb5_pair() -> Result<()> {
        let paths = nishi_damevski_paths();
        let items = items_from_paths(&paths);
        let (bow, file_table) = global_bow(&items, 1)?;
        let rankings = bow.token_rankings();
        let indices = DeltaInvertedIndex::new(&file_table, &rankings, 3, ND_THRESHOLD, 1)?;
        let clone_map = detect_clones(
            &rankings,
            &indices,
            ND_THRESHOLD,
            &file_table,
            &ProgressBar::hidden(),
        )?;

        assert_eq!(
            clone_map.keys().copied().collect::<HashSet<FileId>>(),
            HashSet::from([0, 4]),
            "expected CB1 and CB5 to be the only pair, got {clone_map:?}"
        );
        // Whichever of the two is the origin, the other has to point back at it.
        match (&clone_map[&0], &clone_map[&4]) {
            (Either::Left(clones), Either::Right(origin)) => {
                assert_eq!(clones, &HashSet::from([4]));
                assert_eq!(*origin, 0);
            }
            (Either::Right(origin), Either::Left(clones)) => {
                assert_eq!(clones, &HashSet::from([0]));
                assert_eq!(*origin, 4);
            }
            other => panic!("CB1 and CB5 are not linked as a pair: {other:?}"),
        }
        Ok(())
    }

    /// Blocks are taken as origin in input order and an origin claims the clones it finds, so CB1,
    /// which comes before CB5, has to be the origin of the pair.
    ///
    /// This used to come out the other way round. CB5 reached CB1's candidate map with room to
    /// spare, so the pair was lost in verification rather than in filtering, and was only found
    /// later with CB5 as the origin. Verification resumed the origin at the end of its prefix,
    /// which had already stepped over `return`, a token both blocks share four occurrences of.
    #[test]
    fn nishi_damevski_pair_is_claimed_by_the_earlier_block() -> Result<()> {
        let paths = nishi_damevski_paths();
        let items = items_from_paths(&paths);
        let (bow, file_table) = global_bow(&items, 1)?;
        let rankings = bow.token_rankings();
        let indices = DeltaInvertedIndex::new(&file_table, &rankings, 3, ND_THRESHOLD, 1)?;
        let clone_map = detect_clones(
            &rankings,
            &indices,
            ND_THRESHOLD,
            &file_table,
            &ProgressBar::hidden(),
        )?;

        match clone_map.get(&0) {
            Some(Either::Left(clones)) => assert_eq!(clones, &HashSet::from([4])),
            other => panic!("expected CB1 to be the origin of exactly CB5, got {other:?}"),
        }
        assert_eq!(clone_map.get(&4), Some(&Either::Right(0)));
        Ok(())
    }

    /// At full similarity none of the five blocks is a duplicate of another.
    #[test]
    fn nishi_damevski_finds_no_pairs_at_full_similarity() -> Result<()> {
        let paths = nishi_damevski_paths();
        let items = items_from_paths(&paths);
        let (bow, file_table) = global_bow(&items, 1)?;
        let rankings = bow.token_rankings();
        let indices = DeltaInvertedIndex::new(&file_table, &rankings, 3, 1.0, 1)?;
        let clone_map = detect_clones(
            &rankings,
            &indices,
            1.0,
            &file_table,
            &ProgressBar::hidden(),
        )?;
        assert!(
            clone_map.is_empty(),
            "unexpected clone pairs: {clone_map:?}"
        );
        Ok(())
    }

    // ---- guards ----

    #[test]
    fn detect_clones_ignores_files_without_tokens() -> Result<()> {
        // empty.java has no tokens at all, which leaves it without a prefix to filter on.
        let paths = vec![
            format!("{FILES}/empty.java"),
            format!("{FILES}/c_float.json"),
            format!("{FILES}/c_float.copy"),
        ];
        let items = items_from_paths(&paths);
        let (bow, file_table) = global_bow(&items, 1)?;
        assert_eq!(file_table.length(0), 0, "empty.java should have no tokens");
        let rankings = bow.token_rankings();
        let indices = DeltaInvertedIndex::new(&file_table, &rankings, 3, 0.8, 1)?;
        let clone_map = detect_clones(
            &rankings,
            &indices,
            0.8,
            &file_table,
            &ProgressBar::hidden(),
        )?;
        // The empty file is left out, and the two identical ones still pair up.
        assert!(!clone_map.contains_key(&0));
        assert_eq!(
            clone_map.keys().copied().collect::<HashSet<FileId>>(),
            HashSet::from([1, 2])
        );
        Ok(())
    }

    #[test]
    fn thresholds_outside_the_unit_interval_are_rejected() {
        let input = format!("{TEST_DATA}/duplicate_files.csv");
        for threshold in [1.5, 0.0, -0.5] {
            let result = run(
                &input,
                None,
                None,
                true,
                "overlap",
                threshold,
                1,
                &[],
                1,
                "name",
                test_logger(),
            );
            assert!(
                result.is_err(),
                "threshold {threshold} should have been rejected"
            );
        }
    }

    #[test]
    fn delta_index_needs_at_least_one_scheme() -> Result<()> {
        let paths = vec![format!("{FILES}/foo.java")];
        let items = items_from_paths(&paths);
        let (bow, file_table) = global_bow(&items, 1)?;
        let rankings = bow.token_rankings();
        assert!(DeltaInvertedIndex::new(&file_table, &rankings, 0, 0.8, 1).is_err());
        Ok(())
    }

    // ---- verify_candidates ----

    #[test]
    fn verify_candidates_detects_clone_above_threshold() -> Result<()> {
        // Set up: origin=c_float.json (0), candidate=c_float.copy (1) — identical content.
        let paths = vec![
            format!("{FILES}/c_float.json"),
            format!("{FILES}/c_float.copy"),
        ];
        let items = items_from_paths(&paths);
        let threshold = 0.8;
        let (bow, file_table) = global_bow(&items, 1)?;
        let rankings = bow.token_rankings();

        let matcher = Matcher::words_matcher();
        let origin_bow = sorted_bow(&matcher, &file_table, 0, &rankings)?;

        // Seed the candidate map with zero matches so verify_candidates starts fresh.
        let mut candidate_map = CandidateMap::new();
        candidate_map.add_candidate(1, &file_table, 0, Cursor::default());
        let mut clone_map: CloneMap = HashMap::new();

        verify_candidates(
            0,
            &origin_bow,
            &mut candidate_map,
            &mut clone_map,
            0,
            &rankings,
            threshold,
            &file_table,
            &Matcher::words_matcher(),
        )?;

        assert!(
            clone_map.contains_key(&0) || clone_map.contains_key(&1),
            "identical files not recognised as clones by verify_candidates"
        );
        Ok(())
    }

    #[test]
    fn verify_candidates_skips_files_already_in_clone_map() -> Result<()> {
        let paths = vec![
            format!("{FILES}/c_float.json"),
            format!("{FILES}/c_float.copy"),
        ];
        let items = items_from_paths(&paths);
        let threshold = 0.8;
        let (bow, file_table) = global_bow(&items, 1)?;
        let rankings = bow.token_rankings();

        let matcher = Matcher::words_matcher();
        let origin_bow = sorted_bow(&matcher, &file_table, 0, &rankings)?;

        let mut candidate_map = CandidateMap::new();
        candidate_map.add_candidate(1, &file_table, 0, Cursor::default());

        // Pre-populate the clone map so candidate 1 is already claimed.
        let mut clone_map: CloneMap = HashMap::new();
        clone_map.insert(1, Either::Right(99));

        verify_candidates(
            0,
            &origin_bow,
            &mut candidate_map,
            &mut clone_map,
            0,
            &rankings,
            threshold,
            &file_table,
            &Matcher::words_matcher(),
        )?;

        // Candidate 1 should not be re-assigned a new origin.
        assert!(
            matches!(clone_map.get(&1), Some(Either::Right(99))),
            "already-claimed clone was re-assigned"
        );
        Ok(())
    }

    // ---- integration tests ----

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
            &[],
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

    /// At full similarity overlap finds the same groups as the bag of words hash, except that it
    /// leaves the two empty files alone: they have no tokens to compare, so each is its own group.
    #[test]
    fn overlap_files() -> Result<()> {
        test_duplicate_files(
            &format!("{TEST_DATA}/duplicate_files_overlap.csv"),
            "overlap",
        )
    }

    /// Files are compared only against others of their own language, and files of no known
    /// language are not compared at all.
    ///
    /// c_float.json and c_float.copy hold the same content but sit in different languages, so
    /// unlike in `overlap_files` they no longer pair up. empty.c belongs to no listed language and
    /// is left alone as well, which leaves five groups where there were four.
    #[test]
    fn overlap_only_compares_within_a_language() -> Result<()> {
        let input = format!("{TEST_DATA}/duplicate_files_overlap.csv");
        let output = format!("{TEST_DATA}/out_languages.csv");
        let map = format!("{TEST_DATA}/map_languages.csv");
        let languages = format!("{TEST_DATA}/languages.json");
        delete_file(&output, true)?;
        delete_file(&map, true)?;

        run(
            &input,
            Some(&output),
            Some(&map),
            true,
            "overlap",
            1.0,
            1,
            &[&languages],
            1,
            "name",
            test_logger(),
        )?;

        let map_df = open_csv(&map, None, None)?;
        let unique_df = open_csv(&output, None, None)?;

        // Every input file is still accounted for in the map.
        assert_eq!(map_df.height(), 6);
        // Only foo_clone.java is a duplicate now, so five files remain.
        assert_eq!(unique_df.height(), 5);

        let names = map_df.column("name")?.str()?;
        let originals = map_df.column("original")?.str()?;
        let pairs: HashMap<&str, &str> = names
            .into_iter()
            .flatten()
            .zip(originals.into_iter().flatten())
            .collect();

        let copy = format!("{FILES}/c_float.copy");
        let json = format!("{FILES}/c_float.json");
        assert_eq!(
            pairs.get(copy.as_str()),
            Some(&copy.as_str()),
            "c_float.copy should not be a clone of a file in another language"
        );
        assert_eq!(pairs.get(json.as_str()), Some(&json.as_str()));
        // Same language, same tokens: these two still pair up.
        assert_eq!(
            pairs.get(format!("{FILES}/foo_clone.java").as_str()),
            Some(&format!("{FILES}/foo.java").as_str())
        );

        delete_file(&output, false)?;
        delete_file(&map, false)
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
            &[],
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
            &[],
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
            &[],
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
            &[],
            1,
            "wrongcol",
            test_logger(),
        );
        delete_file(&output, true)?;
        assert!(result.is_err());
        Ok(())
    }
}
