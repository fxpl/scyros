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

use std::iter::FromIterator;
use std::vec;

use clap::{value_parser, Arg, ArgAction, Command};
use polars::frame::DataFrame;
use polars::prelude::{col, lit, DataType, Field, IntoLazy, Schema};

use crate::utils::error::*;
use crate::utils::fs::*;
use crate::utils::logger::{log_output_file, log_write_output, Logger};

/// Command line arguments parsing.
pub fn cli() -> Command {
    Command::new("filter_metadata")
        .about("Filter out projects that are below provided thresholds for some characteristics.")
        .long_about(
            " Filter out projects that are below provided thresholds for some characteristics."
        )
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
                .help("Path to the output csv file storing the remaining projects.")
                .required(false),
        )
        .arg(
            Arg::new("loc")
                .short('l')
                .long("loc")
                .value_name("LOC")
                .help("The threshold for lines of code under which a project is discarded. Default to 0")
                .value_parser(value_parser!(u64))
                .required(false)
                .default_value("0"),
        )
        .arg(
            Arg::new("disabled")
                .short('d')
                .long("disabled")
                .help("Discard disabled projects.")
                .default_value("false")
                .action(ArgAction::SetTrue),
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
            Arg::new("age")
                .short('a')
                .long("age")
                .value_name("AGE")
                .help("The threshold for the age (in days) of the project under which it is discarded. Default to 0")
                .value_parser(value_parser!(u32))
                .required(false)
                .default_value("0"),
        )
        .arg(
            Arg::new("non-code")
                .long("non-code")
                .help("Discard projects that do not contain code (e.g., documentation only).")
                .default_value("false")
                .required(false)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("no-output")
                .long("no-output")
                .help("Do not write the output file.")
                .default_value("false")
                .required(false)
                .action(ArgAction::SetTrue)
                .conflicts_with_all(vec!["output", "force"]),
        )
}

