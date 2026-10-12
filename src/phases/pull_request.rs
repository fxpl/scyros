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

#![doc = include_str!("../docs/pull_request.md")]

use std::collections::HashSet;
use std::fmt::Write as _;
use std::io::Write;
use std::iter::FromIterator as _;
use std::path::Path;

use crate::utils::csv::*;
use crate::utils::dataframes::{self, has_column, require_columns};
use crate::utils::error::{error_cell, error_field, ErrorCode, ErrorSummary, RowError, ERROR};
use crate::utils::fs::*;
use crate::utils::github::*;
use crate::utils::github_api::*;
use crate::utils::json::*;
use crate::utils::logger::{log_seed, Logger};
use crate::utils::repo::{PullRequest, Repo, RepoColumn, Source, Unit};
use anyhow::{bail, ensure, Error, Result};
use clap::ArgAction;
use clap::{Arg, Command};
use indicatif::ProgressBar;
use json::JsonValue;
use polars::frame::DataFrame;
use polars::prelude::*;
use rand::rngs::StdRng;
use rand::seq::SliceRandom as _;
use rand::SeedableRng;
use tracing::{debug, info};

use super::rename_arg;

/// Command line arguments parsing.
pub fn cli() -> Command {
    Command::new("pr")
        .about("Collect pull requests of GitHub projects")
        .long_about(include_str!("../docs/pull_request.md"))
        .author("Andrea Gilot <andrea.gilot@it.uu.se>")
        .disable_version_flag(true)
        .arg(
            Arg::new("output")
                .short('o')
                .long("output")
                .value_name("OUTPUT_FILE.csv")
                .help("Path to the output csv file to store the metadata. \
                       By default, the name of the output file is the same as the input file with the suffix '.pulls.csv'.")
                .required(false)
        )
        .arg(
            Arg::new("input")
                .short('i')
                .long("input")
                .value_name("INPUT_FILE.csv")
                .help("Path to the input csv file to use. It must have a column 'repo_id' with the ids of the projects and a column 'repo_name' with their full names. \
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
            Arg::new("dest")
                .short('d')
                .long("dest")
                .aliases(["target", "destination"])
                .value_name("DESTINATION")
                .help("Path to the directory where to store the pull request comments.")
                .required(true)
        )
        .arg(
            Arg::new("seed")
                .short('s')
                .long("seed")
                .value_name("SEED")
                .help("Seed used to randomly shuffle the input data.")
                .default_value("9990520807055774474")
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
        .arg(rename_arg())
}

/// Entry point of the program.
///
/// # Arguments
///
/// * `input_path` - The path to the input file. It has the columns `repo_id`, `repo_name` and `source`.
/// * `output_path` - The path to the output file. If None, the output file will be named as the input file + ".pulls.csv".
/// * `tokens` - The path to the file containing the GitHub tokens.
/// * `seed` - The seed to use for the random number generator.
/// * `force` - Whether to override the output file if it already exists.
/// * `infer_source` - Whether to infer the source of the projects without one (see [`Repo::from_csv_row`]).
/// * `target` - The target directory where to store the pull request files.
/// * `sub` - The number of projects to sample from the input file. If not specified, all remaining projects in the input file are used.
/// * `renames` - Columns of the input read under other names.
/// * `logger` - Logger for logging progress.
///
/// # Returns
///
/// * Unit if the program finished successfully or an error message if an error occurred.
///
pub fn run(
    input_path: &str,
    output_path: Option<&String>,
    tokens: &str,
    seed: u64,
    force: bool,
    infer_source: bool,
    target: &str,
    sub: Option<usize>,
    renames: &[(String, String)],
    logger: &Logger,
) -> Result<()> {
    // Column index of the id in the output file.
    const ID_COL: usize = 0;

    // Check if the token file is valid and load the tokens.
    let tokens: Vec<String> = logger.log_tokens(tokens)?;

    // Load input file
    let input_file: DataFrame = logger.run_task("Loading input file", || {
        let df: DataFrame = open_csv(
            input_path,
            Some(Schema::from_iter(
                RepoColumn::fields().into_iter().chain([error_field()]),
            )),
            None,
            renames,
        )?;
        require_columns(
            &df,
            input_path,
            &[RepoColumn::RepoId.name(), RepoColumn::RepoName.name()],
        )?;
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
        match (repo.id, &repo.name) {
            (Some(id), Some(_)) => Ok((id, repo, error)),
            _ => bail!("Row {idx} of the input file has no id or no full name"),
        }
    });

    let n_pr: usize = input_file.height();

    info!("  {} projects found.", n_pr);

    // Name of the output file.
    let default_output_path: String = format!("{}.pulls.csv", &input_path);
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
            "  the metadata of {} projects have already been queried",
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

    let header: Vec<&str> = header();
    output_file.write_header(&header)?;
    let mut errors = ErrorSummary::<PullRequest>::new(&header)?;

    let gh = Github::new(tokens);

    info!("Starting to query the GitHub API...");

    // Number of projects to sample.
    let mut n: usize = match sub {
        Some(m) => m,
        None => dataframes::u32(&input_file, RepoColumn::RepoId.name())?
            .into_iter()
            .filter(|id| !previous_results.contains(id))
            .count(),
    };

    // Create a progress bar
    let progress_bar: ProgressBar = ProgressBar::new(n_pr as u64);
    progress_bar.set_style(
        indicatif::ProgressStyle::default_bar()
            .template("{elapsed} {wide_bar} {percent}%")
            .unwrap(),
    );

    if sub.is_some() {
        progress_bar.set_length(n as u64);
    }

    for row in shuffled_rows {
        if n == 0 {
            break;
        }
        let (id, repo, input_error): Project = row?;
        if previous_results.contains(&id) {
            continue;
        }
        // The rows of a project are written once all its pull requests are fetched.
        let rows: Vec<Vec<String>> = match input_error {
            // A project with an error in the input is copied with this error, without request.
            Some(code) => vec![error_row(&repo, code)],
            None => match pull_requests(&gh, id, &repo, target) {
                Err(e) => {
                    debug!("Repository {id}: {e}");
                    vec![error_row(&repo, e.code)]
                }
                Ok(prs) => prs
                    .into_iter()
                    .map(|pr| -> Result<Vec<String>> {
                        Ok(match pr {
                            Err(e) => {
                                debug!("A pull request of repository {id}: {e}");
                                error_row(&repo, e.code)
                            }
                            Ok(mut pr) => {
                                let error: Option<RowError> = scrape_pr_comments(&gh, id, &pr)?;
                                if let Some(e) = &error {
                                    debug!("Pull request {} of repository {id}: {e}", pr.pr_number);
                                    pr.local_discussion_path = String::new();
                                }
                                pr_row(&repo, &pr, error.map(|e| e.code))
                            }
                        })
                    })
                    .collect::<Result<Vec<Vec<String>>>>()?,
            },
        };
        for row in rows {
            errors.add(&row)?;
            output_file.write_record(row)?;
        }
        progress_bar.inc(1);
        n -= 1;
    }
    output_file.flush()?;
    errors.log();
    Ok(())
}

/// A project to query: its id, the project, and the error of its row in the input, if any.
type Project = (u32, Repo, Option<ErrorCode>);

/// Queries the pull requests of a project on GitHub.
///
/// # Arguments
///
/// * `gh` - The GitHub client.
/// * `id` - The id of the project.
/// * `repo` - The project.
/// * `target` - The directory where the discussions of the pull requests are stored.
///
/// # Returns
///
/// The pull requests, each one possibly with the error of its answer, or why they could not be queried.
fn pull_requests(
    gh: &Github,
    id: u32,
    repo: &Repo,
    target: &str,
) -> Result<Vec<Result<PRMetadata, RowError>>, RowError> {
    if repo.source != Source::GitHub {
        return Err(RowError::new(
            ErrorCode::UnsupportedSource,
            format!("source {}", repo.source),
        ));
    }
    // Pull requests are fetched page by page (100 pull requests per page).
    scrape_pages(
        gh,
        &|per_page, page| {
            format!("https://api.github.com/repositories/{id}/pulls?state=all&per_page={per_page}&page={page}")
        },
        &|json| PRMetadata::parse_json(&json, (id, target.to_string())),
    )
}

/// Returns the header of the output: the columns of the pull requests, the error, then their metadata.
fn header() -> Vec<&'static str> {
    PullRequest::header()
        .into_iter()
        .chain([ERROR])
        .chain(PRMetadata::COLUMNS)
        .collect()
}

/// Returns the row of a pull request in the output.
///
/// # Arguments
///
/// * `repo` - The project of the pull request.
/// * `pr` - The pull request.
/// * `error` - Why the discussion of the pull request could not be fetched, if it could not.
fn pr_row(repo: &Repo, pr: &PRMetadata, error: Option<ErrorCode>) -> Vec<String> {
    PullRequest {
        repo,
        number: pr.pr_number,
    }
    .row()
    .into_iter()
    .chain([error_cell(error)])
    .chain(pr.values())
    .collect()
}

/// Returns the row of a project, or of one of its pull requests, that could not be queried:
/// the project, the error, and empty cells.
///
/// # Arguments
///
/// * `repo` - The project.
/// * `error` - Why it could not be queried.
fn error_row(repo: &Repo, error: ErrorCode) -> Vec<String> {
    repo.values()
        .into_iter()
        // Without pull request, there is no number nor link.
        .chain(vec![
            String::new();
            PullRequest::header().len() - Repo::columns().len()
        ])
        .chain([error.to_string()])
        .chain(vec![String::new(); PRMetadata::COLUMNS.len()])
        .collect()
}

/// Represents the metadata of a GitHub pull request.
#[derive(Debug, Clone, Eq, PartialEq, Default)]
struct PRMetadata {
    /// The number of the pull request.
    pr_number: u32,
    /// The local path of the file storing the discussion of the pull request.
    local_discussion_path: String,
    /// The user who created the pull request.
    user: String,
    /// The id of the user who created the pull request.
    user_id: u64,
    /// The timestamp of the creation of the pull request.
    created_at: u64,
    /// The timestamp of the last update of the pull request.
    updated_at: u64,
    /// The timestamp of the closing of the pull request.
    closed_at: u64,
    /// The timestamp of the merging of the pull request.
    merged_at: u64,
    /// Whether the pull request is a draft.
    draft: bool,
    /// The state of the pull request.
    state: String,
    /// The text field associated with the pull request.
    body: String,
}

impl PRMetadata {
    /// Columns of the metadata of a pull request, after the columns of the pull request and its link.
    const COLUMNS: [&'static str; 9] = [
        "local_discussion_path",
        "user",
        "user_id",
        "created_at",
        "updated_at",
        "closed_at",
        "merged_at",
        "draft",
        "state",
    ];

    /// Returns the values of the pull request, in the order of [`Self::COLUMNS`].
    fn values(&self) -> Vec<String> {
        vec![
            self.local_discussion_path.clone(),
            self.user.clone(),
            self.user_id.to_string(),
            self.created_at.to_string(),
            self.updated_at.to_string(),
            self.closed_at.to_string(),
            self.merged_at.to_string(),
            u8::from(self.draft).to_string(),
            self.state.clone(),
        ]
    }
}

impl FromGitHub for PRMetadata {
    type Complement = (u32, String);
    fn parse_json(json: &JsonValue, complement: Self::Complement) -> Result<Self, Error> {
        let pr_number: u32 = get_field::<u32>(json, "number")?;
        let created_at: i64 = if field_is_null(json, "created_at")? {
            0
        } else {
            Self::parse_date_time(json, "created_at")?
        };
        let updated_at: i64 = if field_is_null(json, "updated_at")? {
            0
        } else {
            Self::parse_date_time(json, "updated_at")?
        };
        let closed_at: i64 = if field_is_null(json, "closed_at")? {
            0
        } else {
            Self::parse_date_time(json, "closed_at")?
        };
        let merged_at: i64 = if field_is_null(json, "merged_at")? {
            0
        } else {
            Self::parse_date_time(json, "merged_at")?
        };
        let draft: bool = get_field::<bool>(json, "draft")?;
        let state: String = get_field::<String>(json, "state")?;
        let user_json: &JsonValue = &json["user"];
        let user: String = get_field::<String>(user_json, "login")?;
        let user_id: u64 = get_field::<u64>(user_json, "id")?;
        let (project_id, target): (u32, String) = complement;
        let path: String = Path::new(&target)
            .join((project_id % 10000).to_string())
            .join(project_id.to_string())
            .join(format!("{project_id}_{pr_number}.csv"))
            .to_string_lossy()
            .into_owned();
        let body: String = if field_is_null(json, "body")? {
            "".to_string()
        } else {
            clean_string_to_csv(&get_field::<String>(json, "body")?)
        };
        Ok(Self {
            local_discussion_path: path,
            pr_number,
            created_at: created_at as u64,
            updated_at: updated_at as u64,
            closed_at: closed_at as u64,
            merged_at: merged_at as u64,
            draft,
            state,
            user,
            user_id,
            body,
        })
    }
}

/// Scrape all pages of a GitHub API endpoint.
///
/// # Arguments
///
/// * `gh` - The GitHub client to use for making requests.
/// * `request` - A function that takes the number of items per page and the page number, and returns the URL of the GitHub API endpoint.
/// * `func` - The function processing each item in the response.
///
/// # Returns
///
/// A vector containing the results of applying `func` to each item in the response, with an invalid answer error
/// for the items it cannot process, or the error of the requests.
fn scrape_pages<T>(
    gh: &Github,
    request: &dyn Fn(usize, usize) -> String,
    func: &dyn Fn(JsonValue) -> Result<T, Error>,
) -> Result<Vec<Result<T, RowError>>, RowError> {
    let mut page: usize = 1;
    const PER_PAGE: usize = 100;
    let mut is_null: bool = false;
    let mut items: Vec<Result<T, RowError>> = Vec::new();
    while !is_null {
        let json: JsonValue = gh.request(&request(PER_PAGE, page))?;
        {
            if json.is_empty() {
                is_null = true;
            } else {
                items.extend(json.members().map(|item| {
                    func(item.clone()).map_err(|e| RowError::new(ErrorCode::InvalidAnswer, e))
                }));
                if items.is_empty() {
                    is_null = true;
                } else {
                    page += 1;
                }
            }
        }
    }
    Ok(items)
}

/// Type of text field that can appear in a pull request discussion.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum PRCommentType {
    /// Code review comment.
    Review,
    /// Comment mentioning specific code lines.
    Code,
    /// General discussion comment.
    Discussion,
    /// Pull request text
    Body,
}

