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

//! Collect pull requests of GitHub projects. The input file must be a valid CSV file where one of the columns (\"name\") contains the full names of the projects, and another one (\"id\") contains their ids.
//! The program sends requests to the GitHub API to collect metadata about the pull requests of the projects in the input file, as well as the comments in each pull request.
//! Projects are chosen randomly without replacement. The metadata of the pull requests is saved in a new CSV file. If the program is interrupted, it
//! can be restarted and will continue from where it left off. The comments of each pull request are saved in separate CSV files in the target directory.
//! By default, the name of the output file is the same as the input file with the suffix '.pulls.csv'.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::io::Write;
use std::iter::FromIterator as _;
use std::path::Path;

use crate::utils::csv::*;
use crate::utils::dataframes::u32;
use crate::utils::error::*;
use crate::utils::fs::*;
use crate::utils::github::*;
use crate::utils::github_api::*;
use crate::utils::json::*;
use crate::utils::logger::Logger;
use clap::ArgAction;
use clap::{Arg, Command};
use indicatif::ProgressBar;
use json::JsonValue;
use polars::frame::DataFrame;
use polars::prelude::*;
use rand::rngs::StdRng;
use rand::seq::SliceRandom as _;
use rand::SeedableRng;

/// Command line arguments parsing.
pub fn cli() -> Command {
    Command::new("pr")
        .about("Collect pull requests of GitHub projects")
        .long_about(
            "Collect pull requests of GitHub projects. The input file must be a valid CSV file where one of the columns (\"name\") contains the full names of the projects, and another one (\"id\") contains their ids.\n\
            The program sends requests to the GitHub API to collect metadata about the pull requests of the projects in the input file, as well as the comments in each pull request.\n\
            Projects are chosen randomly without replacement. The metadata of the pull requests is saved in a new CSV file.\nIf the program is interrupted, it \
            can be restarted and will continue from where it left off.\n The comments of each pull request are saved in separate CSV files in the target directory.\n\
            By default, the name of the output file is the same as the input file with the suffix '.pulls.csv'.\n"
        )
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
                .help("Path to the input csv file to use. One of the columns must contain the full names of the projects. ")
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
            Arg::new("ids")
                .long("ids")
                .help("Name of the column containing the ids of the projects.")
                .value_name("COLUMN_NAME")
                .default_value("id")
        )
        .arg(
            Arg::new("names")
                .long("names")
                .help("Name of the column containing the full names of the projects.")
                .value_name("COLUMN_NAME")
                .default_value("name")
        )
        .arg(
            Arg::new("sub")
                .long("sub")
                .value_name("NUMBER_OF_PROJECTS")
                .help("Number of projects to sample from the input file. \
                       If not specified, all remaining projects in the input file are used.")
        )
}