/// Filters out projects that are below provided thresholds for some characteristics.
///
/// # Arguments
///
/// * `input_path` - The path to the input CSV file.
/// * `output_path` - The optional path to the output CSV file. Defaults to the input path with ".unique.csv" appended.
/// * `loc` - The threshold for lines of code under which a project is discarded.
/// * `age` - The threshold for the age (in days) of the project under which it is discarded. If `None`, no filtering is applied.
/// * `disabled` - Whether to discard disabled projects.
/// * `force` - Whether to override the output file if it already exists.
/// * `non_code` - Whether to discard projects that do not contain code (e.g., documentation only).
/// * `no_output` - Whether to write the output file.
/// * `logger` - The logger displaying the progress.
///
/// # Returns
///
/// A result indicating success or failure of the operation.
pub fn run(
    input_path: &str,
    output_path: Option<&str>,
    loc: u64,
    age: u32,
    disabled: bool,
    non_code: bool,
    force: bool,
    no_output: bool,
    logger: &mut Logger,
) -> Result<(), Error> {
    let default_output_path = format!("{}.filtered.csv", input_path);
    let output_path = output_path.unwrap_or(&default_output_path);

    check_path(input_path)?;

    // Checks if the output file already exists
    log_output_file(logger, output_path, no_output, force)?;

    let mut projects: DataFrame = open_csv(
        input_path,
        Some(Schema::from_iter(vec![
            Field::new("id".into(), DataType::UInt32),
            Field::new("name".into(), DataType::String),
            Field::new("language".into(), DataType::String),
            Field::new("created".into(), DataType::UInt64),
            Field::new("pushed".into(), DataType::UInt64),
            // Field::new("updated".into(), DataType::UInt64),
            // Field::new("fork".into(), DataType::UInt32),
            Field::new("disabled".into(), DataType::UInt32),
            // Field::new("archived".into(), DataType::UInt32),
            // Field::new("stars".into(), DataType::UInt32),
            // Field::new("forks".into(), DataType::UInt32),
            // Field::new("issues".into(), DataType::UInt32),
            // Field::new("has_issues".into(), DataType::UInt32),
            // Field::new("watchers_count".into(), DataType::UInt32),
            // Field::new("subscribers".into(), DataType::UInt32),
            Field::new("size".into(), DataType::UInt64),
            // Field::new("license".into(), DataType::String),
        ])),
        Some(vec![
            "id", "name", "language", "created", "pushed", // "updated",
            // "fork",
            "disabled",
            // "archived",
            // "stars",
            // "forks",
            // "issues",
            // "has_issues",
            // "watchers_count",
            // "subscribers",
            "size",
            // "license",
        ]),
    )?;
    let projects_count = projects.height();

    logger.log(&format!("{} ids found in the file", projects_count))?;

    projects = map_err(
        projects
            .lazy()
            .filter(col("name").str().starts_with(lit("http/2 ")).not())
            .with_column((col("pushed") - col("created")).alias("age"))
            .drop(vec!["created", "pushed"])
            .with_column(
                (col("age") / lit(60 * 60 * 24))
                    .cast(DataType::UInt32)
                    .alias("age"),
            )
            .collect(),
        "Could not compute the age of the projects",
    )?;

    // Discarding projects that are unreachable (i.e., turned private or deleted)

    let mut reachable_projects_count = projects.height();
    let reachable_projects_percentage =
        (reachable_projects_count as f64 / projects_count as f64) * 100.0;

    logger.log(&format!(
        "{} projects ({:.2}%) are unreachable (turned private or deleted)",
        projects_count - reachable_projects_count,
        100.0 - reachable_projects_percentage
    ))?;
    logger.log(&format!(
        "{} remaining projects ({:.2}%)",
        reachable_projects_count, reachable_projects_percentage
    ))?;

    // Discarding projects that are not code (e.g., documentation only)

    if non_code {
        projects = map_err(
            projects
                .lazy()
                .filter(col("language").str().len_bytes().neq(lit(0)))
                .collect(),
            "Could not filter projects by size",
        )?;

        let code_count = projects.height();
        let code_percentage = (code_count as f64 / reachable_projects_count as f64) * 100.0;

        logger.log(&format!(
            "\n{} projects ({:.2}%) contain code",
            code_count, code_percentage
        ))?;
        logger.log(&format!(
            "{} projects ({:.2}%) do not contain code",
            reachable_projects_count - code_count,
            100.0 - code_percentage
        ))?;

        reachable_projects_count = code_count;
    }

    let loc_mask = col("size").gt_eq(lit(loc));

    let loc_filter_count = map_err(
        projects
            .clone()
            .lazy()
            .filter(loc_mask.clone())
            .count()
            .collect(),
        "Could not filter projects by size",
    )?;

    // Safe unwrap
    let loc_filter_count: usize = loc_filter_count.get(0).unwrap()[0]
        .extract::<u32>()
        .unwrap() as usize;
    let loc_filter_percentage = (loc_filter_count as f64 / reachable_projects_count as f64) * 100.0;

    logger.log(&format!(
        "\nProjects with ≥ {} lines of code: {} / {:.2} %",
        loc, loc_filter_count, loc_filter_percentage
    ))?;
    logger.log(&format!(
        "Projects with < {} lines of code: {} / {:.2} %",
        loc,
        reachable_projects_count - loc_filter_count,
        100.0 - loc_filter_percentage
    ))?;

    let age_mask = col("age").gt_eq(lit(age));

    let age_filter_count = map_err(
        projects
            .clone()
            .lazy()
            .filter(age_mask.clone())
            .count()
            .collect(),
        "Could not filter projects by age",
    )?;

    // Safe unwrap
    let age_filter_count: usize = age_filter_count.get(0).unwrap()[0]
        .extract::<u32>()
        .unwrap() as usize;
    let age_filter_percentage = (age_filter_count as f64 / reachable_projects_count as f64) * 100.0;

    logger.log(&format!(
        "\nProjects ≥ {} days old: {} / {:.2} %",
        age, age_filter_count, age_filter_percentage
    ))?;

    logger.log(&format!(
        "Projects < {} days old: {} / {:.2} %",
        age,
        reachable_projects_count - age_filter_count,
        100.0 - age_filter_percentage
    ))?;

    let disabled_mask = if disabled {
        col("disabled").eq(lit(0))
    } else {
        lit(true)
    };

    if disabled {
        let disabled_filter_count = map_err(
            projects
                .clone()
                .lazy()
                .filter(disabled_mask.clone())
                .count()
                .collect(),
            "Could not filter projects by disabled",
        )?;

        // Safe unwrap
        let disabled_filter_count: usize = disabled_filter_count.get(0).unwrap()[0]
            .extract::<u32>()
            .unwrap() as usize;
        let disabled_filter_percentage =
            (disabled_filter_count as f64 / reachable_projects_count as f64) * 100.0;

        logger.log(&format!(
            "\nNon-disabled projects: {} / {:.2} %",
            disabled_filter_count, disabled_filter_percentage
        ))?;

        logger.log(&format!(
            "Disabled projects:     {} / {:.2} %",
            reachable_projects_count - disabled_filter_count,
            100.0 - disabled_filter_percentage
        ))?;
    }

    projects = map_err(
        projects
            .lazy()
            .filter(loc_mask.and(age_mask).and(disabled_mask))
            .collect(),
        "Could not filter projects",
    )?;

    let retained_projects_count = projects.height();
    let retained_projects_percentage =
        (retained_projects_count as f64 / reachable_projects_count as f64) * 100.0;

    logger.log(&format!(
        "\nRetained projects among {}: {} / {:.2} %",
        if non_code {
            "those containing code"
        } else {
            "reachable ones"
        },
        retained_projects_count,
        retained_projects_percentage
    ))?;

    logger.log(&format!(
        "Projects that have not been retained:     {} / {:.2} %\n",
        reachable_projects_count - retained_projects_count,
        100.0 - retained_projects_percentage
    ))?;

    // Writes the result to the output CSV file
    log_write_output(logger, output_path, &mut projects, no_output)
}

#[cfg(test)]
mod tests {

    use super::*;

    const TEST_DATA: &str = "tests/data/phases/filter_metadata";

    #[test]
    fn test_remove_forks() {
        let input_path = format!("{}/filter_metadata.csv", TEST_DATA);
        let default_output_path = format!("{}.filtered.csv", input_path);

        assert!(delete_file(&default_output_path, true).is_ok());
        assert!(run(
            &input_path,
            None,
            500,
            3,
            true,
            true,
            true,
            false,
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

        assert!(expected_df.equals(&output_df));

        assert!(delete_file(&default_output_path, false).is_ok());
    }
}
