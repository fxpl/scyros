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
use indicatif::ProgressBar;
use polars::frame::DataFrame;
use polars::prelude::{DataFrameJoinOps as _, DataType, Field, Schema};
use tracing::{info, warn};

use crate::utils::bow::{Bow, RankedToken, Token};
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
            Arg::new("prefix")
                .short('p')
                .long("prefix")
                .value_name("PREFIX_DEPTH")
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
/// * `similarity` - The name of the similarity criterion: `exact`, `bow` or `overlap`.
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

    let criterion: Criterion =
        Criterion::parse(similarity, Threshold::new(threshold)?, prefix_depth)?;

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
    info!("Starting file processing...\n");

    let paths: Vec<&str> = files
        .column(input_header)?
        .str()?
        .into_iter()
        .flatten()
        .collect();

    let groups: DuplicateGroups = criterion.group(&paths, languages_file_paths, threads, logger)?;
    groups.report(file_count);
    groups.write(&files, input_header, output_path, map_path, logger)
}

/// How two files are judged to be duplicates of one another.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Criterion {
    /// The files must match byte for byte.
    Exact,
    /// The files must hold the same tokens, which ignores their order and any whitespace.
    BagOfWords,
    /// The files must hold enough tokens in common, which also catches files differing by a few
    /// statements.
    Overlap {
        /// The share of tokens a pair has to have in common.
        threshold: Threshold,
        /// How far the prefix used to reject candidates may be deepened.
        prefix_depth: usize,
    },
}

impl Criterion {
    /// Reads a criterion from the name the command line uses for it.
    ///
    /// # Arguments
    ///
    /// * `name` - One of the values the `-s` argument accepts.
    /// * `threshold` - The share of tokens a pair has to have in common, used by `overlap` only.
    /// * `prefix_depth` - How far the prefix may be deepened, used by `overlap` only.
    fn parse(name: &str, threshold: Threshold, prefix_depth: usize) -> Result<Self> {
        match name {
            "exact" => Ok(Criterion::Exact),
            "bow" => Ok(Criterion::BagOfWords),
            "overlap" => Ok(Criterion::Overlap {
                threshold,
                prefix_depth,
            }),
            other => anyhow::bail!("Unknown similarity criterion '{other}'."),
        }
    }

    /// Sorts the files into duplicate groups.
    ///
    /// # Arguments
    ///
    /// * `paths` - The files to sort, in the order the input listed them.
    /// * `languages_file_paths` - Files mapping extensions to languages, used by `overlap` only.
    /// * `threads` - The number of threads to read and tokenize with.
    /// * `logger` - The logger displaying the progress.
    fn group(
        &self,
        paths: &[&str],
        languages_file_paths: &[&str],
        threads: usize,
        logger: &Logger,
    ) -> Result<DuplicateGroups> {
        match self {
            Criterion::Exact => group_by_hash(paths, false, threads),
            Criterion::BagOfWords => group_by_hash(paths, true, threads),
            Criterion::Overlap {
                threshold,
                prefix_depth,
            } => group_by_overlap(
                paths,
                *threshold,
                *prefix_depth,
                languages_file_paths,
                threads,
                logger,
            ),
        }
    }
}

/// The duplicate groups a criterion sorts the files into.
struct DuplicateGroups {
    /// Each file paired with the file standing for its group.
    representative_of: Vec<[String; 2]>,
    /// The file standing for each group.
    representatives: Vec<String>,
    /// The size of each group, in the same order as `representatives`.
    sizes: Vec<u32>,
    /// Files that could not be read, and so were left out of the grouping altogether.
    unreadable: usize,
}

impl DuplicateGroups {
    /// Creates an empty set of groups.
    ///
    /// # Arguments
    ///
    /// * `capacity` - The number of files about to be sorted, used to size the results.
    fn new(capacity: usize) -> Self {
        DuplicateGroups {
            representative_of: Vec::with_capacity(capacity),
            representatives: Vec::new(),
            sizes: Vec::new(),
            unreadable: 0,
        }
    }

    /// Folds the groups found in one corpus into the results of the whole run.
    ///
    /// # Arguments
    ///
    /// * `corpus` - The corpus the groups were found in.
    /// * `clone_map` - The groups found in it.
    fn extend_from(&mut self, corpus: &Corpus, clone_map: &CloneMap) {
        for file in corpus.ids() {
            if let Some(size) = clone_map.group_size(file) {
                self.representatives.push(corpus.path(file).to_string());
                self.sizes.push(size);
            }
            let representative = clone_map.representative_of(file);
            self.representative_of.push([
                corpus.path(file).to_string(),
                corpus.path(representative).to_string(),
            ]);
        }
    }

    /// Records a file that was never compared against anything, and so stands alone.
    ///
    /// # Arguments
    ///
    /// * `path` - The file to record.
    fn add_singleton(&mut self, path: &str) {
        self.representatives.push(path.to_string());
        self.sizes.push(1);
        self.representative_of
            .push([path.to_string(), path.to_string()]);
    }

    /// Logs how the files were distributed between the groups.
    ///
    /// # Arguments
    ///
    /// * `file_count` - The number of files the input listed.
    fn report(&self, file_count: usize) {
        let considered: usize = file_count - self.unreadable;
        if self.unreadable > 0 {
            let percentage = (self.unreadable as f64 / file_count as f64) * 100.0;
            info!(
                "Ignored large files: {} / {:.2} %",
                self.unreadable, percentage
            );
            info!(
                "Remaining files: {} / {:.2} %",
                considered,
                100.0 - percentage
            );
        }

        let unique: usize = self.representatives.len();
        let unique_percentage = (unique as f64 / considered as f64) * 100.0;
        info!("Unique files: {} / {:.2} %", unique, unique_percentage);
        info!(
            "Duplicate files: {} / {:.2} %",
            considered - unique,
            100.0 - unique_percentage
        );

        let largest_group: u32 = self.sizes.iter().max().copied().unwrap_or_default();
        info!(
            "Most duplicated file: {} times / {:.2} %",
            largest_group,
            (largest_group as f64 / considered as f64) * 100.0
        );
    }

