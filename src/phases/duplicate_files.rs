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

use std::collections::HashMap;
use std::iter::FromIterator;

use anyhow::{ensure, Context, Result};
use blake3::Hash;
use clap::{Arg, ArgAction, Command};
use indicatif::ProgressBar;
use polars::frame::DataFrame;
use polars::prelude::{DataFrameJoinOps as _, DataType, Field, Schema};
use tracing::info;

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
    // threshold: f64,
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
    let items: Vec<(usize, &str)> = files
        .column(input_header)?
        .str()?
        .into_iter()
        .flatten()
        .enumerate()
        .collect();

    info!("Starting file processing...\n");

    let workers: Vec<Matcher> = (0..threads).map(|_| Matcher::words_matcher()).collect();
    let progress = ProgressBar::new(file_count as u64);
    progress.set_style(
        indicatif::ProgressStyle::default_bar().template("{elapsed} {wide_bar} {percent}%")?,
    );

    let mut hash_map: HashMap<Hash, (usize, &str, u32)> = std::collections::HashMap::new();
    let mut clone_map: HashMap<&str, &str> = HashMap::new();
    let mut big_files: usize = 0;

    parallel_pipeline(
        items,
        workers,
        |matcher: &mut Matcher,
         (idx, name): (usize, &str)|
         -> Result<(usize, &str, Option<Hash>)> {
            match load_file(name, 1024 * 1024 * 1024) {
                Ok(Ok(file_content)) => {
                    let hash: Hash = if similarity == "exact" {
                        blake3::hash(&file_content)
                    } else {
                        blake3::hash(&matcher.bag_of_words(&file_content, true).serialize())
                    };
                    Ok((idx, name, Some(hash)))
                }
                Ok(Err(_)) => Ok((idx, name, None)),
                Err(e) => Err(e),
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
            1,
            "wrongcol",
            test_logger(),
        );
        delete_file(&output, true)?;
        assert!(result.is_err());
        Ok(())
    }
}
