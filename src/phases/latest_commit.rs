// Copyright 2026 Andrea Gilot
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

#![doc = include_str!("../docs/latest_commit.md")]

use std::collections::HashSet;
use std::io::Write as _;
use std::iter::FromIterator as _;
use std::path::Path;

use crate::utils::csv::*;
use crate::utils::dataframes::{has_column, require_columns};
use crate::utils::error::{error_cell, error_field, ErrorCode, ErrorSummary, RowError, ERROR};
use crate::utils::fs::*;
use crate::utils::github_api::Github;
use crate::utils::json::*;
use crate::utils::logger::*;
use crate::utils::parallel::parallel_pipeline;
use crate::utils::repo::{Repo, RepoColumn, Source, Unit};
use json::JsonValue;

use super::rename_arg;
use anyhow::{bail, ensure, Result};
use clap::ArgAction;
use clap::{Arg, Command};
use indicatif::ProgressBar;
use polars::frame::DataFrame;
use polars::prelude::*;
use rand::rngs::StdRng;
use rand::seq::SliceRandom as _;
use rand::SeedableRng;
use tracing::{debug, info};

/// Command line arguments parsing.
pub fn cli() -> Command {
    Command::new("latest_commit")
        .about("Pin GitHub projects to the latest commit of their default branch.")
        .long_about(include_str!("../docs/latest_commit.md"))
        .author("Andrea Gilot <andrea.gilot@it.uu.se>")
        .disable_version_flag(true)
        .arg(
            Arg::new("output")
                .short('o')
                .long("output")
                .value_name("OUTPUT_FILE.csv")
                .help("Path to the output csv file. \
                       By default, the name of the output file is the same as the input file with the suffix '.latest_commit.csv'.")
                .required(false)
        )
        .arg(
            Arg::new("input")
                .short('i')
                .long("input")
                .value_name("INPUT_FILE.csv")
                .help("Path to the input csv file to use. It must have a column 'repo_id' with the ids of the projects. \
                       Its repository columns are copied to the output, and its other columns are ignored.")
                .required(true)
        )
        .arg(
            Arg::new("tokens")
                .short('t')
                .long("tokens")
                .value_name("TOKENS_FILE.csv")
                .help("Path to the file containing the GitHub tokens to use. It must be a valid CSV file with one column named 'token' and where every line is a \
                       valid GitHub token (e.g ghp_Ab0C1D2eFg3hIjk4LM56oPqRsTuvWX7yZa8B).")
                .required(true)
        )
        .arg(
            Arg::new("seed")
                .short('s')
                .long("seed")
                .value_name("SEED")
                .help("Seed used to randomly shuffle the input data.")
                .default_value("2955615809866670875")
                .value_parser(clap::value_parser!(u64)),
        )
        .arg(
            Arg::new("force")
                .short('f')
                .long("force")
                .help("Override the output file if it already exists.")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("infer-source")
                .long("infer-source")
                .help("Infer the source of the projects without one: github if they have an id, local otherwise. \
                       Without this flag, a project without a source is an error.")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("sub")
                .long("sub")
                .value_name("NUMBER_OF_PROJECTS")
                .help("Number of projects to sample from the input file. \
                       If not specified, all remaining projects in the input file are used.")
                .value_parser(clap::value_parser!(usize))
        )
        .arg(
            Arg::new("threads")
                .short('n')
                .long("threads")
                .value_name("THREADS")
                .help("Number of projects queried from the GitHub API at the same time.")
                .default_value("1")
                .value_parser(clap::builder::RangedU64ValueParser::<usize>::new().range(1..)),
        )
        .arg(rename_arg())
}

