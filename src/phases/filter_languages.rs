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

#![doc = include_str!("../docs/filter_languages.md")]

use std::collections::{HashMap, HashSet};
use std::iter::FromIterator;
use std::vec;

use anyhow::{Context, Result};
use clap::{Arg, ArgAction, Command};
use polars::frame::DataFrame;
use polars::prelude::{col, lit, DataType, Field, IdxCa, IntoLazy, Schema};
use tracing::info;

use crate::utils::logger::{log_output_file, log_write_output, Logger};
use crate::utils::regex::KeywordFiles;
use crate::utils::{dataframes, fs::*};

/// Command line arguments parsing.
pub fn cli() -> Command {
    Command::new("filter_languages")
        .about("Filter out projects that do not contain any code written in a programming language from a list provided by the user.")
        .long_about(include_str!("../docs/filter_languages.md")
        )
        .disable_version_flag(true)
        .arg(
            Arg::new("input")
                .short('i')
                .long("input")
                .value_name("INPUT_FILE.csv")
                .help("Path to the input csv file storing the projects languages.")
                .required(true),
        )
        .arg(
            Arg::new("output")
                .short('o')
                .long("output")
                .value_name("OUTPUT_FILE.csv")
                .help("Path to the output csv file storing the projects containing at least one of the languages provided by the user.")
                .required(false),
        )
        .arg(
            Arg::new("languages")
                .short('l')
                .long("languages")
                .alias("lang")
                .value_name("LANGUAGES.json")
                .help("Path to the json file storing the languages to keep with the following format: \n{\n    \"languages\": [\"lang1\", \"lang2\", ...]\n}\n")
                .required(true)
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
                .help("Does not write the output file.")
                .default_value("false")
                .required(false)
                .action(ArgAction::SetTrue)
                .conflicts_with_all(vec!["output", "force"]),
        )
}

/// Entrypoint of the program
///
/// # Arguments
///
/// * `input_path` - The path to the input CSV file.
/// * `output_path` - The optional path to the output CSV file. Defaults to the input path with ".filtered_lang.csv" appended.
/// * `languages_path` - The path to the JSON file storing the languages to keep.
/// * `force` - Whether to override the output file if it already exists.
/// * `no_output` - Whether to write the output file.
/// * `logger` - The logger displaying the progress.
///
/// # Returns
///
/// A result indicating success or failure of the operation.
pub fn run(
    input_path: &str,
    output_path: Option<&str>,
    languages_path: &str,
    force: bool,
    no_output: bool,
    logger: &Logger,
) -> Result<()> {
    let default_output_path = format!("{input_path}.filtered_lang.csv");
    let output_path = output_path.unwrap_or(&default_output_path);

    check_path(input_path)?;

    // Check if the output file already exists
    log_output_file(output_path, no_output, force)?;

    let languages: HashSet<String> = KeywordFiles::new(false)
        .add_file(languages_path, false)?
        .matchers
        .keys()
        .cloned()
        .collect();

    let mut projects: DataFrame = open_csv(
        input_path,
        Some(Schema::from_iter(vec![
            Field::new("id".into(), DataType::UInt32),
            Field::new("name".into(), DataType::String),
            Field::new("languages".into(), DataType::String),
            Field::new("latest_commit".into(), DataType::String),
        ])),
        None,
    )?;
    let projects_count = projects.height();

    info!("{} projects found in the file", projects_count);

    projects = projects
        .lazy()
        .filter(col("name").str().starts_with(lit("http/2 ")).not())
        .collect()
        .with_context(|| "Could not filter unreachable projects")?;

    // Discarding projects that are unreachable (i.e., turned private or deleted)

    let reachable_projects_count = projects.height();
    let reachable_projects_percentage =
        (reachable_projects_count as f64 / projects_count as f64) * 100.0;

    info!(
        "\n{} projects ({:.2}%) are unreachable (turned private or deleted)",
        projects_count - reachable_projects_count,
        100.0 - reachable_projects_percentage
    );
    info!(
        "{} remaining projects ({:.2}%)",
        reachable_projects_count, reachable_projects_percentage
    );

    let languages_maps: Vec<(usize, HashMap<&str, &str>)> =
        dataframes::str(&projects, "languages")?
            .into_iter()
            .map(parse_map)
            .enumerate()
            .collect();

    let languages_mask = languages_maps
        .into_iter()
        .filter_map(|(idx, m)| {
            Some(Some(idx as u32))
                .filter(|_| m.keys().any(|k| languages.contains(&k.to_lowercase())))
        })
        .collect::<IdxCa>();

    projects = projects
        .take(&languages_mask)
        .with_context(|| "Could not filter projects according to languages")?;

    let retained_projects_count = projects.height();
    let retained_projects_percentage =
        (retained_projects_count as f64 / reachable_projects_count as f64) * 100.0;

    info!(
        "\n{} projects ({:.2}%) do not contain any code written in a programming language in {}",
        reachable_projects_count - retained_projects_count,
        100.0 - retained_projects_percentage,
        languages_path
    );
    info!(
        "{} remaining projects ({:.2}%)\n",
        retained_projects_count, retained_projects_percentage
    );

    // Writes the result to the output CSV file
    log_write_output(logger, output_path, &mut projects, no_output)
}

fn parse_map(map: &str) -> HashMap<&str, &str> {
    map.split(';')
        .filter_map(|pair| {
            let mut parts = pair.splitn(2, ':');
            match (parts.next(), parts.next()) {
                (Some(k), Some(v)) => Some((k, v)),
                _ => None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {

    use crate::utils::logger::test_logger;
    use anyhow::ensure;

    use super::*;

    #[test]
    fn test_parse_map() -> Result<()> {
        let input = "key1:value1;key2:value2;key3:value3";
        let expected: HashMap<&str, &str> =
            [("key1", "value1"), ("key2", "value2"), ("key3", "value3")]
                .iter()
                .cloned()
                .collect();
        let result = parse_map(input);
        ensure!(
            result == expected,
            "Parsed map does not match expected result."
        );
        Ok(())
    }
    #[test]
    fn test_parse_empty_map() -> Result<()> {
        let input = "";
        let expected: HashMap<&str, &str> = HashMap::new();
        let result = parse_map(input);
        ensure!(
            result == expected,
            "Parsed map does not match expected result."
        );
        Ok(())
    }

    const TEST_DATA: &str = "tests/data/phases/filter_languages";

    #[test]
    fn test_filter_languages() -> Result<()> {
        let input_path = format!("{TEST_DATA}/filter_languages.csv");
        let default_output_path = format!("{input_path}.filtered_lang.csv");
        let language_path = "tests/data/keywords/scala_float.json";

        delete_file(&default_output_path, true)?;
        run(
            &input_path,
            None,
            language_path,
            false,
            false,
            test_logger(),
        )?;

        let expected_df = open_csv(&format!("{default_output_path}.expected"), None, None)?;
        let output_df = open_csv(&default_output_path, None, None)?;

        ensure!(
            expected_df.equals(&output_df),
            "Filtered DataFrame does not match expected result."
        );

        delete_file(&default_output_path, false)
    }
}
