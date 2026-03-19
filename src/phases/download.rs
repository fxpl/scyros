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

#![doc = include_str!("../docs/download.md")]

use crate::utils::logger::Logger;
use anyhow::{anyhow, Context, Result};
use clap::{Arg, ArgAction, Command};
use indicatif::ProgressBar;
use polars::frame::DataFrame;
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
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::thread::sleep;
use std::time::Duration;
use tracing::{debug, info};
use walkdir::WalkDir;
use zip_extensions::zip_extract::zip_extract;

use crate::utils::csv::*;
use crate::utils::fs::*;
use crate::utils::regex::*;

/// Command line arguments parsing.
pub fn cli() -> Command {
    Command::new("download")
        .about("Downloads all github repositories from a list and keeps only the files that satisfy user defined criteria.")
        .long_about(include_str!("../docs/download.md"))
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
                .conflicts_with("skip")
                .required_unless_present("skip"),
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
                        {\n\
                            \"languages\": [\n\
                                {\n\
                                \"name\": \"LanguageName\",\n\
                                \"extensions\": [\".ext1\", \".ext2\", ...],\n\
                                \"keywords\": [\"localKeyword1\", \"localKeyword2\", ...]    // optional\n\
                                },\n\
                                ...\n\
                            ],\n\
                            \"keywords\": [\"globalKeyword1\", \"globalKeyword2\", ...]      // optional\n\
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
            Arg::new("order")
                .long("order")
                .help("Order in which the projects are processed.")
                .value_parser(["random", "sequential"])
                .default_value("random")
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

/// Entry point of the program
///
/// # Arguments
///
/// * `input_file_path` - Path to the input csv file to use.
/// * `projects_output_path` - Path to the output csv file storing the project statistics. If not specified, the input file name will be used with ".project_log.csv" appended.
/// * `files_output_path` - Path to the output csv file storing the file statistics. If not specified, the input file name will be used with ".file_log.csv" appended.
/// * `target` - Path to the directory where projects will be downloaded.
/// * `tokens_file` - Path to the file containing the GitHub tokens to use.
/// * `keywords_file_paths` - Path to the files containing the list of extensions and keywords to use.
/// * `skip` - If true, skip the downloading of the repositories.
/// * `count` - If true, compute statistics on the downloaded projects without deleting any file.
/// * `overwrite` - If true, overwrite the log files if they exist.
/// * `seed` - The seed used to shuffle the projects.
/// * `logger` - The logger to use to display information about the progress of the program.
/// * `thread` - The number of threads to use when not downloading and computing statistic locally instead.
/// * `order` - The order in which the projects are processed.
pub fn run(
    input_file_path: &str,
    projects_output_path: Option<&str>,
    files_output_path: Option<&str>,
    target: &str,
    tokens_file: Option<&str>,
    keywords_file_paths: &[&str],
    skip: bool,
    count: bool,
    overwrite: bool,
    seed: u64,
    logger: &Logger,
    thread: usize,
    order: &str,
) -> Result<()> {
    // Check if the token file is valid and load the tokens.
    let tokens: Vec<String> = if skip {
        (0..thread).map(|n| n.to_string()).collect()
    } else {
        logger.log_tokens(tokens_file.unwrap())? // safe unwrap
    };

    let input_file: DataFrame = logger.run_task("Loading input file", || {
        open_csv(
            input_file_path,
            Some(Schema::from_iter(vec![
                Field::new("id".into(), DataType::UInt32),
                Field::new("name".into(), DataType::String),
                Field::new("path".into(), DataType::String),
                Field::new("latest_commit".into(), DataType::String),
            ])),
            Some(if skip {
                vec!["path"]
            } else {
                vec!["id", "name", "latest_commit"]
            }),
        )
    })?;

    let mut shuffled_idx: Vec<usize> = (0..input_file.height()).collect::<Vec<usize>>();

    if order == "random" {
        // Load the ids from the input file in random order.
        logger.run_task("Loading project IDs in random order", || {
            let mut rng: StdRng = SeedableRng::seed_from_u64(seed);
            shuffled_idx.shuffle(&mut rng);
            Ok(())
        })?;
    }

    let shuffled_rows = shuffled_idx.into_iter().map(|idx| {
        let row = input_file.get_row(idx).unwrap().0;

        if skip {
            match row[0].clone() {
                AnyValue::String(path) => Ok((idx, None, path, None)),
                _ => Err(idx),
            }
        } else {
            match (row[0].clone(), row[1].clone(), row[2].clone()) {
                (AnyValue::UInt32(id), AnyValue::String(name), AnyValue::String(latest_commit)) => {
                    Ok((idx, Some(id), name, Some(latest_commit)))
                }
                _ => Err(idx),
            }
        }
    });

    let n_proj = input_file.height();
    info!("  {} projects found.", n_proj);

    const MAX_SUBDIRS: usize = 30000;

    if !skip {
        // Create the target directory if it does not exist.
        create_dir(target)?;

        // Create subsubdirectories to avoid reaching the limit of 32k subdirectories on some filesystems.
        for i in 0..(n_proj / MAX_SUBDIRS + 1) {
            create_dir(format!("{target}/{i}"))?;
        }
    }

    // Open the log file for the projects or create it if it does not exist.
    let default_project_log_path = format!("{input_file_path}.project_log.csv");
    let project_log_path: &str = projects_output_path.unwrap_or(&default_project_log_path);

    // Load previous results if the skip flag is not set.

    let previous_results: HashSet<(Option<u32>, Option<String>)> =
        logger.run_task("Resuming progress", || {
            Ok(if overwrite || !Path::new(&project_log_path).exists() {
                HashSet::<(Option<u32>, Option<String>)>::new()
            } else {
                let project_log_file: CSVFile = CSVFile::new(project_log_path, FileMode::Read)?;
                let prev_res: HashSet<(Option<u32>, Option<String>)> = if skip {
                    project_log_file
                        .column::<String>(0)?
                        .into_iter()
                        .map(|s| (None, Some(s)))
                        .collect()
                } else {
                    project_log_file
                        .column::<u32>(0)?
                        .into_iter()
                        .map(|id| (Some(id), None))
                        .collect()
                };
                prev_res
            })
        })?;

    if previous_results.is_empty() {
        info!("  No previously downloaded projects found, starting from scratch.",);
    } else {
        info!(
            "  {} projects have already been downloaded",
            previous_results.len()
        )
    }

    let keyword_files: KeywordFiles = logger.run_task("Loading keywords", || {
        KeywordFiles::new().add_files(keywords_file_paths, true)
    })?;

    info!(
        "  {} languages found in {} keyword files.",
        keyword_files.languages().len(),
        keyword_files.len()
    );
    debug!("  Languages: {}", keyword_files.languages().join(", "));
    debug!(
        "  File extensions: {}",
        keyword_files.extensions().join(", ")
    );
    debug!(
        "  Regexes: {}",
        keyword_files
            .debug_regexes()
            .into_iter()
            .map(|(lang, regexes)| format!("{}:\n{}", lang, regexes.join("\n")))
            .collect::<Vec<_>>()
            .join("\n")
    );

    let files_with_kw_headers: String = keyword_files
        .paths
        .iter()
        .map(|p| format!("files_with_{p}"))
        .collect::<Vec<String>>()
        .join(",");
    let loc_of_files_with_kw_headers: String = keyword_files
        .paths
        .iter()
        .map(|p| format!("loc_of_files_with_{p}"))
        .collect::<Vec<String>>()
        .join(",");
    let words_of_files_with_kw_headers: String = keyword_files
        .paths
        .iter()
        .map(|p| format!("words_of_files_with_{p}"))
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
    let project_log_headers: Vec<&str> = if skip {
        [
            "path",
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
        ]
        .to_vec()
    } else {
        [
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
        ]
        .to_vec()
    };

    project_log_file.write_header(&project_log_headers)?;

    // Open the log file for the files or create it if it does not exist.
    // If the overwrite flag is set, the file is generated anew.
    let default_file_log_path = format!("{input_file_path}.file_log.csv");
    let file_log_path: &str = files_output_path.unwrap_or(&default_file_log_path);
    let mut file_log = CSVFile::new(
        file_log_path,
        if overwrite {
            FileMode::Overwrite
        } else {
            FileMode::Append
        },
    )?;

    let file_log_headers: Vec<&str> = if skip {
        ["path", "language", "loc", "words", &keyword_match_headers].to_vec()
    } else {
        [
            "id",
            "name",
            "language",
            "loc",
            "words",
            &keyword_match_headers,
        ]
        .to_vec()
    };

    file_log.write_header(&file_log_headers)?;

    // Iterate over the projects and collect metadata.
    let iter = Mutex::new(shuffled_rows);

    info!("Starting download...");

    // Numbers of threads to be spawned.
    let n = tokens.len();
    debug!("Spawning {n} threads for downloading and processing the repositories.");

    // Every thread comes with a sender channel.
    // The sender channel is used to send information about the downloaded repository back to the main thread.
    // The receiver channel is used by the main thread to collect and write the information to the log file.
    let (tx, rx) = crossbeam_channel::unbounded::<Option<Result<(String, String)>>>();
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
                        let mut iter_guard = iter.lock().expect("Mutex poisoned");
                        iter_guard.next()
                    };

                    match next_item {
                        Some(row) => {
                            match row {
                                Ok((row_nr, id_opt, full_name, last_commit)) => {
                                    // Check if the project has already been downloaded.
                                    // If not, download it and send the information back to the main thread.

                                    let project_path: String = match (last_commit, id_opt) {
                                        (Some(commit), Some(id)) => format!(
                                            "{}/{}/{}-{}",
                                            target,
                                            row_nr / MAX_SUBDIRS,
                                            id,
                                            commit
                                        ),
                                        (None, None) => full_name.to_string(),
                                        _ => unreachable!(),
                                    };

                                    let path_opt = if skip {
                                        Some(project_path.clone())
                                    } else {
                                        id_opt.map(|id| id.to_string())
                                    };

                                    if (!skip || Path::new(&project_path).exists())
                                        && !previous_results.contains(&(id_opt, path_opt))
                                    {
                                        match download_repo(
                                            t.as_str(),
                                            id_opt,
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
                                    let _ = my_tx
                                        .send(Some(Err(anyhow!("Could not parse row {row_nr}"))));
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
                anyhow::Ok(())
            });
        }

        let mut ended_threads: usize = 0;

        let progress = ProgressBar::new(n_proj as u64);
        progress.set_style(
            indicatif::ProgressStyle::default_bar().template("{elapsed} {wide_bar} {percent}%")?,
        );
        progress.inc(previous_results.len() as u64);

        // Writes received messages to the log file.
        // The order is therefore non-deterministic although the list of projects is.
        while let Ok(msg) = rx.recv() {
            match msg {
                Some(msg_content) => {
                    let (project_msg, files_msg) = msg_content?;

                    writeln!(&mut project_log_file, "{project_msg}")?;
                    if !files_msg.trim().is_empty() {
                        write!(&mut file_log, "{files_msg}")?;
                    }
                    progress.inc(1);
                }
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
    })
    .map_err(|e| anyhow!("Thread panicked: {e:?}"))?
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
/// * `id_opt` - The id of the project, if the project is downloaded from GitHub.
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
    id_opt: Option<u32>,
    project_path: &str,
    full_name: &str,
    last_commit: Option<&str>,
    keywords_files: &KeywordFiles,
    word_counter: &Matcher,
    skip: bool,
    delete: bool,
) -> Result<(String, String)> {
    if !skip {
        let id = id_opt.with_context(|| {
            format!(
                "Project {} does not have an id, cannot be downloaded",
                full_name
            )
        })?;
        let http_client = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(None)
            .pool_idle_timeout(Duration::from_secs(90))
            .build()?;
        let mut headers = HeaderMap::new();

        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}"))?,
        );

        headers.insert(USER_AGENT, HeaderValue::from_static("Scyros"));

        let url_str: String = format!(
            "https://api.github.com/repositories/{}/zipball/{}",
            id,
            last_commit.with_context(|| format!(
                "Last commit not found for project {full_name} (id: {id})"
            ))?
        );

        let url: reqwest::Url =
            reqwest::Url::parse(&url_str).with_context(|| format!("Bad URL {url_str}"))?;

        let mut response_res: Result<Response> = Err(anyhow!("Did not send request yet"));
        const MAX_RETRIES: usize = 5;
        let mut attempts: usize = 0;

        fn retry_delay(attempt: usize) -> Duration {
            // exp backoff: 250ms, 500ms, 1s, 2s, 4s ...
            let base_ms: u64 = 250u64.saturating_mul(1u64 << attempt.min(MAX_RETRIES));
            Duration::from_millis(base_ms)
        }

        while attempts < MAX_RETRIES && response_res.is_err() {
            attempts += 1;
            response_res = http_client
                .get(url.clone())
                .headers(headers.clone())
                .send()
                .with_context(|| {
                    format!(
                    "Could not download repository {full_name} (id: {id}), error while sending HTTP request"
                )
                });
            if response_res.is_err() {
                if attempts < MAX_RETRIES {
                    // Wait before retrying
                    sleep(retry_delay(attempts));
                } else {
                    response_res = Err(anyhow!(
                        "Could not download repository {full_name} (id: {id}), maximum number of retries reached"
                    ));
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
        let mut out: File = open_file(format!("{project_path}.zip"), FileMode::Overwrite)?;

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

        zip_extract(
            &format!("{project_path}.zip").into(),
            &Path::new(project_path).to_path_buf(),
        )
        .with_context(|| format!("Failed to extract archive to {project_path}"))?;

        delete_file(format!("{project_path}.zip"), true)?;
    }

    if delete {
        for entry in WalkDir::new(project_path)
            .contents_first(true)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file())
            .filter(|e| {
                let ext = e.path().extension().and_then(|s| s.to_str());
                !matches!(ext, Some(ext) if keywords_files.extensions_to_language.contains_key(ext))
            })
        {
            delete_file(entry.path(), false)?;
        }
        // Delete symbolic links
        for entry in WalkDir::new(project_path)
            .contents_first(true)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_symlink())
        {
            let path = entry.path();

            if path.is_dir() {
                delete_dir(path, false)?;
            } else {
                delete_file(path, false)?;
            }
        }
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
        let file_list: Vec<PathBuf> = WalkDir::new(project_path)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file())
            .filter(|e| {
                let path = e.path();
                path.extension().is_some() && path.to_str().is_some_and(|s| s.ends_with(ext))
            })
            .map(|e| e.into_path())
            .collect();

        for path in file_list {
            if let Ok(file) = &load_file(&path, 1024 * 1024 * 1024) {
                let words = match file {
                    Ok(content) => word_counter.count_matches_in_text(content),
                    Err(_) => word_counter.count_matches_in_file(&path)?,
                };

                let loc = match file {
                    Ok(content) => content.lines().count(),
                    Err(_) => file_lines_count(&path)?,
                };

                let matches: Vec<usize> = match file {
                    Ok(content) => keywords_files.count_matches_in_text(lang, content),
                    Err(_) => keywords_files.count_matches_in_file(lang, &path)?,
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

                    let path_str = &path
                        .to_str()
                        .with_context(|| {
                            format!("Could not convert path to string: {}", &path.display())
                        })?
                        .replace(",", "-was_comma-")
                        .replace("\"", "-was_quote-");
                    writeln!(
                        &mut files_output,
                        "{}{},{},{},{},{}",
                        id_opt.map_or_else(String::new, |i| format!("{},", i)),
                        path_str,
                        lang,
                        loc,
                        words,
                        matches
                            .iter()
                            .map(|m| m.to_string())
                            .collect::<Vec<String>>()
                            .join(",")
                    )?;
                } else if delete {
                    delete_file(&path, false)?
                }
            }
        }
    }

    if delete {
        delete_empty_dirs(project_path)?
    }

    let project_output = format!(
        "{}{},{}{}{},{},{},{},{},{},{},{},{},{}",
        id_opt.map_or_else(String::new, |i| format!("{i},")),
        project_path,
        if skip {
            "".to_string()
        } else {
            format!("{full_name},")
        },
        if skip {
            "".to_string()
        } else {
            let last_commit = last_commit
                .with_context(|| format!("Last commit not found for project {full_name}"))?;
            format!("{last_commit},")
        },
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

    use crate::utils::logger::test_logger;
    use anyhow::ensure;

    use super::*;

    const TEST_DATA: &str = "tests/data/phases/download";

    fn download_test(
        input: &str,
        target: Option<&str>,
        keywords_files: &[&str],
        count: bool,
        skip: bool,
    ) -> Result<()> {
        let input_file: String = format!("{TEST_DATA}/{input}");
        let output_file_project: String = format!("{input_file}.project_log.csv");
        let output_file_file: String = format!("{input_file}.file_log.csv");
        ensure!(
            std::path::Path::new(&input_file).exists(),
            "Input file {input_file} does not exist"
        );

        // Remove the output files if they exist.
        delete_file(&output_file_file, true)?;
        delete_file(&output_file_project, true)?;

        let target_def: String = match target {
            Some(t) => format!("target/tests/{t}"),
            None => String::new(),
        };

        // Remove the target directory if it exists.
        if target.is_some() {
            delete_dir(&target_def, true)?;
        }

        let tokens_file: String = "ghtokens.csv".to_string();

        for keywords_file in keywords_files {
            ensure!(
                std::path::Path::new(keywords_file).exists(),
                "Keywords file {keywords_file} does not exist"
            );
        }

        run(
            &input_file,
            None,
            None,
            &target_def,
            Some(&tokens_file),
            keywords_files,
            skip,
            count,
            false,
            0,
            test_logger(),
            2,
            "random",
        )?;

        assert_eq!(
            CSVFile::new(&output_file_project, FileMode::Read)?.indexed_lines::<String>(0)?,
            CSVFile::new(
                &format!("{TEST_DATA}/{input}.project_log.csv.expected"),
                FileMode::Read
            )?
            .indexed_lines(0)?
        );

        delete_file(&output_file_file, false)?;
        delete_file(&output_file_project, false)
    }

    #[test]
    fn download_java_scala_float_double() -> Result<()> {
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
    fn download_float_local() -> Result<()> {
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

    #[test]
    fn download_local() -> Result<()> {
        download_test(
            "to_download_local_c.csv",
            None,
            &["tests/data/keywords/c.json"],
            true,
            true,
        )
    }
}