    /// Writes the groups out as the two CSV files the command produces.
    ///
    /// # Arguments
    ///
    /// * `files` - The input rows, which the unique-files output carries over.
    /// * `input_header` - The name of the column holding the file paths.
    /// * `output_path` - Where to write the unique files.
    /// * `map_path` - Where to write the mapping from each file to its representative.
    /// * `logger` - The logger displaying the progress.
    fn write(
        self,
        files: &DataFrame,
        input_header: &str,
        output_path: &str,
        map_path: &str,
        logger: &Logger,
    ) -> Result<()> {
        log_write_rows(
            logger,
            map_path,
            [input_header, "original"],
            self.representative_of,
        )?;

        let clusters = DataFrame::new(vec![
            polars::prelude::Column::new(input_header.into(), self.representatives),
            polars::prelude::Column::new("count".into(), self.sizes),
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

/// Groups files that hash to the same value
///
/// # Arguments
///
/// * `paths` - The files to group.
/// * `bag_of_words` - Whether to hash the bag of tokens rather than the file contents.
/// * `threads` - The number of threads to read and hash with.
fn group_by_hash(paths: &[&str], bag_of_words: bool, threads: usize) -> Result<DuplicateGroups> {
    let workers: Vec<Matcher> = (0..threads).map(|_| Matcher::words_matcher()).collect();
    let progress = ProgressBar::new(paths.len() as u64);
    progress.set_style(
        indicatif::ProgressStyle::default_bar().template("{elapsed} {wide_bar} {percent}%")?,
    );

    // The first file to reach a given hash stands for every file that reaches it afterwards.
    let mut first_seen: HashMap<Hash, usize> = HashMap::new();
    let mut representatives: Vec<String> = Vec::new();
    let mut sizes: Vec<u32> = Vec::new();
    let mut representative_of: Vec<[String; 2]> = Vec::with_capacity(paths.len());
    let mut unreadable: usize = 0;

    parallel_pipeline(
        paths,
        workers,
        |matcher: &mut Matcher, name: &&str| -> Result<(&str, Option<Hash>)> {
            match load_file(name, MAX_FILE_SIZE)? {
                Ok(contents) => {
                    let hash: Hash = if bag_of_words {
                        blake3::hash(&matcher.bag_of_words(&contents, true).serialize())
                    } else {
                        blake3::hash(&contents)
                    };
                    Ok((name, Some(hash)))
                }
                Err(_) => Ok((name, None)),
            }
        },
        |(name, opt_hash)| {
            match opt_hash {
                None => unreadable += 1,
                Some(hash) => {
                    let group = *first_seen.entry(hash).or_insert_with(|| {
                        representatives.push(name.to_string());
                        sizes.push(0);
                        representatives.len() - 1
                    });
                    sizes[group] += 1;
                    representative_of.push([name.to_string(), representatives[group].clone()]);
                    progress.inc(1);
                }
            }
            Ok(())
        },
    )?;
    progress.finish();

    Ok(DuplicateGroups {
        representative_of,
        representatives,
        sizes,
        unreadable,
    })
}

/// Groups files by how many tokens they have in common.
///
/// # Arguments
///
/// * `paths` - The files to group.
/// * `threshold` - The share of tokens a pair has to have in common.
/// * `prefix_depth` - How far the prefix used to reject candidates may be deepened.
/// * `languages_file_paths` - Files mapping extensions to languages.
/// * `threads` - The number of threads to read and tokenize with.
/// * `logger` - The logger displaying the progress.
fn group_by_overlap(
    paths: &[&str],
    threshold: Threshold,
    prefix_depth: usize,
    languages_file_paths: &[&str],
    threads: usize,
    logger: &Logger,
) -> Result<DuplicateGroups> {
    let keyword_files: KeywordFiles = logger.run_task("Loading languages", || {
        KeywordFiles::new(false).add_files(languages_file_paths, true)
    })?;

    let mut by_language: HashMap<String, Vec<&str>> = HashMap::new();
    let mut unclassified: Vec<&str> = Vec::new();
    if languages_file_paths.is_empty() {
        warn!("No language file given: every file is compared against every other, regardless of language.");
        by_language.insert("all".to_string(), paths.to_vec());
    } else {
        for name in paths {
            match keyword_files.file_language(name) {
                Some(language) => by_language.entry(language).or_default().push(name),
                None => unclassified.push(name),
            }
        }
    }

    let progress = ProgressBar::new(paths.len() as u64);
    progress.set_style(
        indicatif::ProgressStyle::default_bar().template("{elapsed} {wide_bar} {percent}%")?,
    );

    let mut groups = DuplicateGroups::new(paths.len());

    for (language, group_paths) in &by_language {
        info!("{}: {} files", language, group_paths.len());
        // Identifiers are local to a group, and so are the rarity ranking and the index built
        // from it.
        let corpus = Corpus::build(group_paths, threads)?;
        let index = DeltaInvertedIndex::new(&corpus, prefix_depth, threshold, threads)?;
        let clone_map = detect_clones(&corpus, &index, threshold, &progress)?;
        groups.extend_from(&corpus, &clone_map);
    }

    if !unclassified.is_empty() {
        info!("Unknown language, left uncompared: {}", unclassified.len());
    }
    for name in unclassified {
        groups.add_singleton(name);
        progress.inc(1);
    }
    progress.finish();

    Ok(groups)
}

/// A set of code blocks compared against one another.
struct Corpus {
    /// Map between file identifiers and their paths on disk.
    paths: Vec<Box<str>>,
    /// Map between file identifiers and their lengths in tokens.
    lengths: Vec<u32>,
    /// Every token of the corpus, ranked from rarest to most common.
    rankings: HashMap<Token, usize>,
    /// The tokenizer every code block is read with.
    matcher: Matcher,
}

impl Corpus {
    /// Reads every code block once, to count its tokens and to rank the tokens of the corpus by
    /// how rare they are, as described in Section 3.3.1 of:
    ///
    /// Hitesh Sajnani, Vaibhav Saini, Jeffrey Svajlenko, Chanchal K. Roy, and Cristina V. Lopes. 2016.
    /// SourcererCC: scaling code clone detection to big-code.
    /// In Proceedings of the 38th International Conference on Software Engineering (ICSE '16).
    /// Association for Computing Machinery, New York, NY, USA, 1157–1168.
    /// [https://doi.org/10.1145/2884781.2884877]
    ///
    /// Blocks that cannot be read are kept, with a length of zero, so that identifiers still line
    /// up with the input.
    ///
    /// # Arguments
    ///
    /// * `paths` - The code blocks to compare, in the order their identifiers follow.
    /// * `threads` - The number of threads to read and tokenize with.
    fn build(paths: &[&str], threads: usize) -> Result<Self> {
        let items: Vec<(FileId, &str)> = paths.iter().copied().enumerate().collect();
        let mut corpus_bow: Bow = Bow::new(true);
        let mut lengths: Vec<u32> = vec![0u32; items.len()];
        let workers: Vec<Matcher> = (0..threads).map(|_| Matcher::words_matcher()).collect();

        parallel_pipeline(
            &items,
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
                    lengths[file_id] = file_bow.sum();
                    corpus_bow.extend(file_bow);
                }
                Ok(())
            },
        )?;

        Ok(Corpus {
            paths: items.iter().map(|(_, path)| (*path).into()).collect(),
            lengths,
            rankings: corpus_bow.token_rankings(),
            matcher: Matcher::words_matcher(),
        })
    }

    /// The identifiers of every code block in the corpus.
    fn ids(&self) -> impl Iterator<Item = FileId> {
        0..self.paths.len()
    }

    /// The number of tokens in a code block, counting repeats.
    ///
    /// # Arguments
    ///
    /// * `codeblock` - The code block to measure.
    fn length(&self, codeblock: FileId) -> u32 {
        self.lengths[codeblock]
    }

    /// The path a code block was read from.
    ///
    /// # Arguments
    ///
    /// * `codeblock` - The code block to locate.
    fn path(&self, codeblock: FileId) -> &str {
        &self.paths[codeblock]
    }

    /// Where a token sits in the rarity ranking, rarest first.
    ///
    /// # Arguments
    ///
    /// * `token` - A token of the corpus. Tokens from outside it have no rank and are an error.
    fn rank(&self, token: &Token) -> Result<usize> {
        self.rankings.get(token).copied().with_context(|| {
            format!(
                "Token not found in global ranking: {}",
                String::from_utf8_lossy(token)
            )
        })
    }

    /// Reads a code block and returns its tokens in the corpus rarity order.
    ///
    /// # Arguments
    ///
    /// * `codeblock` - The code block to read.
    fn sorted_tokens(&self, codeblock: FileId) -> Result<Vec<RankedToken<'_>>> {
        let contents = load_file(self.path(codeblock), MAX_FILE_SIZE)?
            .map_err(|_| anyhow::anyhow!("File too large at path '{}'", self.path(codeblock)))?;
        self.matcher
            .bag_of_words(&contents, true)
            .sort_by(&self.rankings)
    }
}

/// The fraction of tokens two code blocks must share to count as duplicates of one another.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Threshold(f64);

impl Threshold {
    /// Builds a threshold, rejecting values outside the interval (0, 1].
    /// # Arguments
    ///
    /// * `value` - The fraction of shared tokens required, greater than 0 and at most 1.
    fn new(value: f64) -> Result<Self> {
        ensure!(
            value > 0.0 && value <= 1.0,
            "Similarity threshold must be greater than 0 and at most 1, got {value}."
        );
        Ok(Threshold(value))
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
    fn prefix_length(&self, token_count: u32) -> u32 {
        token_count - (token_count as f64 * self.0).ceil() as u32 + 1
    }

    /// The number of tokens two code blocks must share to be a clone pair, as described in
    /// Section 3.1 of:
    ///
    /// Hitesh Sajnani, Vaibhav Saini, Jeffrey Svajlenko, Chanchal K. Roy, and Cristina V. Lopes. 2016.
    /// SourcererCC: scaling code clone detection to big-code.
    /// In Proceedings of the 38th International Conference on Software Engineering (ICSE '16).
    /// Association for Computing Machinery, New York, NY, USA, 1157–1168.
    /// [https://doi.org/10.1145/2884781.2884877]
    ///
    /// # Arguments
    ///
    /// * `origin_token_count` - The total number of tokens in the origin code block.
    /// * `candidate_token_count` - The total number of tokens in the candidate code block.
    fn required_matches(&self, origin_token_count: u32, candidate_token_count: u32) -> u32 {
        (max(origin_token_count, candidate_token_count) as f64 * self.0).ceil() as u32
    }

    /// The fewest tokens a code block can hold and still be a clone of one of `origin_token_count`
    /// tokens. A shorter candidate cannot reach the threshold however well it matches, so it is
    /// rejected without being looked at.
    ///
    /// # Arguments
    ///
    /// * `origin_token_count` - The total number of tokens in the origin code block.
    fn shortest_possible_clone(&self, origin_token_count: u32) -> u32 {
        self.required_matches(origin_token_count, origin_token_count)
    }
}

/// Returns how many of the leading entries in `sorted_bow` are needed for their
/// combined cumulative frequency to reach `prefix_length`.
///
/// # Arguments
///
/// * `sorted_bow` - The rank-sorted bag of words for a code block with cumulative frequencies.
/// * `prefix_length` - The target cumulative frequency to reach with the prefix.
fn weighted_prefix_end(sorted_bow: &[RankedToken], prefix_length: u32) -> Result<usize> {
    for (idx, ranked) in sorted_bow.iter().enumerate() {
        if ranked.cumulative >= prefix_length {
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
/// * `corpus` - The code blocks being compared, which the lengths are read from.
/// * `progress` - Advanced once per code block; pass a hidden bar to silence it.
fn detect_clones(
    corpus: &Corpus,
    delta_inverted_index: &DeltaInvertedIndex,
    threshold: Threshold,
    progress: &ProgressBar,
) -> Result<CloneMap> {
    let mut clone_map: CloneMap = CloneMap::new();

    for origin in corpus.ids() {
        progress.inc(1);
        // A block already known to be a clone of an earlier one does not need a search of its own,
        // since its own clones are found through that earlier block.
        if clone_map.contains(origin) {
            continue;
        }
        let origin_token_count: u32 = corpus.length(origin);
        // A block with no tokens has no prefix to filter on, and no overlap to measure against
        // anything. It is reported unique rather than compared.
        if origin_token_count == 0 {
            continue;
        }
        let sorted_tokens: Vec<RankedToken> = corpus.sorted_tokens(origin)?;
        let mut candidate_map = CandidateMap::new();

        // Token where the 1-prefix ends, used to determine the starting point for the prefix schemes.
        let initial_prefix_end: usize =
            weighted_prefix_end(&sorted_tokens, threshold.prefix_length(origin_token_count))?;
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

            let mut origin_cursor: Cursor = Cursor::new();
            for (position, origin_token) in sorted_tokens.iter().enumerate().take(scheme_end) {
                origin_cursor.advance(origin_token.frequency);
                let new_token: bool = position + 1 == prefix_end;
                filtering_cost += delta_inverted_index.token_filtering_cost(
                    origin_token.token,
                    scheme,
                    new_token,
                );
                for posting in delta_inverted_index
                    .slices_to_scan(scheme, new_token)
                    .iter()
                    .filter_map(|index| index.get(origin_token.token))
                    .flatten()
                {
                    // Blocks already placed in a group are settled and are not searched again.
                    if clone_map.contains(posting.codeblock) {
                        continue;
                    }
                    candidate_map.consider(
                        posting,
                        origin_token,
                        origin_token_count,
                        corpus,
                        threshold,
                    );
                }
            }
            if scheme == 1 {
                //apply updates for the first prefix scheme before estimating costs since it relies on min/max length
                candidate_map.apply_pending_updates(corpus);
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
            candidate_map.apply_pending_updates(corpus);
            best_prefix = scheme;
        }

        verify_candidates(
            origin,
            &sorted_tokens,
            &mut candidate_map,
            &mut clone_map,
            best_prefix,
            threshold,
            corpus,
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
/// * `corpus` - The code blocks being compared, which the lengths are read from.
/// * `word_matcher` - The tokenizer
fn verify_candidates(
    origin_codeblock: FileId,
    sorted_tokens: &[RankedToken],
    candidate_map: &mut CandidateMap,
    clone_map: &mut CloneMap,
    p_prefix: usize,
    threshold: Threshold,
    corpus: &Corpus,
) -> Result<()> {
    let origin_token_count = corpus.length(origin_codeblock);
    let origin_unique_tokens = sorted_tokens.len();
    for candidate in candidate_map
        .candidates_with_at_least(p_prefix as u32)
        .collect::<HashSet<FileId>>()
    {
        if clone_map.contains(candidate) {
            continue;
        }
        if candidate == origin_codeblock {
            continue; //skip comparing the code block to itself
        }
        let mut origin_last_seen_token = Cursor::new();

        // load code block, sort tokens by global frequency, calculate similarity, if above threshold add to clone map
        let vectored_candidate_bow = corpus.sorted_tokens(candidate)?;
        let candidate_token_count: u32 = corpus.length(candidate);
        let candidate_unique_tokens: usize = vectored_candidate_bow.len();
        let current_threshold: u32 =
            threshold.required_matches(origin_token_count, candidate_token_count);
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
            let origin_token = sorted_tokens[origin_last_seen_token.position];
            let candidate_token = vectored_candidate_bow[candidate_last_seen_token.position + 1];

            let origin_rank = corpus.rank(origin_token.token)?;
            let candidate_rank = corpus.rank(candidate_token.token)?;

            if current_matches >= current_threshold {
                break;
            } else if upper_bound + current_matches >= current_threshold {
                if origin_token.token == candidate_token.token {
                    new_matches += min(origin_token.frequency, candidate_token.frequency);
                    candidate_last_seen_token.advance(candidate_token.frequency);
                    origin_last_seen_token.advance(origin_token.frequency);
                } else if origin_rank > candidate_rank {
                    // The candidate holds the rarer token, so the origin cannot match it.
                    candidate_last_seen_token.advance(candidate_token.frequency);
                } else {
                    origin_last_seen_token.advance(origin_token.frequency);
                }
            } else {
                break;
            }
        }
        candidate_map.add_candidate(candidate, corpus, new_matches, candidate_last_seen_token);
        if candidate_map.get_token_matches(candidate) >= current_threshold {
            clone_map.record(origin_codeblock, candidate);
        }
    }
    Ok(())
}

/// Which side of a duplicate group a code block sits on.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Membership {
    /// The block stands for its group, and holds the blocks found to be duplicates of it.
    Representative(HashSet<FileId>),
    /// The block is a duplicate of the block it names.
    DuplicateOf(FileId),
}

/// The duplicate groups found so far.
#[derive(Debug, Default)]
struct CloneMap {
    membership: HashMap<FileId, Membership>,
}

impl CloneMap {
    /// Creates an empty map.
    fn new() -> Self {
        CloneMap::default()
    }

    /// Whether a code block already belongs to a group.
    ///
    /// # Arguments
    ///
    /// * `codeblock` - The code block to look for.
    fn contains(&self, codeblock: FileId) -> bool {
        self.membership.contains_key(&codeblock)
    }

    /// Records that one code block is a duplicate of another, from both sides.
    ///
    /// # Arguments
    ///
    /// * `representative` - The block that stands for the group.
    /// * `duplicate` - The block found to be a duplicate of it.
    fn record(&mut self, representative: FileId, duplicate: FileId) {
        match self
            .membership
            .entry(representative)
            .or_insert_with(|| Membership::Representative(HashSet::new()))
        {
            Membership::Representative(duplicates) => {
                duplicates.insert(duplicate);
            }
            entry => *entry = Membership::Representative(HashSet::from([duplicate])),
        }
        self.membership
            .insert(duplicate, Membership::DuplicateOf(representative));
    }

    /// The block standing for the group a code block belongs to, which is the block itself unless
    /// it was found to be a duplicate of an earlier one.
    ///
    /// # Arguments
    ///
    /// * `codeblock` - The code block to resolve.
    fn representative_of(&self, codeblock: FileId) -> FileId {
        match self.membership.get(&codeblock) {
            Some(Membership::DuplicateOf(representative)) => *representative,
            _ => codeblock,
        }
    }

    /// The size of the group a code block stands for, counting the block itself, or `None` if the
    /// block is a duplicate and so stands for nothing. A block in no group at all stands for a
    /// group of one.
    ///
    /// # Arguments
    ///
    /// * `codeblock` - The code block to size the group of.
    fn group_size(&self, codeblock: FileId) -> Option<u32> {
        match self.membership.get(&codeblock) {
            Some(Membership::DuplicateOf(_)) => None,
            Some(Membership::Representative(duplicates)) => Some(duplicates.len() as u32 + 1),
            None => Some(1),
        }
    }

    /// How a code block takes part in a group, if it takes part in one at all.
    ///
    /// # Arguments
    ///
    /// * `codeblock` - The code block to look up.
    #[cfg(test)]
    fn membership(&self, codeblock: FileId) -> Option<&Membership> {
        self.membership.get(&codeblock)
    }

    /// The code blocks that belong to a group, in no particular order.
    #[cfg(test)]
    fn codeblocks(&self) -> impl Iterator<Item = FileId> + '_ {
        self.membership.keys().copied()
    }

    /// Whether no group has been found at all.
    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.membership.is_empty()
    }
}

/// How far a walk through one code block's rank-sorted tokens has got.
///
/// Carrying the running frequency alongside the index lets a walk say how many tokens it has left
/// without looking back over the ones it has passed, which is what the upper bound on a pair's
/// remaining overlap is built from.
///
/// The two sides of a comparison read `position` differently, and each is consistent with itself:
/// a cursor over the origin points at the next token to read, while a cursor stored against a
/// candidate points at the last token that matched, so there the next to read is `position + 1`.
/// Confusing the two is what makes a pair lose the tokens between them.
#[derive(Debug, Clone, Copy, Default)]
struct Cursor {
    /// Index into the code block's rank-sorted tokens; see above for which token it names.
    position: usize,
    /// Frequencies of every token covered so far, counting repeats.
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
    /// * `corpus` - The code blocks being compared, which the lengths are read from.
    /// * `token_rankings` - The mapping of tokens to their frequency in the global corpus, used to
    ///   determine their rank and build the prefix schemes.
    /// * `max_scheme` - The maximum prefix scheme to build in the delta index (e.g., 10 for 1-prefix to 10-prefix).
    /// * `threshold` - The similarity threshold for duplicate detection (0.0 to 1.0), used to
    ///   determine the length of the prefixes.
    /// * `threads` - The number of threads to use for parallel processing when building the index.
    fn new(
        corpus: &'w Corpus,
        max_scheme: usize,
        threshold: Threshold,
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
        parallel_pipeline(
            &corpus.ids().collect::<Vec<_>>(),
            (0..threads).map(|_| ()).collect(),
            |_: &mut (), file_id: &FileId| -> Result<Option<(FileId, Vec<RankedToken<'w>>)>> {
                Ok(Some((*file_id, corpus.sorted_tokens(*file_id)?)))
            },
            |res_opt| {
                if let Some((file_id, vector_bow)) = res_opt {
                    let mut scheme: usize = 1;
                    let prefix_length: u32 = threshold.prefix_length(corpus.length(file_id));
                    for (idx, ranked) in vector_bow.into_iter().enumerate() {
                        res.add(
                            scheme,
                            ranked.token,
                            Posting {
                                codeblock: file_id,
                                occurrences: ranked.frequency,
                                cursor: Cursor {
                                    position: idx,
                                    cumulative: ranked.cumulative,
                                },
                            },
                        );
                        if ranked.cumulative >= prefix_length {
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
    /// Creates an empty map, ready for one origin's search.
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            match_histogram: HashMap::new(),
            min_length: u32::MAX,
            max_length: 0,
            pending_updates: Vec::new(),
        }
    }

    /// How many tokens a candidate has been found to share with the origin so far, counting only
    /// what has been committed. A code block that is not a candidate shares nothing.
    ///
    /// # Arguments
    ///
    /// * `codeblock` - The candidate to count the matches of.
    fn get_token_matches(&self, codeblock: FileId) -> u32 {
        self.entries
            .get(&codeblock)
            .map(|entry| entry.matches)
            .unwrap_or(0)
    }

    /// Stages matches against a candidate, to be committed only if the prefix scheme that found
    /// them turns out to be worth its cost.
    ///
    /// # Arguments
    ///
    /// * `codeblock` - The candidate the matches were found against.
    /// * `new_matches` - How many tokens matched here.
    /// * `last_token_seen_pos` - Index of the matched token within the candidate's own tokens.
    /// * `last_token_seen_cumul_freq` - Frequencies of the candidate's tokens up to and including
    ///   that one.
    fn add_pending_update(
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

    /// Weighs one code block holding a prefix token of the origin, and keeps it if it could still
    /// reach the threshold.
    ///
    /// A block is dropped when it is too short to reach the threshold however well it matches, or
    /// when everything it has left to offer, added to what it has matched already, still falls
    /// short. This is the token position filtering of Section 3.3.2 of:
    ///
    /// Hitesh Sajnani, Vaibhav Saini, Jeffrey Svajlenko, Chanchal K. Roy, and Cristina V. Lopes. 2016.
    /// SourcererCC: scaling code clone detection to big-code.
    /// In Proceedings of the 38th International Conference on Software Engineering (ICSE '16).
    /// Association for Computing Machinery, New York, NY, USA, 1157–1168.
    /// [https://doi.org/10.1145/2884781.2884877]
    ///
    /// Blocks that survive are staged rather than committed, since the prefix scheme that found
    /// them may still be judged too expensive to keep.
    ///
    /// # Arguments
    ///
    /// * `posting` - Where the token sits in the candidate, and how often it occurs there.
    /// * `origin_token` - The prefix token of the origin that was looked up.
    /// * `origin_token_count` - The total number of tokens in the origin.
    /// * `corpus` - The paths and token counts of the corpus.
    /// * `threshold` - The share of tokens a pair has to have in common.
    fn consider(
        &mut self,
        posting: &Posting,
        origin_token: &RankedToken,
        origin_token_count: u32,
        corpus: &Corpus,
        threshold: Threshold,
    ) {
        let candidate_token_count: u32 = corpus.length(posting.codeblock);
        if candidate_token_count < threshold.shortest_possible_clone(origin_token_count) {
            return;
        }

        let new_matches: u32 = min(origin_token.frequency, posting.occurrences);
        // The most the two could still end up sharing: what matches here, plus every token
        // neither has reached yet.
        let upper_bound: u32 = min(
            origin_token_count - origin_token.cumulative,
            candidate_token_count - posting.cursor.cumulative,
        );
        let required: u32 = threshold.required_matches(origin_token_count, candidate_token_count);

        if self.get_token_matches(posting.codeblock) + upper_bound + new_matches >= required {
            self.add_pending_update(
                posting.codeblock,
                new_matches,
                posting.cursor.position,
                posting.cursor.cumulative,
            );
        }
    }

    /// Commits every staged match, emptying the staging list.
    ///
    /// Called once a prefix scheme has been judged worth keeping. A scheme that is rejected leaves
    /// its staged matches to be dropped instead.
    ///
    /// # Arguments
    ///
    /// * `corpus` - The code blocks being compared, which the lengths are read from.
    fn apply_pending_updates(&mut self, corpus: &Corpus) {
        let updates = self.pending_updates.drain(..).collect::<Vec<_>>();
        for (codeblock, candidate_entry) in updates {
            self.add_candidate(
                codeblock,
                corpus,
                candidate_entry.matches,
                candidate_entry.last_seen_token,
            );
        }
    }

    /// Adds matches to a candidate, entering it into the map if it is not already there.
    ///
    /// The candidate is moved to the histogram bucket for its new total, so that the cost estimate
    /// can find the candidates at a given depth without walking them all.
    ///
    /// # Arguments
    ///
    /// * `codeblock` - The candidate the matches were found against.
    /// * `corpus` - The code blocks being compared, which the lengths are read from.
    /// * `new_matches` - Matches to add to those already recorded for this candidate.
    /// * `last_seen_token` - How far into the candidate's own tokens the comparison has reached.
    fn add_candidate(
        &mut self,
        codeblock: FileId,
        corpus: &Corpus,
        new_matches: u32,
        last_seen_token: Cursor,
    ) {
        let entry = match self.entries.entry(codeblock) {
            Entry::Occupied(occupied) => occupied.into_mut(),
            Entry::Vacant(vacant) => {
                let length: u32 = corpus.length(codeblock);
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

    /// The midpoint between the shortest and the longest candidate in the map, which stands in for
    /// the length of a typical one when estimating what verification will cost.
    ///
    /// This is the Map estimate of Section 3.5.2 of the report: the bounds are taken over the
    /// candidates of this origin rather than over the whole corpus, so that one very long or very
    /// short code block elsewhere in the corpus does not skew the estimate.
    fn mid_length(&self) -> u32 {
        if self.entries.is_empty() {
            0
        } else {
            (self.min_length + self.max_length) / 2
        }
    }

    /// The candidates that have matched at least `n` tokens of the origin.
    ///
    /// A candidate that has matched more than the prefix scheme demands is still worth verifying,
    /// so this is the only question detection asks of the histogram. Reading the answer out of the
    /// buckets avoids walking every candidate, which matters because the cost estimate asks it once
    /// per prefix scheme.
    ///
    /// # Arguments
    ///
    /// * `n` - The fewest matches a candidate must have to be returned.
    fn candidates_with_at_least(&self, n: u32) -> impl Iterator<Item = FileId> + '_ {
        self.match_histogram
            .iter()
            .filter(move |(&matches, _)| matches >= n)
            .flat_map(|(_, bucket)| bucket.iter().copied())
    }

    /// How far into a candidate's own tokens the comparison has reached, which is where
    /// verification picks it up again.
    ///
    /// # Arguments
    ///
    /// * `codeblock` - The candidate to locate. Asking about a code block that is not a candidate
    ///   is an error, since there is no comparison to resume.
    fn last_seen_token(&self, codeblock: FileId) -> Result<Cursor> {
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
    fn verification_cost(&self, n: u32, origin_token_count: u32) -> u32 {
        let number_of_candidates: u32 = self.candidates_with_at_least(n).count() as u32; //the candidates that have already reached n matches

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

    fn ranked(token: &Token, frequency: u32, cumulative: u32) -> RankedToken<'_> {
        RankedToken {
            token,
            frequency,
            cumulative,
        }
    }

    /// A corpus of code blocks with the given token counts, named `file0.rs` and upwards, and
    /// with no ranking. Enough for anything that only reads lengths and paths.
    fn make_corpus(lengths: Vec<u32>) -> Corpus {
        make_corpus_ranked(lengths, HashMap::new())
    }

    /// A corpus as above, but carrying a ranking.
    fn make_corpus_ranked(lengths: Vec<u32>, rankings: HashMap<Token, usize>) -> Corpus {
        Corpus {
            paths: lengths
                .iter()
                .enumerate()
                .map(|(i, _)| format!("file{i}.rs").into_boxed_str())
                .collect(),
            lengths,
            rankings,
            matcher: Matcher::words_matcher(),
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
    fn threshold_rejects_values_outside_its_interval() {
        assert!(Threshold::new(0.8).is_ok());
        assert!(Threshold::new(1.0).is_ok());
        assert!(Threshold::new(0.0).is_err());
        assert!(Threshold::new(-0.5).is_err());
        assert!(Threshold::new(1.5).is_err());
    }

    #[test]
    fn prefix_length_random() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(0);

        for _ in 0..10_000 {
            let token_count: u32 = rng.gen_range(1..=10_000);
            let threshold = Threshold(rng.gen_range(f64::MIN_POSITIVE..=1.0));
            let result = threshold.prefix_length(token_count);
            assert!(result >= 1);
            assert!(result <= token_count);

            // Full similarity leaves a prefix of one token, and no similarity at all leaves every
            // token in the prefix. Neither is reachable through Threshold::new.
            assert_eq!(Threshold(1.0).prefix_length(token_count), 1);
            assert_eq!(Threshold(0.0).prefix_length(token_count), token_count + 1);

            assert_eq!(threshold.prefix_length(1), 1);
        }
    }

    #[test]
    fn prefix_length_partial_threshold() {
        assert_eq!(Threshold(0.8).prefix_length(10), 3);
        assert_eq!(Threshold(0.5).prefix_length(10), 6);
    }

    #[test]
    fn required_matches_random() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(0);

        for _ in 0..10_000 {
            let token_count1: u32 = rng.gen_range(1..=10_000);
            let token_count2: u32 = rng.gen_range(1..=10_000);
            let threshold = Threshold(rng.gen_range(f64::MIN_POSITIVE..=1.0));
            let result = threshold.required_matches(token_count1, token_count2);
            let result_sym = threshold.required_matches(token_count2, token_count1);
            assert!(result >= 1);
            assert_eq!(result, result_sym);
            assert!(result <= token_count1.max(token_count2));

            assert_eq!(
                Threshold(1.0).required_matches(token_count1, token_count2),
                token_count1.max(token_count2)
            );
            assert_eq!(
                Threshold(0.0).required_matches(token_count1, token_count2),
                0
            );
            assert_eq!(threshold.required_matches(1, 1), 1);
        }
    }

    #[test]
    fn required_matches_det() {
        assert_eq!(Threshold(0.8).required_matches(10, 10), 8);
        assert_eq!(Threshold(0.8).required_matches(10, 8), 8);
        assert_eq!(Threshold(0.75).required_matches(10, 10), 8);
    }

    #[test]
    fn shortest_possible_clone_is_the_origin_measured_against_itself() {
        let threshold = Threshold(0.8);
        assert_eq!(threshold.shortest_possible_clone(10), 8);
        // Anything shorter cannot reach the threshold however well it matches.
        assert!(threshold.required_matches(10, 7) > 7);
    }

    // ---- weighted_prefix_end ----

    #[test]
    fn weighted_prefix_end_first_element() -> Result<()> {
        let w1: Token = b"foo".to_vec();
        let bow: Vec<RankedToken> = vec![ranked(&w1, 3, 3)];
        // cumulative=3 >= prefix_length=3 at idx 0 → return 1
        assert_eq!(weighted_prefix_end(&bow, 3)?, 1);
        Ok(())
    }

    #[test]
    fn weighted_prefix_end_second_element() -> Result<()> {
        let w1: Token = b"foo".to_vec();
        let w2: Token = b"bar".to_vec();
        let bow: Vec<RankedToken> = vec![ranked(&w1, 3, 3), ranked(&w2, 2, 5)];
        // cumulative=3 < 4, cumulative=5 >= 4 at idx 1 → return 2
        assert_eq!(weighted_prefix_end(&bow, 4)?, 2);
        Ok(())
    }

    #[test]
    fn weighted_prefix_end_unreachable_returns_error() {
        let w1: Token = b"foo".to_vec();
        let bow: Vec<RankedToken> = vec![ranked(&w1, 3, 3)];
        assert!(weighted_prefix_end(&bow, 10).is_err());
    }

    #[test]
    fn corpus_rank_found() -> Result<()> {
        let token: Token = b"hello".to_vec();
        let corpus = make_corpus_ranked(vec![], HashMap::from([(token.clone(), 42)]));
        assert_eq!(corpus.rank(&token)?, 42);
        Ok(())
    }

    #[test]
    fn corpus_rank_missing() {
        let token: Token = b"missing".to_vec();
        let corpus = make_corpus_ranked(vec![], HashMap::new());
        assert!(corpus.rank(&token).is_err());
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
    fn corpus_accessors() {
        let corpus = make_corpus(vec![10, 20, 15]);
        assert_eq!(corpus.length(0), 10);
        assert_eq!(corpus.length(1), 20);
        assert_eq!(corpus.length(2), 15);
        assert_eq!(corpus.path(0), "file0.rs");
        assert_eq!(corpus.path(1), "file1.rs");
        assert_eq!(corpus.path(2), "file2.rs");
        let ids: Vec<_> = corpus.ids().collect();
        assert_eq!(ids, vec![0, 1, 2]);
    }

    // ---- CandidateMap ----

    #[test]
    fn candidate_map_new_is_empty() {
        let cm = CandidateMap::new();
        assert_eq!(cm.get_token_matches(0), 0);
        assert_eq!(cm.mid_length(), 0);
        assert!(cm
            .candidates_with_at_least(1)
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
        let corpus = make_corpus(vec![10]);
        let mut cm = CandidateMap::new();
        cm.add_candidate(
            0,
            &corpus,
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
        let corpus = make_corpus(vec![10]);
        let mut cm = CandidateMap::new();
        cm.add_candidate(
            0,
            &corpus,
            3,
            Cursor {
                position: 2,
                cumulative: 3,
            },
        );
        cm.add_candidate(
            0,
            &corpus,
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
        let corpus = make_corpus(vec![10, 8, 15]);
        let mut cm = CandidateMap::new();
        cm.add_candidate(0, &corpus, 1, Cursor::default()); // length 10
        cm.add_candidate(2, &corpus, 1, Cursor::default()); // length 15
        assert_eq!(cm.mid_length(), 12);
        Ok(())
    }

    #[test]
    fn candidate_map_candidates_with_at_least() -> Result<()> {
        let corpus = make_corpus(vec![10, 10, 10]);
        let mut cm = CandidateMap::new();
        cm.add_candidate(0, &corpus, 3, Cursor::default());
        cm.add_candidate(1, &corpus, 3, Cursor::default());
        cm.add_candidate(2, &corpus, 5, Cursor::default());

        // Two candidates share a bucket, and both come back with the one above them.
        assert_eq!(
            cm.candidates_with_at_least(3).collect::<HashSet<FileId>>(),
            HashSet::from([0, 1, 2])
        );
        assert_eq!(
            cm.candidates_with_at_least(5).collect::<HashSet<FileId>>(),
            HashSet::from([2])
        );
        assert_eq!(cm.candidates_with_at_least(6).count(), 0);
        Ok(())
    }

    #[test]
    fn candidate_map_histogram_updated_on_accumulation() -> Result<()> {
        let corpus = make_corpus(vec![10]);
        let mut cm = CandidateMap::new();
        cm.add_candidate(0, &corpus, 3, Cursor::default());
        assert_eq!(cm.candidates_with_at_least(3).count(), 1);

        cm.add_candidate(0, &corpus, 2, Cursor::default());
        // The candidate moved to the bucket for five matches rather than being listed in both:
        // were the old bucket left behind, asking for three or more would return it twice.
        assert_eq!(cm.candidates_with_at_least(3).count(), 1);
        assert_eq!(cm.candidates_with_at_least(5).count(), 1);
        assert_eq!(cm.candidates_with_at_least(6).count(), 0);
        Ok(())
    }

    #[test]
    fn candidate_map_pending_updates_applied() -> Result<()> {
        let corpus = make_corpus(vec![10, 8]);
        let mut cm = CandidateMap::new();
        cm.add_pending_update(0, 3, 2, 3);
        cm.add_pending_update(1, 2, 1, 2);
        // Not yet applied.
        assert_eq!(cm.get_token_matches(0), 0);
        cm.apply_pending_updates(&corpus);
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
        let corpus = make_corpus(vec![10, 20]);
        let mut cm = CandidateMap::new();
        cm.add_candidate(0, &corpus, 3, Cursor::default()); // length 10, 3 matches
        cm.add_candidate(1, &corpus, 5, Cursor::default()); // length 20, 5 matches
                                                            // n=3: both have >= 3 matches → 2 candidates, no survivors
                                                            // average_length = (10 + 20) / 2 = 15
                                                            // cost = 2 * (10 + 15) = 50
        assert_eq!(cm.verification_cost(3, 10), 50);
        Ok(())
    }

    #[test]
    fn verification_cost_counts_survivors_from_pending() -> Result<()> {
        // candidate 0 already has 2 matches, a pending update will push it to 3
        let corpus = make_corpus(vec![10]);
        let mut cm = CandidateMap::new();
        cm.add_candidate(0, &corpus, 2, Cursor::default());
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

    // ---- CloneMap ----

    #[test]
    fn clone_map_records_both_sides_of_a_group() {
        let mut clone_map = CloneMap::new();
        clone_map.record(0, 1);
        assert_eq!(
            clone_map.membership(0),
            Some(&Membership::Representative(HashSet::from([1])))
        );
        assert_eq!(clone_map.membership(1), Some(&Membership::DuplicateOf(0)));
        assert_eq!(clone_map.representative_of(1), 0);
        assert_eq!(clone_map.representative_of(0), 0);
        assert_eq!(clone_map.group_size(0), Some(2));
        assert_eq!(clone_map.group_size(1), None);
        // A block that belongs to no group at all stands for a group of one.
        assert_eq!(clone_map.group_size(9), Some(1));
        assert_eq!(clone_map.representative_of(9), 9);
    }

    #[test]
    fn clone_map_accumulates_duplicates_of_one_representative() {
        let mut clone_map = CloneMap::new();
        clone_map.record(0, 1);
        clone_map.record(0, 2);
        match clone_map.membership(0).unwrap() {
            Membership::Representative(clones) => {
                assert_eq!(clones.len(), 2);
                assert!(clones.contains(&1) && clones.contains(&2));
            }
            Membership::DuplicateOf(_) => panic!("expected a representative"),
        }
        assert_eq!(clone_map.group_size(0), Some(3));
    }

    // ---- global_bow ----

    const FILES: &str = "tests/data/phases/duplicate_files/files";

    fn path_refs(paths: &[String]) -> Vec<&str> {
        paths.iter().map(|p| p.as_str()).collect()
    }

    #[test]
    fn corpus_build_records_correct_lengths() -> Result<()> {
        let paths = vec![
            format!("{FILES}/foo.java"),
            format!("{FILES}/c_float.json"),
            format!("{FILES}/empty.java"),
        ];
        let corpus = Corpus::build(&path_refs(&paths), 1)?;
        assert_eq!(corpus.ids().count(), 3);
        assert_eq!(corpus.path(0), paths[0]);
        assert_eq!(corpus.path(1), paths[1]);
        assert_eq!(corpus.path(2), paths[2]);
        assert!(corpus.length(0) > 0, "foo.java should have tokens");
        assert!(corpus.length(1) > 0, "c_float.json should have tokens");
        assert_eq!(corpus.length(2), 0, "empty.java should have no tokens");
        assert!(
            !corpus.rankings.is_empty(),
            "the corpus ranking should not be empty"
        );
        Ok(())
    }

    #[test]
    fn global_bow_identical_files_have_same_length() -> Result<()> {
        let paths = vec![
            format!("{FILES}/c_float.json"),
            format!("{FILES}/c_float.copy"),
        ];
        let corpus = Corpus::build(&path_refs(&paths), 1)?;
        assert_eq!(corpus.length(0), corpus.length(1));
        Ok(())
    }

    // ---- sorted_bow ----

    #[test]
    fn sorted_bow_tokens_are_sorted_by_rank() -> Result<()> {
        let paths = vec![format!("{FILES}/foo.java")];
        let corpus = Corpus::build(&path_refs(&paths), 1)?;
        let sorted = corpus.sorted_tokens(0)?;
        assert!(!sorted.is_empty());
        // Ranks must be non-decreasing.
        for w in sorted.windows(2) {
            assert!(
                corpus.rank(w[0].token)? <= corpus.rank(w[1].token)?,
                "not sorted by rank"
            );
        }
        Ok(())
    }

    #[test]
    fn sorted_bow_cumulative_counts_are_non_decreasing() -> Result<()> {
        let paths = vec![format!("{FILES}/foo.java")];
        let corpus = Corpus::build(&path_refs(&paths), 1)?;
        let sorted = corpus.sorted_tokens(0)?;
        for w in sorted.windows(2) {
            assert!(
                w[0].cumulative <= w[1].cumulative,
                "cumulative frequencies not non-decreasing"
            );
        }
        // Final cumulative equals total token count.
        if let Some(last) = sorted.last() {
            assert_eq!(last.cumulative, corpus.length(0));
        }
        Ok(())
    }

    // ---- index_builder ----

    #[test]
    fn index_builder_first_index_is_non_empty() -> Result<()> {
        let paths = vec![format!("{FILES}/foo.java")];
        let corpus = Corpus::build(&path_refs(&paths), 1)?;
        let indices = DeltaInvertedIndex::new(&corpus, 5, Threshold(0.8), 1)?;
        // At least one token from foo.java should appear in the first delta index.
        let first_has_entries = corpus
            .rankings
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
        let threshold = Threshold(0.8);
        let corpus = Corpus::build(&path_refs(&paths), 1)?;
        let indices = DeltaInvertedIndex::new(&corpus, 10, threshold, 1)?;
        let clone_map = detect_clones(&corpus, &indices, threshold, &ProgressBar::hidden())?;
        // The two identical files (ids 0 and 1) must appear together in the clone map.
        let in_map = clone_map.contains(0) || clone_map.contains(1);
        assert!(in_map, "identical files not detected as clones");
        // If 0 is origin, 1 must point back to 0, and vice-versa.
        if let Some(entry) = clone_map.membership(0) {
            match entry {
                Membership::Representative(clones) => assert!(clones.contains(&1)),
                Membership::DuplicateOf(orig) => assert_eq!(*orig, 1),
            }
        }
        Ok(())
    }

    #[test]
    fn detect_clones_distinct_files_are_not_clones() -> Result<()> {
        let paths = vec![format!("{FILES}/foo.java"), format!("{FILES}/c_float.json")];
        let threshold = Threshold(0.95);
        let corpus = Corpus::build(&path_refs(&paths), 1)?;
        let indices = DeltaInvertedIndex::new(&corpus, 10, threshold, 1)?;
        let clone_map = detect_clones(&corpus, &indices, threshold, &ProgressBar::hidden())?;
        // foo.java and c_float.json share very few tokens; neither should be cloned at 0.95.
        let paired = clone_map.contains(0) && clone_map.contains(1) && {
            match (clone_map.membership(0), clone_map.membership(1)) {
                (Some(Membership::Representative(s)), Some(Membership::DuplicateOf(o))) => {
                    s.contains(&1) && *o == 0
                }
                (Some(Membership::DuplicateOf(o)), Some(Membership::Representative(s))) => {
                    s.contains(&0) && *o == 1
                }
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
    const ND_THRESHOLD: Threshold = Threshold(0.8);

    fn nishi_damevski_paths() -> Vec<String> {
        (1..=5).map(|n| format!("{ND_FILES}/cb{n}.java")).collect()
    }

    /// Table 2: the size |t| of each code block, summed from its local token frequencies.
    #[test]
    fn nishi_damevski_block_sizes_match_table_2() -> Result<()> {
        let paths = nishi_damevski_paths();
        let corpus = Corpus::build(&path_refs(&paths), 1)?;
        let sizes: Vec<u32> = corpus.ids().map(|f| corpus.length(f)).collect();
        assert_eq!(sizes, vec![16, 21, 28, 23, 16]);
        Ok(())
    }

    /// Table 2: tokens sorted by global frequency, rarest first, ties broken lexicographically.
    #[test]
    fn nishi_damevski_token_order_matches_table_2() -> Result<()> {
        let paths = nishi_damevski_paths();
        let corpus = Corpus::build(&path_refs(&paths), 1)?;
        let sorted = corpus.sorted_tokens(0)?;
        let tokens: Vec<String> = sorted
            .iter()
            .map(|ranked| String::from_utf8_lossy(ranked.token).into_owned())
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
        let frequencies: Vec<u32> = sorted.iter().map(|ranked| ranked.frequency).collect();
        assert_eq!(frequencies, vec![1, 1, 1, 2, 2, 3, 2, 4]);
        assert_eq!(sorted.last().map(|ranked| ranked.cumulative), Some(16));
        Ok(())
    }

    /// Table 3: the 1-prefix is |t| - ceil(theta * |t|) + 1 tokens long, counting duplicates.
    #[test]
    fn nishi_damevski_prefix_sizes_match_table_3() {
        assert_eq!(ND_THRESHOLD.prefix_length(16), 4);
        assert_eq!(ND_THRESHOLD.prefix_length(21), 5);
    }

    /// Fig. 1: every posting sits in the slice of the scheme that first pulls it into a prefix.
    #[test]
    fn nishi_damevski_delta_index_matches_figure_1() -> Result<()> {
        let paths = nishi_damevski_paths();
        let corpus = Corpus::build(&path_refs(&paths), 1)?;
        let indices = DeltaInvertedIndex::new(&corpus, 3, ND_THRESHOLD, 1)?;

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
        let corpus = Corpus::build(&path_refs(&paths), 1)?;
        let indices = DeltaInvertedIndex::new(&corpus, 3, ND_THRESHOLD, 1)?;

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
        let corpus = Corpus::build(&path_refs(&paths), 1)?;
        let indices = DeltaInvertedIndex::new(&corpus, 3, ND_THRESHOLD, 1)?;
        let clone_map = detect_clones(&corpus, &indices, ND_THRESHOLD, &ProgressBar::hidden())?;

        assert_eq!(
            clone_map.codeblocks().collect::<HashSet<FileId>>(),
            HashSet::from([0, 4]),
            "expected CB1 and CB5 to be the only pair, got {clone_map:?}"
        );
        // Whichever of the two is the origin, the other has to point back at it.
        match (clone_map.membership(0), clone_map.membership(4)) {
            (Some(Membership::Representative(clones)), Some(Membership::DuplicateOf(origin))) => {
                assert_eq!(clones, &HashSet::from([4]));
                assert_eq!(*origin, 0);
            }
            (Some(Membership::DuplicateOf(origin)), Some(Membership::Representative(clones))) => {
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
        let corpus = Corpus::build(&path_refs(&paths), 1)?;
        let indices = DeltaInvertedIndex::new(&corpus, 3, ND_THRESHOLD, 1)?;
        let clone_map = detect_clones(&corpus, &indices, ND_THRESHOLD, &ProgressBar::hidden())?;

        match clone_map.membership(0) {
            Some(Membership::Representative(clones)) => assert_eq!(clones, &HashSet::from([4])),
            other => panic!("expected CB1 to be the origin of exactly CB5, got {other:?}"),
        }
        assert_eq!(clone_map.membership(4), Some(&Membership::DuplicateOf(0)));
        Ok(())
    }

    /// At full similarity none of the five blocks is a duplicate of another.
    #[test]
    fn nishi_damevski_finds_no_pairs_at_full_similarity() -> Result<()> {
        let paths = nishi_damevski_paths();
        let corpus = Corpus::build(&path_refs(&paths), 1)?;
        let indices = DeltaInvertedIndex::new(&corpus, 3, Threshold(1.0), 1)?;
        let clone_map = detect_clones(&corpus, &indices, Threshold(1.0), &ProgressBar::hidden())?;
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
        let corpus = Corpus::build(&path_refs(&paths), 1)?;
        assert_eq!(corpus.length(0), 0, "empty.java should have no tokens");
        let indices = DeltaInvertedIndex::new(&corpus, 3, Threshold(0.8), 1)?;
        let clone_map = detect_clones(&corpus, &indices, Threshold(0.8), &ProgressBar::hidden())?;
        // The empty file is left out, and the two identical ones still pair up.
        assert!(!clone_map.contains(0));
        assert_eq!(
            clone_map.codeblocks().collect::<HashSet<FileId>>(),
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
        let corpus = Corpus::build(&path_refs(&paths), 1)?;
        assert!(DeltaInvertedIndex::new(&corpus, 0, Threshold(0.8), 1).is_err());
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
        let threshold = Threshold(0.8);
        let corpus = Corpus::build(&path_refs(&paths), 1)?;

        let origin_bow = corpus.sorted_tokens(0)?;

        // Seed the candidate map with zero matches so verify_candidates starts fresh.
        let mut candidate_map = CandidateMap::new();
        candidate_map.add_candidate(1, &corpus, 0, Cursor::default());
        let mut clone_map: CloneMap = CloneMap::new();

        verify_candidates(
            0,
            &origin_bow,
            &mut candidate_map,
            &mut clone_map,
            0,
            threshold,
            &corpus,
        )?;

        assert!(
            clone_map.contains(0) || clone_map.contains(1),
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
        let threshold = Threshold(0.8);
        let corpus = Corpus::build(&path_refs(&paths), 1)?;

        let origin_bow = corpus.sorted_tokens(0)?;

        let mut candidate_map = CandidateMap::new();
        candidate_map.add_candidate(1, &corpus, 0, Cursor::default());

        // Pre-populate the clone map so candidate 1 is already claimed.
        let mut clone_map: CloneMap = CloneMap::new();
        clone_map.record(99, 1);

        verify_candidates(
            0,
            &origin_bow,
            &mut candidate_map,
            &mut clone_map,
            0,
            threshold,
            &corpus,
        )?;

        // Candidate 1 should not be re-assigned a new origin.
        assert!(
            matches!(clone_map.membership(1), Some(Membership::DuplicateOf(99))),
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