/// Pins GitHub projects to the latest commit of their default branch.
///
/// The input must be a valid CSV file with the ids and the sources of the projects.
/// Its repository columns are copied to the output, and its other columns are ignored.
/// Projects are queried in a random order.
///
/// The output has the repository columns, with the latest commit of the projects in `latest_commit`, and the link to the projects.
///
/// # Arguments
///
/// * `input_path` - Path to the input file.
/// * `output_path` - Path to the output file. Defaults to the input path with `.latest_commit.csv` appended.
/// * `tokens` - Path to the file with the GitHub tokens.
/// * `seed` - Seed of the random order of the projects.
/// * `force` - Whether to overwrite the output file.
/// * `infer_source` - Whether to infer the source of the projects without one (see [`Repo::from_csv_row`]).
/// * `sub` - Number of projects to query. All the remaining projects if `None`.
/// * `threads` - Number of projects queried from the GitHub API at the same time.
/// * `renames` - Columns of the input read under other names.
/// * `logger` - Logger showing the progress.
pub fn run(
    input_path: &str,
    output_path: Option<&str>,
    tokens: &str,
    seed: u64,
    force: bool,
    infer_source: bool,
    sub: Option<usize>,
    threads: usize,
    renames: &[(String, String)],
    logger: &Logger,
) -> Result<()> {
    // Column index of the id in the output file.
    const ID_COL: usize = 0;

    // Check if the token file is valid and load the tokens.
    let tokens: Vec<String> = logger.log_tokens(tokens)?;

    // Load input file.
    let input_file: DataFrame = logger.run_task("Loading input file", || {
        let df: DataFrame = open_csv(
            input_path,
            Some(Schema::from_iter(
                RepoColumn::fields().into_iter().chain([error_field()]),
            )),
            None,
            renames,
        )?;
        require_columns(&df, input_path, &[RepoColumn::RepoId.name()])?;
        ensure!(
            infer_source || has_column(&df, RepoColumn::Source.name()),
            "{input_path} has no column source. Add it, read another column as source with \
             --rename OLD=source, or infer the sources with --infer-source."
        );
        Ok(df)
    })?;

    log_seed(seed);

    let mut shuffled_idx: Vec<usize> = (0..input_file.height()).collect();

    // Load the ids from the input file in random order.
    logger.run_task("Loading project IDs in random order", || {
        let mut rng: StdRng = SeedableRng::seed_from_u64(seed);
        shuffled_idx.shuffle(&mut rng);
        Ok(())
    })?;

    // Each project comes with the error of its row in the input, if any.
    let shuffled_rows = shuffled_idx.into_iter().map(|idx| -> Result<Project> {
        let repo: Repo = Repo::from_csv_row(&input_file, idx, infer_source)?;
        let error: Option<ErrorCode> = ErrorCode::from_csv_row(&input_file, idx)?;
        match repo.id {
            Some(id) => Ok((id, repo, error)),
            None => bail!("Row {idx} of the input file has no id"),
        }
    });

    let n_proj: usize = input_file.height();

    info!("  {} projects found.", n_proj);

    // Name of the output file.
    let default_output_path: String = format!("{}.latest_commit.csv", &input_path);
    let output_file_path: &str = output_path.unwrap_or(&default_output_path);

    // Load the previous results.
    let previous_results: HashSet<u32> = if force {
        HashSet::new()
    } else {
        logger.run_task("Resuming progress", || {
            Ok(if Path::new(output_file_path).exists() {
                CSVFile::new(output_file_path, FileMode::Read)?
                    .column(ID_COL)?
                    .into_iter()
                    .collect()
            } else {
                HashSet::new()
            })
        })?
    };

    if !previous_results.is_empty() {
        info!(
            "  the latest commit of {} projects has already been queried",
            previous_results.len()
        );
    }

    let mut output_file: CSVFile = CSVFile::new(
        output_file_path,
        if force {
            FileMode::Overwrite
        } else {
            FileMode::Append
        },
    )?;
    let header: Vec<&str> = Repo::header().into_iter().chain([ERROR]).collect();
    output_file.write_header(&header)?;
    let mut errors = ErrorSummary::<Repo>::new(&header)?;

    let gh = Github::new(tokens);

    // Projects that have not been processed yet, in random order, up to the number of projects to sample.
    let n: usize = sub.unwrap_or(usize::MAX);
    let mut projects: Vec<Project> = Vec::new();
    for row in shuffled_rows {
        if projects.len() == n {
            break;
        }
        let project: Project = row?;
        if !previous_results.contains(&project.0) {
            projects.push(project);
        }
    }

    info!("Starting to query the GitHub API...");

    // Create a progress bar
    let progress_bar: ProgressBar = ProgressBar::new(projects.len() as u64);

    progress_bar.set_style(
        indicatif::ProgressStyle::default_bar().template("{elapsed} {wide_bar} {percent}%")?,
    );

    // Rows are written in the order in which the requests complete.
    parallel_pipeline(
        &projects,
        vec![(); threads],
        |_, (id, repo, input_error): &Project| {
            // A project with an error in the input is copied with this error, without request.
            let commit: Result<String, RowError> = match input_error {
                Some(code) => Err((*code).into()),
                None => query(&gh, *id, repo).inspect_err(|e| debug!("Repository {id}: {e}")),
            };
            Ok(commit_row(repo.clone(), commit))
        },
        |row: Vec<String>| {
            errors.add(&row)?;
            output_file.write_record(row)?;
            progress_bar.inc(1);
            Ok(())
        },
    )?;
    output_file.flush()?;
    errors.log();
    Ok(())
}