/// Collects pull requests about GitHub projects.
///
/// The input must be a valid CSV file where the first column is the id of the project and the second column is the full name of the project.
/// Other columns are ignored. Such a file can be obtained by running the random-id-sampling program. Ids are chosen in a random order from the file.
///
/// The output has the following columns:
/// * id: the id of the project.
/// * name: the full name of the project.
/// * pr_number: the pull request number.
/// * file_path: the path of the file storing the contents of the pull request.
/// * user: the user who created the pull request.
/// * user_id: the id of the user who created the pull request.
/// * comments: the number of comments on the pull request.
/// * created_at: the timestamp of the creation of the pull request.
/// * updated_at: the timestamp of the last update of the pull request.
/// * closed_at: the timestamp of the closing of the pull request.
/// * merged_at: the timestamp of the merging of the pull request.
/// * draft: whether the pull request is a draft.
/// * state: the state of the pull request.
/// * commits: the number of commits in the pull request.
/// * additions: the number of additions in the pull request.
/// * deletions: the number of deletions in the pull request.
/// * changed_files: the number of changed files in the pull request.
///
/// # Arguments
///
/// * `input_path` - The path to the input file.
/// * `output_path` - The path to the output file. If None, the output file will be named as the input file + ".pulls.csv".
/// * `tokens` - The path to the file containing the GitHub tokens.
/// * `seed` - The seed to use for the random number generator.
/// * `force` - Whether to override the output file if it already exists.
/// * `ids` - The name of the column containing the ids of the projects.
/// * `names` - The name of the column containing the full names of the projects.
/// * `target` - The target directory where to store the pull request files.
/// * `sub` - The number of projects to sample from the input file. If not specified, all remaining projects in the input file are used.
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
    ids: &str,
    names: &str,
    target: &str,
    sub: Option<usize>,
    logger: &mut Logger,
) -> Result<(), Error> {
    // Check if the token file is valid.
    logger.log_tokens(tokens)?;

    // Load input file
    let input_file: DataFrame = logger.log_completion("Loading input file", || {
        open_csv(
            input_path,
            Some(Schema::from_iter(vec![
                Field::new(ids.into(), DataType::UInt32),
                Field::new(names.into(), DataType::String),
            ])),
            Some(vec![ids, names]),
        )
    })?;

    logger.log_seed(seed)?;

    let mut shuffled_idx: Vec<usize> = (0..input_file.height()).collect();

    // Load the ids from the input file in random order.
    logger.log_completion("Loading project IDs in random order", || {
        let mut rng: StdRng = SeedableRng::seed_from_u64(seed);
        shuffled_idx.shuffle(&mut rng);
        Ok(())
    })?;

    let shuffled_rows = shuffled_idx.into_iter().map(|idx| {
        // Safe unwrap
        let row = input_file.get_row(idx).unwrap().0;

        match (row[0].clone(), row[1].clone()) {
            (AnyValue::UInt32(id), AnyValue::String(name)) => Ok((id, name)),
            _ => Err(idx),
        }
    });

    let n_pr: usize = input_file.height();

    logger.log(&format!("  {} projects found.", n_pr))?;

    // Name of the output file.
    let default_output_path: String = format!("{}.pulls.csv", &input_path);
    let output_file_path: &str = output_path.unwrap_or(&default_output_path);

    // Load the previous results.
    let previous_results: HashSet<u32> = if force {
        HashSet::new()
    } else {
        logger.log_completion("Resuming progress", || {
            // Open output file if it exists and load the ids of the projects that have already been processed.
            Ok(if Path::new(output_file_path).exists() {
                let df_res: DataFrame = open_csv(
                    output_file_path,
                    Some(Schema::from_iter(vec![Field::new(
                        ids.into(),
                        DataType::UInt32,
                    )])),
                    Some(vec![ids]),
                )?;
                u32(&df_res, ids)?.into_iter().collect()
            } else {
                HashSet::new()
            })
        })?
    };

    if !previous_results.is_empty() {
        logger.log(&format!(
            "  the metadata of {} projects have already been queried",
            previous_results.len()
        ))?;
    }

    let mut output_file: CSVFile = CSVFile::new(
        output_file_path,
        if force {
            FileMode::Overwrite
        } else {
            FileMode::Append
        },
    )?;

    output_file.write_header(PRMetadata::header())?;

    let gh = Github::new(tokens);

    logger.log("Starting to query the GitHub API...")?;

    // Number of projects to sample.
    let mut n: usize = match sub {
        Some(m) => m,
        None => n_pr - previous_results.len(),
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
        match row {
            Ok((id, full_name)) => {
                if !previous_results.contains(&id) {
                    // Row to write in the output file.
                    let mut pull_requests: String = String::new();

                    // PRs are fetched page by page (100 PRs per page).

                    if let Ok(pages) = scrape_pages(
                        &gh,
                        &|per_page, page| {
                            format!("https://api.github.com/repositories/{}/pulls?state=all&per_page={}&page={}", id, per_page, page)
                        },
                        &|json| {
                            let mut pr_metadata: PRMetadata =
                                PRMetadata::parse_json(&json, (id, target.to_string()))?;
                            scrape_pr_comments(&gh, id, &pr_metadata).unwrap_or_else(|_| {
                                pr_metadata.file_path = String::new();
                            });
                            Ok(pr_metadata)
                        },
                    ) {
                        for pr_res in pages {
                            let obj: PRMetadata = pr_res.unwrap_or_default();
                            map_err(
                                writeln!(
                                    &mut pull_requests,
                                    "{}",
                                    obj.to_csv((id, full_name.to_string()))
                                ),
                                "Could not write to string builder",
                            )?;
                        }
                        map_err(
                            write!(&mut output_file, "{}", pull_requests),
                            &format!("Could not write to file {}", &output_file_path),
                        )?;
                    }
                    progress_bar.inc(1);
                    n -= 1;
                }
            }
            Err(idx) => {
                map_err(
                    row,
                    &format!("Could not parse row {} in the input file", idx),
                )?;
            }
        }
    }
    Ok(())
}

