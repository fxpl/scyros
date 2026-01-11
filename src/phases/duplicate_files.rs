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

use blake3::Hash;
use clap::{Arg, ArgAction, Command};
use indicatif::ProgressBar;
use polars::frame::DataFrame;
use polars::prelude::{ChunkAgg, DataFrameJoinOps as _, DataType, Field, Schema};

use crate::utils::dataframes::*;
use crate::utils::error::*;
use crate::utils::fs::*;
use crate::utils::logger::Logger;
use crate::utils::regex::Matcher;

/// Command line arguments parsing.
pub fn cli() -> Command {
    Command::new("duplicate_files")
        .about("Detects duplicate files in a dataset, returning only unique files.")
        .long_about(
            "Detects duplicate files in a dataset, returning only unique files. The input and output are CSV files storing file metadata.\n\
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
    logger: &mut Logger,
) -> Result<(), Error> {
    let default_output_path = format!("{}.unique.csv", input_path);
    let default_map_path = format!("{}.duplicates_map.csv", input_path);
    let output_path = output_path.unwrap_or(&default_output_path);
    let map_path = map_path.unwrap_or(&default_map_path);

    check_path(input_path)?;

    match check_path(output_path) {
        Ok(_) => {
            if force {
                logger.log(&format!("Overriding existing file: {}", output_path))?;
            } else {
                Error::new(&format!(
                    "File {} already exists. Use --force to override it.",
                    output_path
                ))
                .to_res()?;
            }
        }
        Err(_) => logger.log(&format!("Creating new file: {}", output_path))?,
    }
    let files: DataFrame = open_csv(
        input_path,
        Some(Schema::from_iter(vec![
            Field::new("name".into(), DataType::String),
            Field::new("extension".into(), DataType::String),
            Field::new("loc".into(), DataType::UInt32),
            Field::new("words".into(), DataType::UInt32),
        ])),
        None,
    )?;

    if !has_column(&files, "name") {
        Error::new("The input file must contain an 'name' column.").to_res()?;
    }

    let file_count = files.height();

    logger.log(&format!("{} files found.", file_count))?;

    let split_dataset = map_err(
        map_err(files.column("name"), "Cannot access 'name' columns")?
            .clone()
            .into_frame()
            .with_row_index("idx".into(), None),
        "Could not add row indices to the dataframe",
    )?
    .split_chunks_by_n(threads, true);

    logger.log("Starting download...")?;
    logger.log("")?;

    // Every thread comes with a sender channel.
    // The sender channel is used to send information about the downloaded repository back to the main thread.
    // The receiver channel is used by the main thread to collect and write the information to the log file.
    let (tx, rx) =
        crossbeam_channel::unbounded::<Option<Result<(u32, String, Option<Hash>), Error>>>();

    map_err_debug(
        crossbeam::thread::scope(|s| {
            let mut ended_threads = 0;
            for chunk in split_dataset {
                let my_tx = tx.clone();
                s.spawn(move |_| {
                    let word_matcher: Matcher = Matcher::words_matcher();
                    for el in chunk
                        .column("name")
                        .and_then(|c| c.str())
                        .unwrap()
                        .into_iter()
                        .zip(chunk.column("idx").unwrap().u32().unwrap().into_iter())
                    {
                        match el {
                            (Some(name), Some(idx)) => {
                                let clean_name: String = name
                                    .replace("-was_comma-", ",")
                                    .replace("-was_quote-", "\"");
                                match load_file(&clean_name, 1024 * 1024 * 1024) {
                                    Ok(Ok(file_content)) => {
                                        let hash = if similarity == "exact" {
                                            blake3::hash(&file_content)
                                        } else {
                                            blake3::hash(
                                                &word_matcher
                                                    .bag_of_words(&file_content)
                                                    .serialize(),
                                            )
                                        };
                                        let _ = my_tx.send(Some(Ok((
                                            idx,
                                            name.to_string(),
                                            Some(hash),
                                        ))));
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
                                let _ =
                                    my_tx.send(Some(Error::new("Could not parse row").to_res()));
                            }
                        }
                    }
                    let _ = my_tx.send(None);
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
            while let Ok(msg) = rx.recv() {
                match msg {
                    Some(Ok((new_idx, new_name, opt_hash))) => match opt_hash {
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
                    },
                    Some(Err(e)) => {
                        panic!("Error in child thread: {}", e);
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

            let _ = logger.log(&format!(
                "Ignored large files: {} / {:.2} %",
                big_files, big_files_percentage
            ));
            let _ = logger.log(&format!(
                "Remaining files: {} / {:.2} %",
                small_files,
                100.0 - big_files_percentage
            ));

            let unique_files = hash_map.len();
            let unique_file_percentage = (unique_files as f64 / small_files as f64) * 100.0;

            let _ = logger.log(&format!(
                "Unique files: {} / {:.2} %",
                unique_files, unique_file_percentage
            ));
            let _ = logger.log(&format!(
                "Duplicate files: {} / {:.2} %",
                small_files - unique_files,
                100.0 - unique_file_percentage
            ));

            let clusters_column: (Vec<String>, Vec<u32>) =
                hash_map.values().map(|v| (v.1.clone(), v.2)).unzip();

            let clusters = DataFrame::new(vec![
                polars::prelude::Column::new("name".into(), clusters_column.0),
                polars::prelude::Column::new("count".into(), clusters_column.1),
            ])
            .unwrap();

            let map_columns: (Vec<String>, Vec<String>) = clone_map
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .unzip();

            let mut map_df = DataFrame::new(vec![
                polars::prelude::Column::new("name".into(), map_columns.0),
                polars::prelude::Column::new("original".into(), map_columns.1),
            ])
            .unwrap();

            let most_duplicated_file = clusters
                .column("count")
                .unwrap()
                .u32()
                .unwrap()
                .max()
                .unwrap();
            let most_duplicated_file_percentage =
                (most_duplicated_file as f64 / small_files as f64) * 100.0;

            let _ = logger.log(&format!(
                "Most duplicated file: {} times / {:.2} %",
                most_duplicated_file, most_duplicated_file_percentage
            ));

            let _ = logger.log_completion(&format!("Writing to {}", map_path), || {
                write_csv(map_path, &mut map_df)
            });

            let mut output_df = files
                .join(
                    &clusters,
                    ["name"],
                    ["name"],
                    polars::prelude::JoinType::Inner.into(),
                    None,
                )
                .unwrap();

            let _ = logger.log_completion(&format!("Writing to {}", output_path), || {
                write_csv(output_path, &mut output_df)
            });
        }),
        "Error in thread",
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {

    use polars::prelude::SortMultipleOptions;

    use super::*;

    const TEST_DATA: &str = "tests/data/phases/duplicate_files/";

    fn test_duplicate_files(input_path: &str, similarity: &str) {
        let default_output_path = format!("{}.unique.csv", input_path);
        let default_map_path = format!("{}.duplicates_map.csv", input_path);

        assert!(delete_file(&default_output_path, true).is_ok());
        assert!(delete_file(&default_map_path, true).is_ok());
        assert!(run(
            &input_path,
            None,
            None,
            false,
            similarity,
            1,
            &mut Logger::new()
        )
        .is_ok());

        let expected_output_path = format!("{}.expected", default_output_path);
        let expected_df = open_csv(&expected_output_path, None, None);
        assert!(expected_df.is_ok());
        let expected_df = expected_df.unwrap();

        let output_df = open_csv(&default_output_path, None, None);
        assert!(output_df.is_ok());
        let output_df = output_df.unwrap();

        let sorted_expected_df = expected_df
            .sort(vec!["name"], SortMultipleOptions::new())
            .unwrap();
        let sorted_output_df = output_df
            .sort(vec!["name"], SortMultipleOptions::new())
            .unwrap();
        assert!(sorted_expected_df.equals(&sorted_output_df));

        assert!(delete_file(&default_output_path, false).is_ok());

        let expected_map_path = format!("{}.expected", default_map_path);
        let expected_map = open_csv(&expected_map_path, None, None);
        assert!(expected_map.is_ok());
        let expected_map = expected_map.unwrap();

        let map_df = open_csv(&default_map_path, None, None);
        assert!(map_df.is_ok());
        let map_df = map_df.unwrap();

        let sorted_expected_map = expected_map
            .sort(vec!["name"], SortMultipleOptions::new())
            .unwrap();
        let sorted_map_df = map_df
            .sort(vec!["name"], SortMultipleOptions::new())
            .unwrap();
        assert!(sorted_expected_map.equals(&sorted_map_df));

        assert!(delete_file(&default_map_path, false).is_ok());
    }

    #[test]
    fn exact_files() {
        test_duplicate_files(&format!("{}/duplicate_files.csv", TEST_DATA), "exact");
        test_duplicate_files(&format!("{}/duplicate_files_bow.csv", TEST_DATA), "bow");
    }
}
