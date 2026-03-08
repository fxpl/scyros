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

#![doc = include_str!("../docs/forks.md")]

use std::iter::FromIterator;

use anyhow::{Context, Result};
use clap::{Arg, ArgAction, Command};
use polars::frame::DataFrame;
use polars::prelude::{col, lit, DataType, Field, IntoLazy, Schema};
use tracing::info;

use crate::utils::fs::*;
use crate::utils::logger::{log_output_file, log_write_output, Logger};

/// Command line arguments parsing.
pub fn cli() -> Command {
    Command::new("forks")
        .about("Discards forks from a CSV file")
        .long_about(include_str!("../docs/forks.md"))
        .disable_version_flag(true)
        .arg(
            Arg::new("input")
                .short('i')
                .long("input")
                .value_name("INPUT_FILE.csv")
                .help("Path to the input csv file storing the projects.")
                .required(true),
        )
        .arg(
            Arg::new("output")
                .short('o')
                .long("output")
                .value_name("OUTPUT_FILE.csv")
                .help("Path to the output csv file to store non-forked projects.")
                .required(false),
        )
        .arg(
            Arg::new("column")
                .long("column")
                .value_name("FORK_COLUMN")
                .help("Name of the column storing whether projects are forks.")
                .default_value("fork"),
        )
        .arg(
            Arg::new("force")
                .short('f')
                .long("force")
                .help("Override the output file if it already exists.")
                .default_value("false")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("no-output")
                .long("no-output")
                .help("Does not write the output file. Prints statistics only.")
                .default_value("false")
                .required(false)
                .action(ArgAction::SetTrue)
                .conflicts_with_all(vec!["output", "force"]),
        )
}

/// Discards forks from a CSV file.
///
/// # Arguments
///
/// * `input_path` - The path to the input CSV file.
/// * `output_path` - The optional path to the output CSV file. Defaults to the input path with ".non-forks.csv" appended.
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
    forks: &str,
    force: bool,
    no_output: bool,
    logger: &Logger,
) -> Result<()> {
    let default_output_path = format!("{}.non-forks.csv", input_path);
    let output_path = output_path.unwrap_or(&default_output_path);

    // Checks if the input file exists
    check_path(input_path)?;

    // Checks if the output file already exists
    log_output_file(output_path, no_output, force)?;

    // Reads the CSV file into a DataFrame
    let mut projects: DataFrame = open_csv(
        input_path,
        Some(Schema::from_iter(vec![Field::new(
            forks.into(),
            DataType::UInt32,
        )])),
        None,
    )?;
    let projects_count = projects.height();

    info!("{} entries found in the file.", projects_count);

    // Filter out forked projects
    projects = projects
        .lazy()
        .filter(col(forks).eq(lit(0)))
        .collect()
        .with_context(|| "Could not remove forked projects")?;

    let non_forks_count = projects.height();
    let non_forks_percentage = (non_forks_count as f64 / projects_count as f64) * 100.0;

    // Log the number of forked and non-forked projects
    info!(
        "Forks: {} / {:.2} %",
        projects_count - non_forks_count,
        100.0 - non_forks_percentage
    );
    info!(
        "Non-forks: {} / {:.2} %",
        non_forks_count, non_forks_percentage
    );

    // Writes the result to the output CSV file
    log_write_output(logger, output_path, &mut projects, no_output)
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::utils::logger::test_logger;
    use anyhow::ensure;

    #[test]
    fn remove_forks() -> Result<()> {
        let input_path = "tests/data/phases/forks/forks.csv";
        let default_output_path = format!("{}.non-forks.csv", input_path);

        delete_file(&default_output_path, true)?;
        run(input_path, None, "fork", false, false, &test_logger())?;

        let expected_df = open_csv(&format!("{}.expected", default_output_path), None, None)?;

        let output_df = open_csv(&default_output_path, None, None)?;

        ensure!(
            expected_df.equals(&output_df),
            "Filtered DataFrame does not match expected result."
        );

        delete_file(&default_output_path, false)
    }
}