/// Represents a comment in a GitHub pull request.
#[derive(Debug)]
struct PRComment {
    /// Unique identifier of the comment.
    id: i64,
    /// Username of the comment author.
    user: String,
    /// User ID of the comment author.
    user_id: u64,
    /// Type of comment (e.g., code review, general discussion, etc.)
    comment_type: PRCommentType,
    /// Timestamp of when the comment was created.
    created_at: u64,
    /// The text of the comment without newlines, quotes or commas.
    body: String,
}

impl PRComment {
    /// Returns the header of a discussion file.
    fn header() -> [&'static str; 7] {
        ["id", ERROR, "user", "user_id", "type", "created_at", "body"]
    }

    /// Returns the row of the comment in a discussion file.
    fn to_csv(&self) -> String {
        format!(
            "{},,{},{},{},{},\"{}\"",
            self.id,
            self.user,
            self.user_id,
            match self.comment_type {
                PRCommentType::Review => "review",
                PRCommentType::Code => "code",
                PRCommentType::Discussion => "discussion",
                PRCommentType::Body => "body",
            },
            self.created_at,
            clean_string_to_csv(&self.body)
        )
    }

    /// Returns the row of a comment that could not be read: its error, and empty cells.
    ///
    /// # Arguments
    ///
    /// * `error` - Why the comment could not be read.
    fn error_row(error: ErrorCode) -> String {
        Self::header()
            .iter()
            .map(|column| {
                if *column == ERROR {
                    error.to_string()
                } else {
                    String::new()
                }
            })
            .collect::<Vec<String>>()
            .join(",")
    }
}

