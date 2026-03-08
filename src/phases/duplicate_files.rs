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

//! Detects duplicate files in a dataset, returning only unique files. The input and output are CSV files storing file metadata.
//! The similarity criterion can be either exact match or token-based (i.e., invariant to token order and whitespaces).

use std::collections::HashMap;
use std::iter::FromIterator;

use anyhow::{anyhow, ensure, Context, Error, Result};
use blake3::Hash;
use clap::{Arg, ArgAction, Command};
use indicatif::ProgressBar;
use polars::frame::DataFrame;
use polars::prelude::{DataFrameJoinOps as _, DataType, Field, Schema};
use tracing::info;

use crate::utils::dataframes::{self, *};
use crate::utils::fs::*;
use crate::utils::logger::{log_output_file, log_write_output, Logger};
use crate::utils::regex::Matcher;

/// Command line arguments parsing.
pub fn cli() -> Command {
    Command::new("duplicate_files")
        .about("Detects duplicate files in a dataset, returning only unique files.")
        .long_about(
            "Detects duplicate files in a dataset, returning only unique files. The input and output are CSV files storing file paths.\n\
            The name of the column storing file paths in the input CSV file can be specified (default is 'name').\n\
             The similarity criterion can be either exact match or token-based (i.e., invariant to token order and whitespaces)."
        )
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
                .value_parser(["exact", "bow"]),
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
    threads: usize,
    input_header: &str,
    logger: &Logger,
) -> Result<()> {
    let default_output_path: String = format!("{}.unique.csv", input_path);
    let default_map_path: String = format!("{}.duplicates_map.csv", input_path);
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
    let split_dataset: Vec<DataFrame> = files
        .column(input_header)?
        .clone()
        .into_frame()
        .with_row_index("idx".into(), None)?
        .split_chunks_by_n(threads, true);

    info!("Starting file processing...\n");

    // Every thread comes with a sender channel.
    // The sender channel is used to send information about the downloaded repository back to the main thread.
    // The receiver channel is used by the main thread to collect and write the information to the log file.
    let (tx, rx) =
        crossbeam_channel::unbounded::<Option<Result<(u32, String, Option<Hash>), Error>>>();
    crossbeam::thread::scope(|s| {
        let mut ended_threads = 0;
        for chunk in split_dataset {
            let my_tx = tx.clone();
            s.spawn(move |_| {
                let word_matcher: Matcher = Matcher::words_matcher();
                for el in chunk
                    .column(input_header)
                    .and_then(|c| c.str())
                    .unwrap()
                    .into_iter()
                    .zip(dataframes::u32(&chunk, "idx").unwrap().into_iter())
                {
                    match el {
                        (Some(name), idx) => {
                            // Revert the temporary replacements of special characters.
                            let clean_name: String = name
                                .replace("-was_comma-", ",")
                                .replace("-was_quote-", "\"");
                            match load_file(&clean_name, 1024 * 1024 * 1024) {
                                Ok(Ok(file_content)) => {
                                    let hash = if similarity == "exact" {
                                        blake3::hash(&file_content)
                                    } else {
                                        blake3::hash(
                                            &word_matcher.bag_of_words(&file_content).serialize(),
                                        )
                                    };
                                    let _ =
                                        my_tx.send(Some(Ok((idx, name.to_string(), Some(hash)))));
                                }
                                Ok(Err(_)) => {
                                    let _ = my_tx.send(Some(Ok((idx, name.to_string(), None))));
                                }
                                Err(e) => {
                                    let _ = my_tx.send(Some(Err(e)));
                                }
                            }
                        }
                        _ => {
                            let _ = my_tx.send(Some(Err(anyhow!("Could not parse row"))));
                        }
                    }
                }
                my_tx.send(None)
            });
        }

        let progress = ProgressBar::new(file_count as u64);
        progress.set_style(
            indicatif::ProgressStyle::default_bar()
                .template("{elapsed} {wide_bar} {percent}%")
                .unwrap(),
        );

        let mut hash_map: HashMap<Hash, (u32, String, u32)> = std::collections::HashMap::new();
        let mut clone_map: HashMap<String, String> = HashMap::new();
        let mut big_files: usize = 0;

        // Writes received messages to the log file.
        // The order is therefore non-deterministic although the list of projects is.
        while let Ok(msg_opt) = rx.recv() {
            match msg_opt {
                Some(msg) => {
                    let (new_idx, new_name, opt_hash) = msg?;
                    match opt_hash {
                        None => {
                            big_files += 1;
                        }
                        Some(hash) => {
                            let (original_idx, original_name, count) = match hash_map.get(&hash) {
                                Some((idx, orig_name, cnt)) => (*idx, orig_name.clone(), *cnt),
                                None => (new_idx, new_name.to_string(), 0),
                            };
                            hash_map.insert(hash, (original_idx, original_name.clone(), count + 1));
                            clone_map.insert(new_name, original_name);
                            progress.inc(1);
                        }
                    }
                }
                None => {
                    // When a None message is received, the sender thread is considered finished.
                    // When all threads are finished, the main thread can exit.
                    ended_threads += 1;
                    if ended_threads == threads {
                        break;
                    }
                }
            }
        }
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

        let clusters_column: (Vec<String>, Vec<u32>) =
            hash_map.values().map(|v| (v.1.clone(), v.2)).unzip();

        let clusters = DataFrame::new(vec![
            polars::prelude::Column::new("name".into(), clusters_column.0),
            polars::prelude::Column::new("count".into(), clusters_column.1),
        ])?;

        let map_columns: (Vec<String>, Vec<String>) = clone_map
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .unzip();

        let mut map_df = DataFrame::new(vec![
            polars::prelude::Column::new("name".into(), map_columns.0),
            polars::prelude::Column::new("original".into(), map_columns.1),
        ])?;

        let most_duplicated_file: u32 = *u32(&clusters, "count")?
            .iter()
            .max()
            .with_context(|| "Empty column 'count'")?;
        let most_duplicated_file_percentage =
            (most_duplicated_file as f64 / small_files as f64) * 100.0;

        info!(
            "Most duplicated file: {} times / {:.2} %",
            most_duplicated_file, most_duplicated_file_percentage
        );

        log_write_output(logger, map_path, &mut map_df, false)?;

        let mut output_df = files
            .join(
                &clusters,
                ["name"],
                ["name"],
                polars::prelude::JoinType::Inner.into(),
                None,
            )
            .unwrap();

        log_write_output(logger, output_path, &mut output_df, false)
    })
    .map_err(|e| anyhow!("Error in child thread: {:?}", e))??;

    Ok(())
}

