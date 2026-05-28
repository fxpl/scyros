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

#![doc = include_str!("../docs/duplicate_ids.md")]
use anyhow::Result;
use clap::{Arg, ArgAction, Command};
use polars::frame::DataFrame;
use tracing::info;

use crate::utils::fs::*;
use crate::utils::logger::log_write_output;
use crate::utils::logger::{log_output_file, Logger};

/// Command line arguments parsing.
pub fn cli() -> Command {
    Command::new("duplicate_ids")
        .about("Discards duplicates in a CSV file according to one of the columns (by default repositories ids).")
        .long_about(include_str!("../docs/duplicate_ids.md"))
        .disable_version_flag(true)
        .arg(
            Arg::new("input")
                .short('i')
                .long("input")
                .value_name("INPUT_FILE.csv")
                .help("Path to the input csv file.")
                .required(true),
        )
        .arg(
            Arg::new("output")
                .short('o')
                .long("output")
                .value_name("OUTPUT_FILE.csv")
                .help("Path to the output csv file storing unique entries. \
                       If not specified, the input file name will be used with \".unique.csv\" appended.")
                .required(false),
        )
        .arg(
            Arg::new("column")
                .short('c')
                .long("column")
                .value_name("COLUMN_NAME")
                .help("Name of the column to check for duplicates.")
                .default_value("id"),
        )
        .arg(
            Arg::new("force")
                .short('f')
                .long("force")
                .help("Overrides the output file if it already exists.")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("no-output")
                .long("no-output")
                .help("Does not write the output file. Prints statistics only.")
                .required(false)
                .action(ArgAction::SetTrue)
                .conflicts_with_all(vec!["output", "force"]),
        )
}

/// Discards duplicate entries from a CSV file.
///
/// # Arguments
///
/// * `input_path` - The path to the input CSV file.
/// * `output_path` - The optional path to the output CSV file. Defaults to the input path with ".unique.csv" appended.
/// * `column` - The name of the column to check for duplicates.
/// * `force` - Whether to override the output file if it already exists.
/// * `no_output` - Whether to skip writing the output file.
/// * `logger` - The logger displaying the progress.
///
/// # Returns
///
/// A result indicating success or failure of the operation.
pub fn run(
    input_path: &str,
    output_path: Option<&str>,
    column: &str,
    force: bool,
    no_output: bool,
    logger: &Logger,
) -> Result<()> {
    let default_output_path = format!("{input_path}.unique.csv");
    let output_path = output_path.unwrap_or(&default_output_path);

    check_path(input_path)?;
    log_output_file(output_path, no_output, force)?;

    let mut ids: DataFrame = open_csv(input_path, None, None)?;
    let ids_count: usize = ids.height();

    info!("{} entries found in the file.", ids_count);

    // Keeping first occurrence of each id.
    // Unique stable is used to ensure reproducibility.
    ids = ids.unique_stable(
        Some(&[column.to_string()]),
        polars::frame::UniqueKeepStrategy::First,
        None,
    )?;
    let unique_ids_count: usize = ids.height();
    let unique_ids_percentage: f64 = (unique_ids_count as f64 / ids_count as f64) * 100.0;

    info!(
        "Unique ids: {} / {:.2} %",
        unique_ids_count, unique_ids_percentage
    );
    info!(
        "Duplicates: {} / {:.2} %",
        ids_count - unique_ids_count,
        100.0 - unique_ids_percentage
    );

    log_write_output(logger, output_path, &mut ids, no_output)
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::utils::logger::test_logger;

    const INPUT: &str = "tests/data/phases/duplicate_ids/duplicate_ids.csv";
    const EXPECTED: &str = "tests/data/phases/duplicate_ids/duplicate_ids.csv.unique.csv.expected";

    #[test]
    fn test_duplicate_ids() -> Result<()> {
        let output = "tests/data/phases/duplicate_ids/out_dedup.csv";
        delete_file(output, true)?;
        run(INPUT, Some(output), "id", false, false, test_logger())?;

        let expected_df = open_csv(EXPECTED, None, None)?;
        let output_df = open_csv(output, None, None)?;

        assert_eq!(expected_df, output_df);

        delete_file(output, false)
    }

    #[test]
    fn missing_input_duplicate_ids() {
        assert!(run("nonexistent.csv", None, "id", false, false, test_logger()).is_err());
    }

    #[test]
    fn output_exists_no_force_duplicate_ids() -> Result<()> {
        let output = "tests/data/phases/duplicate_ids/out_no_force.csv";
        write_file(output, b"")?;
        let result = run(INPUT, Some(output), "id", false, false, test_logger());
        delete_file(output, false)?;
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn force_overwrites_duplicate_ids() -> Result<()> {
        let output = "tests/data/phases/duplicate_ids/out_force.csv";
        write_file(output, b"")?;
        run(INPUT, Some(output), "id", true, false, test_logger())?;
        let expected_df = open_csv(EXPECTED, None, None)?;
        let output_df = open_csv(output, None, None)?;
        assert_eq!(expected_df, output_df);
        delete_file(output, false)
    }

    #[test]
    fn no_output_duplicate_ids() -> Result<()> {
        let output = "tests/data/phases/duplicate_ids/out_no_output.csv";
        run(INPUT, Some(output), "id", false, true, test_logger())?;
        assert!(!std::path::Path::new(output).exists());
        Ok(())
    }

    #[test]
    fn wrong_column_duplicate_ids() -> Result<()> {
        let output = "tests/data/phases/duplicate_ids/out_wrong_col.csv";
        let result = run(
            INPUT,
            Some(output),
            "nonexistent_col",
            false,
            false,
            test_logger(),
        );
        delete_file(output, true)?;
        assert!(result.is_err());
        Ok(())
    }
}