impl FromGitHub for PRComment {
    type Complement = PRCommentType;

    fn parse_json(json: &JsonValue, complement: PRCommentType) -> Result<Self, Error> {
        let id: u64 = get_field::<u64>(json, "id")?;
        let user_json = &json["user"];
        let user: String = get_field::<String>(user_json, "login")?;
        let user_id: u64 = get_field::<u64>(user_json, "id")?;
        let created_at: i64 = if complement == PRCommentType::Review {
            if field_is_null(json, "submitted_at")? {
                0
            } else {
                PRMetadata::parse_date_time(json, "submitted_at")?
            }
        } else if field_is_null(json, "created_at")? {
            0
        } else {
            PRMetadata::parse_date_time(json, "created_at")?
        };
        let body = if field_is_null(json, "body")? {
            "".to_string()
        } else {
            get_field::<String>(json, "body")?
        };

        Ok(Self {
            id: id as i64,
            user,
            user_id,
            comment_type: complement,
            created_at: created_at as u64,
            body,
        })
    }
}

/// Scrapes all comments of a pull request and saves them to a CSV file.
///
/// # Arguments
///
/// * `gh` - The GitHub client to use for making requests.
/// * `repo_id` - The ID of the repository containing the pull request.
/// * `pr` - The metadata of the pull request.
///
/// # Returns
///
/// Why the comments could not be fetched, in which case no file is written, or `None` if they were saved.
/// Comments that cannot be read are saved as rows with an error code.
/// Fails if the file cannot be written.
fn scrape_pr_comments(gh: &Github, repo_id: u32, pr: &PRMetadata) -> Result<Option<RowError>> {
    let mut file_content: String = String::new();
    writeln!(&mut file_content, "{}", PRComment::header().join(","))?;

    // Body of the PR as the first comment.
    let pr_body: PRComment = PRComment {
        id: 0,
        user: pr.user.clone(),
        user_id: pr.user_id,
        comment_type: PRCommentType::Body,
        created_at: pr.created_at,
        body: pr.body.clone(),
    };

    writeln!(&mut file_content, "{}", pr_body.to_csv())?;

    // To get all the comments, we need to scrap three different endpoints.
    for t in [
        (PRCommentType::Discussion, "issues", "comments"),
        (PRCommentType::Code, "pulls", "comments"),
        (PRCommentType::Review, "pulls", "reviews"),
    ] {
        let comments = match scrape_pages(
            gh,
            &|per_page, page| {
                format!(
                    "https://api.github.com/repositories/{}/{}/{}/{}?per_page={}&page={}",
                    repo_id, t.1, pr.pr_number, t.2, per_page, page
                )
            },
            &|json| Ok(PRComment::parse_json(&json, t.0)?.to_csv()),
        ) {
            Ok(comments) => comments,
            Err(e) => return Ok(Some(e)),
        };
        for comment in comments {
            let row: String = comment.unwrap_or_else(|e| {
                debug!(
                    "A comment of pull request {} of repository {repo_id}: {e}",
                    pr.pr_number
                );
                PRComment::error_row(e.code)
            });
            writeln!(&mut file_content, "{row}")?;
        }
    }

    let mut output_file: CSVFile = CSVFile::new(&pr.local_discussion_path, FileMode::Overwrite)?;
    write!(&mut output_file, "{file_content}")?;
    output_file.flush()?;
    Ok(None)
}