#[cfg(test)]
mod tests {

    use polars::prelude::SortMultipleOptions;

    use crate::utils::logger::test_logger;

    use super::*;

    const TEST_DATA: &str = "tests/data/phases/duplicate_files/";

    fn test_duplicate_files(input_path: &str, similarity: &str) -> Result<()> {
        let default_output_path = format!("{}.unique.csv", input_path);
        let default_map_path = format!("{}.duplicates_map.csv", input_path);
        delete_file(&default_output_path, true)?;
        delete_file(&default_map_path, true)?;
        run(
            &input_path,
            None,
            None,
            false,
            similarity,
            1,
            "name",
            test_logger(),
        )?;

        let expected_df = open_csv(&format!("{}.expected", default_output_path), None, None)?;

        let output_df = open_csv(&default_output_path, None, None)?;

        let sorted_expected_df = expected_df
            .sort(vec!["name"], SortMultipleOptions::new())
            .unwrap();
        let sorted_output_df = output_df
            .sort(vec!["name"], SortMultipleOptions::new())
            .unwrap();
        assert_eq!(sorted_expected_df, sorted_output_df);

        delete_file(&default_output_path, false)?;

        let expected_map = open_csv(&format!("{}.expected", default_map_path), None, None)?;

        let map_df = open_csv(&default_map_path, None, None)?;

        let sorted_expected_map = expected_map
            .sort(vec!["name"], SortMultipleOptions::new())
            .unwrap();
        let sorted_map_df = map_df
            .sort(vec!["name"], SortMultipleOptions::new())
            .unwrap();
        ensure!(
            sorted_expected_map.equals(&sorted_map_df),
            "Duplicate map CSV file does not match expected output."
        );

        delete_file(&default_map_path, false)
    }

    #[test]
    fn exact_files() -> Result<()> {
        test_duplicate_files(&format!("{}/duplicate_files.csv", TEST_DATA), "exact")?;
        test_duplicate_files(&format!("{}/duplicate_files_bow.csv", TEST_DATA), "bow")
    }
}
