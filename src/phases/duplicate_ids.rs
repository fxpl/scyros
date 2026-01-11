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

//! Discards duplicates in a CSV file.
//! Two entries are considered to be duplicates if they share the same value in
//! a column specified by the user.
//! The default column stores repositories ids and has \"id\" as a header.
//! Prints statistics about the number of duplicates found in the file and write the unique rows to a new CSV file.
//! By default, the output file name is the same as the input file name with \".unique.csv\" appended.
//!
use clap::{Arg, ArgAction, Command};
use polars::frame::DataFrame;

use crate::utils::error::*;
use crate::utils::fs::*;
use crate::utils::logger::log_write_output;
use crate::utils::logger::{log_output_file, Logger};

/// Command line arguments parsing.
pub fn cli() -> Command {
    Command::new("duplicate_ids")
        .about("Discards duplicates in a CSV file according to one of the columns (by default repositories ids).")
        .long_about(
            "Discards duplicates in a CSV file. Two entries are considered to be duplicates if they share the same value in\
             a column specified by the user.\nThe default column stores repositories ids and has \"id\" as a header.\
             Prints statistics about the number of duplicates found in the file and write the unique rows to a new CSV file.\n
             By default, the output file name is the same as the input file name with \".unique.csv\" appended."

        )
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
    logger: &mut Logger,
) -> Result<(), Error> {
    let default_output_path = format!("{}.unique.csv", input_path);
    let output_path = output_path.unwrap_or(&default_output_path);

    // Checks if the input file exists
    check_path(input_path)?;

    // Checks if the output file already exists
    log_output_file(logger, output_path, no_output, force)?;

    // Reads the CSV file into a DataFrame
    let mut ids: DataFrame = open_csv(input_path, None, None)?;
    let ids_count = ids.height();

    logger.log(&format!("{} entries found in the file.", ids_count))?;

    // Keeping first occurrence of each id.
    // Unique stable is used to ensure reproducibility.
    ids = map_err(
        ids.unique_stable(
            Some(&[column.to_string()]),
            polars::frame::UniqueKeepStrategy::First,
            None,
        ),
        "Could not check for duplicate entries.",
    )?;
    let unique_ids_count = ids.height();
    let unique_ids_percentage = (unique_ids_count as f64 / ids_count as f64) * 100.0;

    // Log the number of unique ids and duplicates
    logger.log(&format!(
        "Unique ids: {} / {:.2} %",
        unique_ids_count, unique_ids_percentage
    ))?;
    logger.log(&format!(
        "Duplicates: {} / {:.2} %",
        ids_count - unique_ids_count,
        100.0 - unique_ids_percentage
    ))?;

    // Writes the result to the output CSV file
    log_write_output(logger, output_path, &mut ids, no_output)
}

#[cfg(test)]
mod tests {

    use super::*;

    const TEST_DATA: &str = "tests/data/phases/duplicate_ids/";

    #[test]
    fn test_duplicate_ids() {
        let input_path = format!("{}/duplicate_ids.csv", TEST_DATA);
        let default_output_path = format!("{}.unique.csv", input_path);

        assert!(delete_file(&default_output_path, true).is_ok());
        assert!(run(&input_path, None, "id", false, false, &mut Logger::new()).is_ok());

        let expected_output_path = format!("{}.expected", default_output_path);
        let expected_df = open_csv(&expected_output_path, None, None);
        assert!(expected_df.is_ok());
        let expected_df = expected_df.unwrap();

        let output_df = open_csv(&default_output_path, None, None);
        assert!(output_df.is_ok());
        let output_df = output_df.unwrap();

        assert!(expected_df.equals(&output_df));

        assert!(delete_file(&default_output_path, false).is_ok());
    }
}