#[cfg(test)]
mod tests {

    use anyhow::ensure;

    use crate::utils::logger::test_logger;

    use super::*;

    const TEST_DATA: &str = "tests/data/phases/pull_request";

    fn test_phase_pull_request(
        input_file: &str,
        output_file: &str,
        target: &str,
        pr_paths: &Vec<String>,
    ) -> Result<()> {
        ensure!(std::path::Path::new(&input_file).exists());

        let tokens_file: String = "ghtokens.csv".to_string();

        run(
            input_file,
            Some(&output_file.to_string()),
            &tokens_file,
            0,
            false,
            false,
            target,
            None,
            &[],
            test_logger(),
        )?;

        for pr_path in pr_paths {
            let pr_discussion = open_csv(pr_path, None, None, &[])?;
            let pr_discussion_expected = open_csv(&format!("{pr_path}.expected"), None, None, &[])?;
            assert_eq!(pr_discussion, pr_discussion_expected);
            delete_file(pr_path, false)?;
        }

        let output_df = open_csv(output_file, None, None, &[])?;
        let expected_df = open_csv(&format!("{output_file}.expected"), None, None, &[])?;
        assert_eq!(expected_df, output_df);
        delete_file(output_file, false)
    }

    /// Returns a repository without license nor commit.
    fn repo(source: Source) -> Repo {
        Repo {
            id: Some(42),
            source,
            name: Some("owner/repo".to_string()),
            license: None,
            commit: None,
        }
    }