/// Represents the metadata of a GitHub pull request.
#[derive(Debug, Clone, Eq, PartialEq, Default)]
struct PRMetadata {
    /// The number of the pull request.
    pr_number: u32,
    /// The path of the file storing the contents of the pull request.
    file_path: String,
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

impl ToCSV for PRMetadata {
    /// Id of the project and project name
    type Key = (u32, String);

    fn header() -> &'static [&'static str] {
        &[
            "id",
            "name",
            "pr_number",
            "file_path",
            "user",
            "user_id",
            "created_at",
            "updated_at",
            "closed_at",
            "merged_at",
            "draft",
            "state",
        ]
    }

    fn to_csv(&self, key: Self::Key) -> String {
        format!(
            "{},{},{},{},{},{},{},{},{},{},{},{}",
            key.0,
            key.1,
            self.pr_number,
            self.file_path,
            self.user,
            self.user_id,
            self.created_at,
            self.updated_at,
            self.closed_at,
            self.merged_at,
            if self.draft { 1 } else { 0 },
            self.state,
        )
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
        let path: String = format!(
            "{}/{}/{}/{}_{}.csv",
            complement.1,
            complement.0 % 10000,
            complement.0,
            complement.0,
            pr_number
        );
        let body: String = if field_is_null(json, "body")? {
            "".to_string()
        } else {
            clean_string_to_csv(&get_field::<String>(json, "body")?)
        };
        Ok(Self {
            file_path: path,
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
/// A vector containing the results of applying `func` to each item in the response, or an error if
/// an error occurred during the requests.
fn scrape_pages<T>(
    gh: &Github,
    request: &dyn Fn(usize, usize) -> String,
    func: &dyn Fn(JsonValue) -> Result<T, Error>,
) -> Result<Vec<Result<T, Error>>, Error> {
    let mut page: usize = 1;
    const PER_PAGE: usize = 100;
    let mut is_null: bool = false;
    let mut items: Vec<Result<T, Error>> = Vec::new();
    while !is_null {
        match gh.request(&request(PER_PAGE, page)) {
            Ok(json) => {
                if json.is_empty() {
                    is_null = true;
                } else {
                    items.extend(json.members().map(|item| func(item.clone())));
                    if items.is_empty() {
                        is_null = true;
                    } else {
                        page += 1;
                    }
                }
            }
            Err(e) => map(
                e,
                &format!("Error during GitHub request {}", &request(PER_PAGE, page)),
            )
            .to_res()?,
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
    /// Unknown type (because the pull request could not be fetched).
    Error,
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

impl ToCSV for PRComment {
    type Key = ();

    fn header() -> &'static [&'static str] {
        &["id", "user", "user_id", "type", "created_at", "body"]
    }

    fn to_csv(&self, _key: Self::Key) -> String {
        format!(
            "{},{},{},{},{},\"{}\"",
            self.id,
            self.user,
            self.user_id,
            match self.comment_type {
                PRCommentType::Review => "review",
                PRCommentType::Code => "code",
                PRCommentType::Discussion => "discussion",
                PRCommentType::Body => "body",
                PRCommentType::Error => "error",
            },
            self.created_at,
            clean_string_to_csv(&self.body)
        )
    }
}

impl Default for PRComment {
    fn default() -> Self {
        Self {
            id: -1,
            user: String::new(),
            user_id: 0,
            comment_type: PRCommentType::Error,
            created_at: 0,
            body: String::new(),
        }
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
/// Unit if the comments were successfully scraped and saved, or an error message if an error occurred.
fn scrape_pr_comments(gh: &Github, repo_id: u32, pr: &PRMetadata) -> Result<(), Error> {
    let mut file_content: String = String::new();
    let mut output_file: CSVFile = CSVFile::new(&pr.file_path, FileMode::Overwrite)?;
    map_err(
        writeln!(&mut file_content, "{}", PRComment::header().join(",")),
        "Could not write headers to string builder",
    )?;

    // Body of the PR as the first comment.
    let pr_body: PRComment = PRComment {
        id: 0,
        user: pr.user.clone(),
        user_id: pr.user_id,
        comment_type: PRCommentType::Body,
        created_at: pr.created_at,
        body: pr.body.clone(),
    };

    map_err(
        writeln!(&mut file_content, "{}", pr_body.to_csv(())),
        "Could not write PR comments to string builder",
    )?;

    // To get all the comments, we need to scrap three different endpoints.
    for t in [
        (PRCommentType::Discussion, "issues", "comments"),
        (PRCommentType::Code, "pulls", "comments"),
        (PRCommentType::Review, "pulls", "reviews"),
    ] {
        for row_res in scrape_pages(
            gh,
            &|per_page, page| {
                format!(
                    "https://api.github.com/repositories/{}/{}/{}/{}?per_page={}&page={}",
                    repo_id, t.1, pr.pr_number, t.2, per_page, page
                )
            },
            &|json| Ok(PRComment::parse_json(&json, t.0)?.to_csv(())),
        )? {
            match row_res {
                Ok(row) => map_err(
                    writeln!(&mut file_content, "{}", row),
                    "Could not write PR comments to string builder",
                )?,
                Err(_) => {
                    writeln!(&mut file_content, "{}", PRComment::default().to_csv(())).unwrap()
                }
            }
        }
    }

    map_err(
        write!(&mut output_file, "{}", file_content),
        &format!("Could not write to file {}", &pr.file_path),
    )
}

#[cfg(test)]
mod tests {

    use super::*;

    const TEST_DATA: &str = "tests/data/phases/pull_request";

    fn test_phase_pull_request(
        input_file: &str,
        output_file: &str,
        target: &str,
        pr_paths: &Vec<String>,
    ) {
        assert!(std::path::Path::new(&input_file).exists());

        let tokens_file: String = "ghtokens.csv".to_string();

        let run_scraper: Result<(), Error> = run(
            &input_file,
            Some(&output_file.to_string()),
            &tokens_file,
            0,
            false,
            "id",
            "name",
            &target,
            None,
            &mut Logger::new(),
        );
        assert!(run_scraper.is_ok());

        for pr_path in pr_paths {
            let pr_discussion = open_csv(pr_path, None, None);
            assert!(pr_discussion.is_ok());
            let pr_discussion = pr_discussion.unwrap();

            let pr_discussion_expected = open_csv(&format!("{}.expected", pr_path), None, None);
            assert!(pr_discussion_expected.is_ok());
            let pr_discussion_expected = pr_discussion_expected.unwrap();

            assert_eq!(pr_discussion, pr_discussion_expected);
            assert!(delete_file(pr_path, false).is_ok());
        }

        let output_df = open_csv(&output_file, None, None);
        assert!(output_df.is_ok());
        let output_df = output_df.unwrap();
        let expected_df = open_csv(&format!("{}.expected", output_file), None, None);
        assert!(expected_df.is_ok());
        let expected_df = expected_df.unwrap();
        assert!(expected_df.equals(&output_df));
        assert!(delete_file(&output_file, false).is_ok());
    }

    #[test]
    fn test_pr_empty_output() {
        test_phase_pull_request(
            &format!("{}/repos.csv", TEST_DATA),
            &format!("{}/repos.csv.pulls.csv", TEST_DATA),
            &format!("{}/prs", TEST_DATA),
            &vec![
                format!("{}/prs/5983/1128315983/1128315983_1.csv", TEST_DATA),
                format!("{}/prs/5983/1128315983/1128315983_2.csv", TEST_DATA),
            ],
        );
    }

    #[test]
    fn test_pr_with_output() {
        let input_path: String = format!("{}/repos2.csv", TEST_DATA);
        let copy_result: Result<u64, std::io::Error> = std::fs::copy(
            &format!("{}/repos_complete.csv.expected", TEST_DATA),
            &format!("{}/repos_complete.csv", TEST_DATA),
        );
        assert!(copy_result.is_ok());
        test_phase_pull_request(
            &input_path,
            &format!("{}/repos_complete.csv", TEST_DATA),
            &format!("{}/prs2", TEST_DATA),
            &vec![],
        );
    }

    #[test]
    fn test_pr_with_partial_output() {
        let input_path: String = format!("{}/repos3.csv", TEST_DATA);
        let output_path: String = format!("{}/repos_partial_output.csv.temp", TEST_DATA);
        let copy_result: Result<u64, std::io::Error> = std::fs::copy(
            &format!("{}/repos_partial_output.csv", TEST_DATA),
            &output_path,
        );
        assert!(copy_result.is_ok());
        assert!(std::path::Path::new(&output_path).exists());

        test_phase_pull_request(
            &input_path,
            &output_path,
            &format!("{}/prs3", TEST_DATA),
            &vec![],
        );
    }

    #[test]
    fn test_language_scraper_inexistent() {
        test_phase_pull_request(
            &format!("{}/invalid.csv", TEST_DATA),
            &format!("{}/invalid.csv.pulls.csv", TEST_DATA),
            &format!("{}/prs_invalid", TEST_DATA),
            &vec![],
        );
    }
}