/// A project to query: its id, the project, and the error of its row in the input, if any.
type Project = (u32, Repo, Option<ErrorCode>);

/// Queries the latest commit of the default branch of a project on GitHub.
///
/// # Arguments
///
/// * `gh` - The GitHub client.
/// * `id` - The id of the project.
/// * `repo` - The project.
fn query(gh: &Github, id: u32, repo: &Repo) -> Result<String, RowError> {
    if repo.source != Source::GitHub {
        return Err(RowError::new(
            ErrorCode::UnsupportedSource,
            format!("source {}", repo.source),
        ));
    }
    // Without a branch, GitHub lists the commits of the default branch, the latest first.
    let commits: JsonValue = gh.request(&format!(
        "https://api.github.com/repositories/{id}/commits?per_page=1"
    ))?;
    if commits.is_empty() {
        return Err(ErrorCode::NoCommit.into());
    }
    get_field::<String>(&commits[0], "sha").map_err(|e| RowError::new(ErrorCode::InvalidAnswer, e))
}

/// Returns the row of a project in the output.
///
/// # Arguments
///
/// * `repo` - The project.
/// * `commit` - The latest commit of the project, or why it could not be queried.
///   On an error, the commit of the input is kept.
fn commit_row(mut repo: Repo, commit: Result<String, RowError>) -> Vec<String> {
    let error: Option<ErrorCode> = match commit {
        Ok(commit) => {
            repo.commit = Some(commit);
            None
        }
        Err(e) => Some(e.code),
    };
    repo.row().into_iter().chain([error_cell(error)]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::ensure;

    const TEST_DATA: &str = "tests/data/phases/latest_commit";

    /// Returns a repository on GitHub, with a license and an older commit.
    fn repo() -> Repo {
        Repo {
            id: Some(42),
            source: Source::GitHub,
            name: Some("owner/repo".to_string()),
            license: Some("MIT License".to_string()),
            commit: Some("old".to_string()),
        }
    }

    #[test]
    fn rows_of_queried_projects() {
        assert_eq!(
            commit_row(repo(), Ok("abc123".to_string())),
            [
                "42",
                "github",
                "owner/repo",
                "MIT License",
                "abc123",
                "https://github.com/owner/repo/tree/abc123",
                ""
            ]
        );
    }

    #[test]
    fn rows_of_failed_queries() {
        // The repository, its commit in the input and its link are kept, and the error is written.
        assert_eq!(
            commit_row(repo(), Err(ErrorCode::NoCommit.into())),
            [
                "42",
                "github",
                "owner/repo",
                "MIT License",
                "old",
                "https://github.com/owner/repo/tree/old",
                "E011"
            ]
        );
    }

    #[test]
    #[ignore = "requires network access and valid GitHub tokens"]
    fn test_latest_commit() -> Result<()> {
        let input_file: String = format!("{TEST_DATA}/repos.csv");
        let output_file: String = format!("{input_file}.latest_commit.csv");
        ensure!(std::path::Path::new(&input_file).exists());

        delete_file(&output_file, true)?;

        run(
            &input_file,
            None,
            "ghtokens.csv",
            0,
            false,
            false,
            None,
            1,
            &[],
            test_logger(),
        )?;

        delete_file(&output_file, false)
    }

    #[test]
    fn missing_tokens_latest_commit() {
        let result = run(
            &format!("{TEST_DATA}/repos.csv"),
            None,
            "nonexistent_tokens.csv",
            0,
            false,
            false,
            None,
            1,
            &[],
            test_logger(),
        );
        assert!(result.is_err());
    }
}