    /// Returns the cell of a row of the output in a column.
    fn cell<'a>(row: &'a [String], column: &str) -> &'a str {
        let index: usize = header()
            .iter()
            .position(|c| *c == column)
            .expect("unknown column");
        &row[index]
    }

    #[test]
    fn rows_of_pull_requests() {
        let pr = PRMetadata {
            pr_number: 3,
            user: "someone".to_string(),
            draft: true,
            state: "open".to_string(),
            ..Default::default()
        };
        let row: Vec<String> = pr_row(&repo(Source::GitHub), &pr, None);
        assert_eq!(row.len(), header().len());
        assert_eq!(cell(&row, "repo_name"), "owner/repo");
        assert_eq!(cell(&row, "pr_number"), "3");
        assert_eq!(cell(&row, "url"), "https://github.com/owner/repo/pull/3");
        assert_eq!(cell(&row, ERROR), "");
        assert_eq!(cell(&row, "user"), "someone");
        assert_eq!(cell(&row, "draft"), "1");
        assert_eq!(cell(&row, "state"), "open");

        // A pull request whose discussion could not be fetched keeps its metadata.
        let row: Vec<String> = pr_row(&repo(Source::GitHub), &pr, Some(ErrorCode::Network));
        assert_eq!(cell(&row, ERROR), "E001");
        assert_eq!(cell(&row, "user"), "someone");
    }

    #[test]
    fn rows_of_projects_not_on_github() -> Result<()> {
        let local: Repo = repo(Source::Local);
        let error: RowError = pull_requests(&Github::new(Vec::new()), 42, &local, "")
            .expect_err("a local project is not queried");
        assert_eq!(error.code, ErrorCode::UnsupportedSource);
        let row: Vec<String> = error_row(&local, error.code);
        assert_eq!(row.len(), header().len());
        assert_eq!(cell(&row, "source"), "local");
        assert_eq!(cell(&row, "repo_name"), "owner/repo");
        assert_eq!(cell(&row, "pr_number"), "");
        assert_eq!(cell(&row, "url"), "");
        assert_eq!(cell(&row, ERROR), "E010");
        assert_eq!(cell(&row, "state"), "");
        Ok(())
    }

    #[test]
    fn rows_of_unreadable_comments() {
        assert_eq!(PRComment::error_row(ErrorCode::InvalidAnswer), ",E002,,,,,");
    }

    #[test]
    #[ignore = "requires network access and valid GitHub tokens"]
    fn test_pr_empty_output() -> Result<()> {
        test_phase_pull_request(
            &format!("{TEST_DATA}/repos.csv"),
            &format!("{TEST_DATA}/repos.csv.pulls.csv"),
            &format!("{TEST_DATA}/prs"),
            &vec![
                format!("{}/prs/5983/1128315983/1128315983_1.csv", TEST_DATA),
                format!("{}/prs/5983/1128315983/1128315983_2.csv", TEST_DATA),
            ],
        )
    }

    #[test]
    #[ignore = "requires network access and valid GitHub tokens"]
    fn test_pr_with_output() -> Result<()> {
        let input_path: String = format!("{TEST_DATA}/repos2.csv");
        std::fs::copy(
            format!("{TEST_DATA}/repos_complete.csv.expected"),
            format!("{TEST_DATA}/repos_complete.csv"),
        )?;
        test_phase_pull_request(
            &input_path,
            &format!("{TEST_DATA}/repos_complete.csv"),
            &format!("{TEST_DATA}/prs2"),
            &vec![],
        )
    }

    #[test]
    #[ignore = "requires network access and valid GitHub tokens"]
    fn test_pr_with_partial_output() -> Result<()> {
        let input_path: String = format!("{TEST_DATA}/repos3.csv");
        let output_path: String = format!("{TEST_DATA}/repos_partial_output.csv.temp");
        std::fs::copy(
            format!("{TEST_DATA}/repos_partial_output.csv"),
            &output_path,
        )?;
        ensure!(std::path::Path::new(&output_path).exists());

        test_phase_pull_request(
            &input_path,
            &output_path,
            &format!("{TEST_DATA}/prs3"),
            &vec![],
        )
    }

    #[test]
    #[ignore = "requires network access and valid GitHub tokens"]
    fn test_language_scraper_inexistent() -> Result<()> {
        test_phase_pull_request(
            &format!("{TEST_DATA}/invalid.csv"),
            &format!("{TEST_DATA}/invalid.csv.pulls.csv"),
            &format!("{TEST_DATA}/prs_invalid"),
            &vec![],
        )
    }
}
