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

//! This program is used to download all the GitHub projects from a list. A list of ids, full names and latest commit SHA of projects must be
//! provided in a CSV file. The program will then make requests to the GitHub API to download every repository in the list in random order
//! Ultimately only the files written in selected languages and featuring one of the provided keywords are kept. For each extension, a list of
//! additional keywords can be also provided.
//! The id of the downloaded repositories are logged in two CSV files. The first one (ending in .project_log) gathers data on a per project basis,
//! the second one (ending in .file_log) gathers data on a per file basis. If the program is interrupted, it can be restarted and will continue from
//! where it left off. A skip flag can be provided to avoid downloading the repositories again and instead only logging the files.
//! A count flag can be provided to compute statistics on the downloaded projects without deleting any file.
//! For usage and command line arguments refer to the [`cli_args`] function.

use crate::utils::logger::Logger;
use clap::{Arg, ArgAction, Command};
use indicatif::ProgressBar;
use polars::prelude::{AnyValue, DataType, Field, Schema};
use rand::rngs::StdRng;
use rand::seq::SliceRandom as _;
use rand::SeedableRng;
use reqwest::blocking::Response;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};
use std::collections::HashSet;
use std::fmt::Write as FmtWrite;
use std::fs::File;
use std::io::{copy, BufRead, Write};
use std::iter::FromIterator as _;
use std::path::Path;
use std::sync::Mutex;
use std::thread::sleep;
use std::time::Duration;
use zip_extensions::zip_extract::zip_extract;

use crate::shell_commands::{FileType, FindCommand, ShellCommand};
use crate::utils::csv::*;
use crate::utils::error::*;
use crate::utils::fs::*;
use crate::utils::regex::*;

/// Command line arguments parsing.
pub fn cli() -> Command {
    Command::new("download")
        .about("Downloads all github repositories from a list and keeps only the files that satisfy user defined criteria.")
        .long_about(
            "Downloads all github repositories from a list and keeps only the files that satisfy the following criteria:\n    \
            - Their extension is in the list of extensions provided in the command line arguments.\n    \
            - They contain a keyword related to floating point arithmetic.\n    \
            - They are not symbolic links.\n\n\
            Furthermore, all empty directories are removed. All downloaded repositories are logged in a CSV file to track progress \
            and be able to resume the download if the program is interrupted.\nThe name of the log file is the same as the input file \
            with the extension \".project_log\".\nEvery file is also logged in a CSV file with the same name as the input file with the \
            extension \".file_log\".\n\
            Ids are chosen in a random order from the input file."
        )
        .author("Andrea Gilot <andrea.gilot@it.uu.se>")
        .disable_version_flag(true)
        .arg(
            Arg::new("input")
                .short('i')
                .long("input")
                .value_name("INPUT_FILE.csv")
                .help("Path to the input csv file to use. It must be a valid CSV file where the first column is the id of the project, \
                       the second column is the full name of the project and the third column is the hash of the latest commit. Other columns are ignored.")
                .required(true)
        )
        .arg(
            Arg::new("projects")
                .short('p')
                .long("projects")
                .value_name("OUTPUT_FILE_PROJECTS.csv")
                .help("Path to the output csv file storing the project statistics.")
                .required(false),
        )
        .arg(
            Arg::new("files")
                .short('f')
                .long("files")
                .value_name("OUTPUT_FILE_FILES.csv")
                .help("Path to the output csv file storing the file statistics.")
                .required(false),
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
                .help("Path to the directory where projects will be downloaded. The directory will be created if it does not exist.")
                .required(true)
        )
        .arg(
            Arg::new("keywords")
                .short('k')
                .long("keywords")
                .num_args(1..)
                .action(ArgAction::Append)
                .value_name("KEYWORDS_FILES.json")
                .help("List of files containing the list of extensions and keywords to use. The files must be in JSON format.\n\
                       The extensions should be written without the period (`java` instead of `.java`). The files must have the following structure:\n    \
                        {\n        \
                            \"extensions\": {\n            \
                                \"ext1\": [\"kw11\", \"kw12\", ...],\n            \
                                \"ext2\": [\"kw21\", \"kw22\", ...],\n            \
                                ...\n        \
                            },\n        \
                            \"keywords\": [\"kw1\", \"kw2\", ...]\n    \
                        }")
                .required(true)
        )
        .arg(
            Arg::new("skip")
                .long("skip")
                .help("Skip the downloading of the repositories.")
                .action(ArgAction::SetTrue)
        )
        .arg(
            Arg::new("count")
                .long("count")
                .help("Compute statistics on the downloaded projects without deleting any file.")
                .action(ArgAction::SetTrue)
        )
        .arg(
            Arg::new("force")
                .long("force")
                .help("Overwrite the log files if they exist.")
                .action(ArgAction::SetTrue)
        )
        .arg(
            Arg::new("threads")
                .short('n')
                .help("Number of threads to use when not downloading and computing statistic locally instead.")
                .requires("skip")
                .default_value("1")
                .value_parser(clap::value_parser!(usize)),
        )
        .arg(
            Arg::new("seed")
                .short('s')
                .long("seed")
                .value_name("SEED")
                .help("Seed used to randomly shuffle the input data.")
                .default_value("12393566520031723923")
                .value_parser(clap::value_parser!(u64)),
        )
}

/// Runs the downloader.
///
/// # Arguments
///
/// * `input_file` - Path to the input csv file to use.
/// * `target` - Path to the directory where projects will be downloaded.
/// * `tokens_file` - Path to the file containing the GitHub tokens to use.
/// * `keywords_files` - Path to the files containing the list of extensions and keywords to use.
/// * `skip` - If true, skip the downloading of the repositories.
/// * `count` - If true, compute statistics on the downloaded projects without deleting any file.
/// * `overwrite` - If true, overwrite the log files if they exist.
/// * `seed` - The seed used to shuffle the projects.
/// * `logger` - The logger to use to display information about the progress of the program.
///
/// Downloads all github repositories from a list and keeps only the files that satisfy the following criteria:
/// * Their extension is in the list of extensions provided in the command line arguments.
/// * They contain a keyword related to floating point arithmetic.
/// * They are not symbolic links.
///
/// Furthermore, all empty directories are removed. All downloaded repositories are logged in a CSV file to track progress
/// and be able to resume the download if the program is interrupted. The name of the log file is the same as the input file
/// with the extension ".project_log". Every file is also logged in a CSV file with the same name as the input file with the
/// extension ".file_log".
///
/// The input (i.e. the file where the ids are stored) must be a valid CSV file where the first column is the id of the project,
/// the second column is the full name of the project and the third column is the hash of the latest commit. Other columns are ignored.
/// Ids are chosen in a random order from the file.
///
/// If the target directory does not exist, it will be created.
///
/// The tokens file must be a valid CSV file with one column named 'token' and where every line is a valid GitHub token
///
/// The lists of extensions and keywords needs to be stored in a JSON file. The extensions should be written without the period (`java` instead of `.java`).
/// The file must have the following structure:
///
/// ```json
/// {
///     "extensions": {
///         "ext1": ["kw11", "kw12", ...],
///         "ext2": ["kw21", "kw22", ...],
///         ...
///     },
///     "keywords": ["kw1", "kw2", ...]
/// }
/// ```
///
/// # Example
///
/// The following configuration file will download all the C, Java and TypeScript files that contain floating point types:
///
/// ```json
/// {
///     "extensions": {
///         "c": [],
///         "java": [],
///         "ts": ["number"],
///         ...
///     },
///     "keywords": ["float", "double"]
/// }
/// ```
///
pub fn run(
    input_file_path: &str,
    projects_output_path: Option<&str>,
    files_output_path: Option<&str>,
    target: &str,
    tokens_file: &str,
    keywords_file_paths: &[&str],
    skip: bool,
    count: bool,
    overwrite: bool,
    seed: u64,
    logger: &mut Logger,
    thread: usize,
) -> Result<(), Error> {
    // Number of columns in the log files
    const PROJECT_LOG_COLS: usize = 14;
    const FILE_LOG_COLS: usize = 6;

    // Check if the token file is valid and load the tokens.
    let tokens: Vec<String> = if skip {
        (0..thread).map(|n| n.to_string()).collect()
    } else {
        logger.log_tokens(tokens_file)?
    };

    let input_file = logger.log_completion("Loading input file", || {
        open_csv(
            input_file_path,
            Some(Schema::from_iter(vec![
                Field::new("id".into(), DataType::UInt32),
                Field::new("name".into(), DataType::String),
                Field::new("path".into(), DataType::String),
                Field::new("latest_commit".into(), DataType::String),
            ])),
            Some(if skip {
                vec!["id", "path"]
            } else {
                vec!["id", "name", "latest_commit"]
            }),
        )
    })?;

    let mut shuffled_idx = (0..input_file.height()).collect::<Vec<usize>>();

    // Load the ids from the input file in random order.
    logger.log_completion("Loading project IDs in random order", || {
        let mut rng: StdRng = SeedableRng::seed_from_u64(seed);
        shuffled_idx.shuffle(&mut rng);
        Ok(())
    })?;

    let shuffled_rows = shuffled_idx.into_iter().map(|idx| {
        let row = input_file.get_row(idx).unwrap().0;

        if skip {
            match (row[0].clone(), row[1].clone()) {
                (AnyValue::UInt32(id), AnyValue::String(path)) => Ok((idx, id, path, None)),
                _ => Err(idx),
            }
        } else {
            match (row[0].clone(), row[1].clone(), row[2].clone()) {
                (AnyValue::UInt32(id), AnyValue::String(name), AnyValue::String(latest_commit)) => {
                    Ok((idx, id, name, Some(latest_commit)))
                }
                _ => Err(idx),
            }
        }
    });

    let n_proj = input_file.height();
    logger.log(&format!("  {} projects found.", n_proj))?;

    const MAX_SUBDIRS: usize = 30000;

    if !skip {
        // Create the target directory if it does not exist.
        create_dir(target)?;

        // Create subsubdirectories to avoid reaching the limit of 32k subdirectories on some filesystems.
        for i in 0..(n_proj / MAX_SUBDIRS + 1) {
            create_dir(format!("{}/{}", target, i))?;
        }
    }

    // Open the log file for the projects or create it if it does not exist.
    let default_project_log_path = format!("{}.project_log.csv", input_file_path);
    let project_log_path: &str = projects_output_path.unwrap_or(&default_project_log_path);

    // Load previous results if the skip flag is not set.

    let previous_results: HashSet<u32> = logger.log_completion("Resuming progress", || {
        Ok(if overwrite || !Path::new(&project_log_path).exists() {
            HashSet::<u32>::new()
        } else {
            let project_log_file = CSVFile::new(project_log_path, FileMode::Read)?;
            let prev_res: HashSet<u32> = project_log_file.column::<u32>(0)?.into_iter().collect();
            prev_res
        })
    })?;

    if !previous_results.is_empty() {
        logger.log(&format!(
            "  {} projects have already been downloaded",
            previous_results.len()
        ))?;
    }

    let keyword_files: KeywordFiles = logger.log_completion("Loading keywords", || {
        KeywordFiles::new().add_files(keywords_file_paths)
    })?;

    let files_with_kw_headers: String = keyword_files
        .paths
        .iter()
        .map(|p| format!("files_with_{}", p))
        .collect::<Vec<String>>()
        .join(",");
    let loc_of_files_with_kw_headers: String = keyword_files
        .paths
        .iter()
        .map(|p| format!("loc_of_files_with_{}", p))
        .collect::<Vec<String>>()
        .join(",");
    let words_of_files_with_kw_headers: String = keyword_files
        .paths
        .iter()
        .map(|p| format!("words_of_files_with_{}", p))
        .collect::<Vec<String>>()
        .join(",");
    let keyword_match_headers: String = keyword_files.paths.join(",");

    let word_counter: Matcher = Matcher::words_matcher();

    let mut project_log_file = CSVFile::new(
        project_log_path,
        if overwrite {
            FileMode::Overwrite
        } else {
            FileMode::Append
        },
    )?;

    // If the file has no header, write the header.
    let project_log_headers: [&str; PROJECT_LOG_COLS] = [
        "id",
        "path",
        "name",
        "latest_commit",
        "files",
        "loc",
        "words",
        "files_with_kw",
        &files_with_kw_headers,
        "loc_with_kw",
        &loc_of_files_with_kw_headers,
        "words_with_kw",
        &words_of_files_with_kw_headers,
        &keyword_match_headers,
    ];

    project_log_file.write_header(&project_log_headers)?;

    // Open the log file for the files or create it if it does not exist.
    // If the overwrite flag is set, the file is generated anew.
    let default_file_log_path = format!("{}.file_log.csv", input_file_path);
    let file_log_path: &str = files_output_path.unwrap_or(&default_file_log_path);
    let mut file_log = CSVFile::new(
        file_log_path,
        if overwrite {
            FileMode::Overwrite
        } else {
            FileMode::Append
        },
    )?;

    let file_log_headers: [&str; FILE_LOG_COLS] = [
        "id",
        "name",
        "language",
        "loc",
        "words",
        &keyword_match_headers,
    ];

    file_log.write_header(&file_log_headers)?;

    // Iterate over the projects and collect metadata.
    let iter = Mutex::new(shuffled_rows);

    logger.log("Starting download...")?;
    logger.log("")?;

    // Numbers of threads to be spawned.
    let n = tokens.len();

    // Every thread comes with a sender channel.
    // The sender channel is used to send information about the downloaded repository back to the main thread.
    // The receiver channel is used by the main thread to collect and write the information to the log file.
    let (tx, rx) = crossbeam_channel::unbounded::<Option<Result<(String, String), Error>>>();

    map_err_debug(
        crossbeam::thread::scope(|s: &crossbeam::thread::Scope<'_>| {
            // Spawn a thread per github token
            for t in tokens {
                let my_tx = tx.clone();
                let keyword_files = &keyword_files;
                let word_counter = &word_counter;
                let iter = &iter;
                let previous_results = &previous_results;
                s.spawn(move |_| {
                    // The main loop of the thread.
                    // Download the repositories until the iterator is empty.
                    loop {
                        // Lock the repository iterator and retrieve the next item.
                        let next_item = {
                            let mut iter_guard = iter.lock().unwrap();
                            iter_guard.next()
                        };

                        match next_item {
                            Some(row) => {
                                match row {
                                    Ok((row_nr, id, full_name, last_commit)) => {
                                        // Check if the project has already been downloaded.
                                        // If not, download it and send the information back to the main thread.

                                        let project_path: String = match last_commit {
                                            Some(commit) => format!(
                                                "{}/{}/{}-{}",
                                                target,
                                                row_nr / MAX_SUBDIRS,
                                                id,
                                                commit
                                            ),
                                            None => full_name.to_string(),
                                        };

                                        if !previous_results.contains(&id)
                                            && (!skip || Path::new(&project_path).exists())
                                        {
                                            match download_repo(
                                                &t,
                                                id,
                                                &project_path,
                                                full_name,
                                                last_commit,
                                                keyword_files,
                                                word_counter,
                                                skip,
                                                !count,
                                            ) {
                                                Ok(r) => {
                                                    let _ = my_tx.send(Some(Ok(r)));
                                                }
                                                Err(e) => {
                                                    let _ = my_tx.send(Some(Err(e)));
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                    Err(row_nr) => {
                                        let _ = my_tx.send(Some(
                                            Error::new(&format!("Could not parse row {}", row_nr))
                                                .to_res(),
                                        ));
                                    }
                                }
                            }
                            None => {
                                // When the iterator is empty, sends a None message to the main thread to signal the end of the thread.
                                let _ = my_tx.send(None);
                                break;
                            }
                        }
                    }
                });
            }

            let mut ended_threads: usize = 0;

            let progress = ProgressBar::new(n_proj as u64);
            progress.set_style(
                indicatif::ProgressStyle::default_bar()
                    .template("{elapsed} {wide_bar} {percent}%")
                    .unwrap(),
            );
            progress.inc(previous_results.len() as u64);

            // Writes received messages to the log file.
            // The order is therefore non-deterministic although the list of projects is.
            while let Ok(msg) = rx.recv() {
                match msg {
                    Some(Ok((project_msg, files_msg))) => {
                        writeln!(&mut project_log_file, "{}", project_msg).unwrap();
                        if !files_msg.trim().is_empty() {
                            write!(&mut file_log, "{}", files_msg).unwrap();
                        }
                        progress.inc(1);
                    }
                    Some(Err(e)) => e.chain("Error in child thread").to_res()?,
                    None => {
                        // When a None message is received, the sender thread is considered finished.
                        // When all threads are finished, the main thread can exit.
                        ended_threads += 1;
                        if ended_threads == n {
                            break;
                        }
                    }
                }
            }
            progress.finish();
            Ok(())
        }),
        "Error in thread",
    )?
}

/// Downloads a GitHub repository and filters the files according to the provided extensions and keywords.
/// Specifically, the following steps are executed:
/// * Download the repository as a zip archive. (If the skip flag is set, this step is skipped).
/// * Unzip the archive. (If the skip flag is set, this step is skipped).
/// * Remove the zip archive. (If the skip flag is set, this step is skipped).
/// * Remove all files that do not end with one of the provided extensions. (If delete is false, this step is skipped).
/// * Remove all symbolic links. (If delete is false, this step is skipped).
/// * Counts the number of files, lines of code and words in the directory.
/// * Remove all files that do not contain one of the provided keywords. (If delete is false, this step is skipped).
/// * Counts (again) the number of files and lines of code in the directory.
/// * Collect information on every file kept.
/// * Remove all empty directories. (If delete is false, this step is skipped)
///
///
/// # Arguments
///
/// * `token` - The GitHub token to use for the request.
/// * `id` - The id of the project.
/// * `project_path` - The path to the directory where the repository is/will be downloaded.
/// * `full_name` - The full name of the project.
/// * `last_commit` - The hash of the last commit of the project.
/// * `filename` - The name of the directory where the repository will be downloaded.
/// * `matchers` - A map from file extensions to matchers for searching keywords.
/// * `word_counter` - A matcher for counting words in a file.
/// * `skip` - If true, skip the downloading and the filtering of the repositories and only log the files (not the projects).
///
/// # Returns
///
/// A tuple which first entry contains:
///     * The id of the project.
///     * The path to the directory where the repository is/will be downloaded.
///     * The full name of the project.
///     * The hash of the last commit.
///     * The number of files before filtering by keyword.
///     * The total number of lines of code before filtering by keyword.
///     * The total number of words before filtering by keyword.
///     * The number of files after filtering by keyword.
///     * The total number of lines of code after filtering by keyword.
///     * The total number of words after filtering by keyword.
/// and which second entry contains a list of lines (one per file kept) with the following information:
///    * The path to the file.
///    * The extension of the file.
///    * The number of lines of code in the file.
///    * The number of words in the file.
///    * The number of keywords found in the file.
///
/// # Panics
///
/// * If one of the shell commands fails.
///
/// # Example
///
/// To keep files written in C, Java or TypeScript that feature floating point types the following map can be used for `keywords`:
/// ```json
/// {
///     "c": "float|double",
///     "java": "float|double",
///     "ts": "number"
/// }
/// ```
///
fn download_repo(
    token: &str,
    id: u32,
    project_path: &str,
    full_name: &str,
    last_commit: Option<&str>,
    keywords_files: &KeywordFiles,
    word_counter: &Matcher,
    skip: bool,
    delete: bool,
) -> Result<(String, String), Error> {
    if !skip {
        let http_client = map_err(
            reqwest::blocking::Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .timeout(None)
                .pool_idle_timeout(Duration::from_secs(90))
                .build(),
            "Failed to build HTTP client",
        )?;
        let mut headers = HeaderMap::new();

        headers.insert(
            AUTHORIZATION,
            map_err(
                HeaderValue::from_str(&format!("Bearer {}", token)),
                "Invalid header format for HTTP request",
            )?,
        );

        headers.insert(USER_AGENT, HeaderValue::from_static("Scyros"));

        let url_str: String = format!(
            "https://api.github.com/repositories/{}/zipball/{}",
            id,
            ok_or_else(
                last_commit,
                &format!(
                    "Last commit not found for project {} (id: {})",
                    full_name, id
                )
            )?
        );

        let url: reqwest::Url =
            map_err(reqwest::Url::parse(&url_str), &format!("Bad URL {url_str}"))?;

        let mut response_res: Result<Response, Error> =
            Error::new("Did not send request yet").to_res();
        const MAX_RETRIES: usize = 5;
        let mut attempts: usize = 0;

        fn retry_delay(attempt: usize) -> Duration {
            // exp backoff: 250ms, 500ms, 1s, 2s, 4s ...
            let base_ms: u64 = 250u64.saturating_mul(1u64 << attempt.min(MAX_RETRIES));
            Duration::from_millis(base_ms)
        }

        while attempts < MAX_RETRIES && response_res.is_err() {
            attempts += 1;
            response_res = map_err(
                http_client.get(url.clone()).headers(headers.clone()).send(),
                &format!(
                    "Could not download repository {} (id: {}), error while sending HTTP request",
                    full_name, id
                ),
            );
            if response_res.is_err() {
                if attempts < MAX_RETRIES {
                    // Wait before retrying
                    sleep(retry_delay(attempts));
                } else {
                    response_res = Error::new(&format!(
                        "Could not download repository {} (id: {}), maximum number of retries reached",
                        full_name, id
                    ))
                    .to_res();
                }
            }
        }

        let mut response = response_res?;

        if !response.status().is_success() {
            return Ok((
                error_row(id, full_name, last_commit, keywords_files.len()),
                String::new(),
            ));
        }

        // Create output file
        let mut out: File = open_file(&format!("{}.zip", project_path), FileMode::Overwrite)?;

        // Stream response to file
        match copy(&mut response, &mut out) {
            Ok(_) => (),
            Err(_) => {
                return Ok((
                    error_row(id, full_name, last_commit, keywords_files.len()),
                    String::new(),
                ));
            }
        }

        map_err(
            zip_extract(
                &format!("{}.zip", project_path).into(),
                &Path::new(project_path).to_path_buf(),
            ),
            &format!("Failed to extract archive to {}", project_path),
        )?;

        delete_file(format!("{}.zip", project_path), true)?;
    }

    if delete {
        ShellCommand::Find {
            builder: FindCommand::new(project_path)
                .file_type(FileType::File)
                .not()
                .file_extensions(keywords_files.extensions_to_language.keys())
                .delete(),
        }
        .run();

        ShellCommand::Find {
            builder: FindCommand::new(project_path)
                .file_type(FileType::SymbolicLink)
                .delete(),
        }
        .run();
    }

    let mut dir_loc_before_filter: usize = 0;
    let mut dir_files_before_filter: usize = 0;
    let mut dir_words_before_filter: usize = 0;

    let mut files_output: String = String::new();
    let mut dir_loc_after_filter_any: usize = 0;
    let mut dir_loc_after_filter: Vec<usize> = vec![0; keywords_files.len()];
    let mut dir_files_after_filter_any: usize = 0;
    let mut dir_files_after_filter: Vec<usize> = vec![0; keywords_files.len()];
    let mut dir_words_after_filter_any: usize = 0;
    let mut dir_words_after_filter: Vec<usize> = vec![0; keywords_files.len()];
    let mut dir_matches: Vec<usize> = vec![0; keywords_files.len()];

    // Remove all files that do not contain the keywords.
    // Repeat the process for every extension.
    for (ext, lang) in keywords_files.extensions_to_language.iter() {
        let file_list = ShellCommand::Find {
            builder: FindCommand::new(project_path)
                .file_type(FileType::File)
                .file_extension(ext),
        }
        .run();

        for path in file_list.lines() {
            match &load_file(path, 1024 * 1024 * 1024) {
                Ok(file) => {
                    let words = match file {
                        Ok(content) => word_counter.count_matches_in_text(content),
                        Err(_) => word_counter.count_matches_in_file(path)?,
                    };

                    let loc = match file {
                        Ok(content) => content.lines().count(),
                        Err(_) => file_lines_count(path)?,
                    };

                    let matches: Vec<usize> = match file {
                        Ok(content) => keywords_files.count_matches_in_text(lang, content),
                        Err(_) => keywords_files.count_matches_in_file(lang, path)?,
                    };

                    dir_files_before_filter += 1;
                    dir_loc_before_filter += loc;
                    dir_words_before_filter += words;

                    if matches.iter().any(|m| m > &0) {
                        dir_files_after_filter_any += 1;
                        dir_loc_after_filter_any += loc;
                        dir_words_after_filter_any += words;

                        for i in 0..keywords_files.len() {
                            if matches[i] > 0 {
                                dir_files_after_filter[i] += 1;
                                dir_loc_after_filter[i] += loc;
                                dir_words_after_filter[i] += words;
                            }
                        }

                        for (i, match_count) in matches.iter().enumerate() {
                            dir_matches[i] += match_count;
                        }

                        // Remove commas from the filename to avoid issues with the CSV format.
                        map_err(
                            writeln!(
                                &mut files_output,
                                "{},{},{},{},{},{}",
                                id,
                                path.replace(",", "-was_comma-")
                                    .replace("\"", "-was_quote-"),
                                lang,
                                loc,
                                words,
                                matches
                                    .iter()
                                    .map(|m| m.to_string())
                                    .collect::<Vec<String>>()
                                    .join(",")
                            ),
                            "Could not write to builder",
                        )?;
                    } else if delete {
                        delete_file(path, false)?
                    }
                }
                // When the file contains non-ASCII characters, opening it fails.
                // In this case, we ignore the file and continue.
                Err(_) => (),
            }
        }
    }

    if delete {
        ShellCommand::Find {
            builder: FindCommand::new(project_path)
                .file_type(FileType::Directory)
                .empty()
                .delete(),
        }
        .run();
    }

    let project_output = format!(
        "{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
        id,
        project_path,
        full_name,
        last_commit.unwrap_or_default(),
        dir_files_before_filter,
        dir_loc_before_filter,
        dir_words_before_filter,
        dir_files_after_filter_any,
        dir_files_after_filter
            .iter()
            .map(|m| m.to_string())
            .collect::<Vec<String>>()
            .join(","),
        dir_loc_after_filter_any,
        dir_loc_after_filter
            .iter()
            .map(|m| m.to_string())
            .collect::<Vec<String>>()
            .join(","),
        dir_words_after_filter_any,
        dir_words_after_filter
            .iter()
            .map(|m| m.to_string())
            .collect::<Vec<String>>()
            .join(","),
        dir_matches
            .iter()
            .map(|m| m.to_string())
            .collect::<Vec<String>>()
            .join(",")
    );

    Ok((project_output, files_output))
}

fn error_row(id: u32, full_name: &str, last_commit: Option<&str>, n_kw_files: usize) -> String {
    format!(
        "{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
        id,
        "error",
        full_name,
        last_commit.unwrap_or_default(),
        0,
        0,
        0,
        0,
        vec![0; n_kw_files]
            .iter()
            .map(|m| m.to_string())
            .collect::<Vec<String>>()
            .join(","),
        0,
        vec![0; n_kw_files]
            .iter()
            .map(|m| m.to_string())
            .collect::<Vec<String>>()
            .join(","),
        0,
        vec![0; n_kw_files]
            .iter()
            .map(|m| m.to_string())
            .collect::<Vec<String>>()
            .join(","),
        vec![0; n_kw_files]
            .iter()
            .map(|m| m.to_string())
            .collect::<Vec<String>>()
            .join(",")
    )
}

#[cfg(test)]
mod tests {

    use super::*;

    const TEST_DATA: &str = "tests/data/phases/download";

    fn download_test(
        input: &str,
        target: Option<&str>,
        keywords_files: &[&str],
        count: bool,
        skip: bool,
    ) {
        let input_file: String = format!("{}/{}", TEST_DATA, input);
        let output_file_project: String = format!("{}.project_log.csv", input_file);
        let output_file_file: String = format!("{}.file_log.csv", input_file);
        assert!(std::path::Path::new(&input_file).exists());

        // Remove the output files if they exist.
        assert!(delete_file(&output_file_file, true).is_ok());
        assert!(delete_file(&output_file_project, true).is_ok());

        let target_def: String = match target {
            Some(t) => format!("target/tests/{}", t),
            None => String::new(),
        };

        // Remove the target directory if it exists.
        if target.is_some() {
            assert!(delete_dir(&target_def, true).is_ok());
        }

        let tokens_file: String = "ghtokens.csv".to_string();

        for keywords_file in keywords_files {
            assert!(std::path::Path::new(keywords_file).exists());
        }

        let run_downloader = run(
            &input_file,
            None,
            None,
            &target_def,
            &tokens_file,
            keywords_files,
            skip,
            count,
            false,
            0,
            &mut Logger::new(),
            2,
        );

        assert!(run_downloader.is_ok());

        assert_eq!(
            CSVFile::new(&output_file_project, FileMode::Read)
                .unwrap()
                .indexed_lines::<String>(0)
                .unwrap(),
            CSVFile::new(
                &format!("{}/{}.project_log.csv.expected", TEST_DATA, input),
                FileMode::Read
            )
            .unwrap()
            .indexed_lines(0)
            .unwrap()
        );

        assert!(delete_file(&output_file_file, false).is_ok());
        assert!(delete_file(&output_file_project, false).is_ok());
    }

    #[test]
    fn download_java_scala_float_double() {
        download_test(
            "to_download.csv",
            Some("java_scala_float_double"),
            &[
                "tests/data/keywords/java_float.json",
                "tests/data/keywords/scala_float.json",
            ],
            false,
            false,
        )
    }

    #[test]
    fn download_float_local() {
        download_test(
            "to_download_local.csv",
            None,
            &[
                "tests/data/keywords/fp_types.json",
                "tests/data/keywords/fp_transcendental.json",
                "tests/data/keywords/fp_others.json",
                "tests/data/keywords/std_math.json",
            ],
            true,
            true,
        )
    }
}
