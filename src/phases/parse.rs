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

#![doc = include_str!("../docs/parse.md")]
use clap::ArgAction;
use clap::{Arg, Command};
use indicatif::ProgressBar;
use polars::prelude::*;
use rand::rngs::StdRng;
use rand::seq::SliceRandom as _;
use rand::SeedableRng;

use anyhow::{anyhow, bail, ensure, Context, Result};
use std::iter::FromIterator as _;
use std::sync::atomic::{AtomicBool, Ordering};
use std::vec;
use std::{collections::HashSet, path::Path, sync::Mutex};
use tracing::{debug, info, warn};
use tree_sitter::{Language, Node, Parser, Tree};

use crate::utils::dataframes::{has_column, require_columns};
use crate::utils::error::{error_cell, ErrorCode, ErrorSummary, RowError, ERROR};
use crate::utils::fs::*;
use crate::utils::regex::*;
use crate::utils::repo::{Function, RepoColumn, RepoFile, Unit, FILE, LOCAL_PATH};
use crate::utils::{
    csv::*,
    logger::{log_output_file, log_seed, Logger},
};

use super::rename_arg;

/// Command line arguments parsing.
pub fn cli() -> Command {
    Command::new("parse")
        .about("Parse all the files in the dataset and extract functions whose body contains one of the provided keywords.")
        .long_about(include_str!("../docs/parse.md"))
        .disable_version_flag(true)
        .arg(
            Arg::new("input")
                .short('i')
                .long("input")
                .value_name("INPUT_FILE.csv")
                .help("Path to the input csv file to use. It must be a valid CSV file with a column 'local_path' containing the paths to the files \
                       and a column 'file' containing their paths inside their repositories, such as the file log of download. \
                       The repository columns are copied to the output files. The language of a file is given by its extension. Other columns are ignored.")
                .required(true)
        )
        .arg(
            Arg::new("output")
                .short('o')
                .long("output")
                .value_name("OUTPUT_FILE.csv")
                .help("Path to the output csv file storing the functions statistics.")
                .required(false),
        )
        .arg(
            Arg::new("logs")
                .short('l')
                .long("logs")
                .value_name("LOGS_FOLDER")
                .help("Path to the folder where the logs are stored. The default is the current folder.")
                .required(false),
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
            Arg::new("regex")
                .long("regex")
                .help("Whether to interpret the keywords as regular expressions. If not specified, the keywords are interpreted as whole words to match.")
                .default_value("false")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("case-sensitive")
                .long("case-sensitive")
                .help("Match the keywords case-sensitively. By default, letter case is ignored when matching keywords. File extensions are always case-sensitive.")
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
            Arg::new("threads")
                .short('n')
                .long("threads")
                .value_name("THREADS")
                .help("Number of threads to use.")
                .default_value("1")
                .value_parser(clap::builder::RangedU64ValueParser::<usize>::new().range(1..))
        )
        .arg(
            Arg::new("seed")
                .short('s')
                .long("seed")
                .value_name("SEED")
                .help("Seed used to randomly shuffle the input file.")
                .default_value("8155495201244430235")
                .value_parser(clap::value_parser!(u64)),
        )
        .arg(
            Arg::new("failures")
            .long("failures")
            .value_name("POLICY")
            .help("Failure policy when a file or a function has a parsing error.\n\
            ignore: continue parsing\n\
            skip-file: replace the file statistics with an error row in the output file, does not extract any function from the file\n\
            skip-function: replace the function statistics with an error row in the output file\n\
            abort: stop the program")
            .default_value("ignore")
            .value_parser(["ignore", "skip-file", "skip-function", "abort"]),
        )
        .arg(
            Arg::new("ignore-comments")
            .long("ignore-comments")
            .help("Whether to ignore comments when extracting functions, in addition to ignoring them during keyword matching.")
            .default_value("false")
            .action(ArgAction::SetTrue)
            .conflicts_with("count"),
        )
        .arg(
            Arg::new("lambdas")
            .long("lambdas")
            .help("Whether to extract lambda functions as well. By default, only named functions are extracted.")
            .default_value("false")
            .action(ArgAction::SetTrue)
        )
        .arg(
            Arg::new("count")
                .long("count")
                .alias("no-output")
                .help("Compute statistics on the functions without writing the functions to files.") 
                .action(ArgAction::SetTrue)
        )
        .arg(
            Arg::new("infer-source")
                .long("infer-source")
                .help("Infer the source of the repositories without one: github if they have an id, local otherwise. \
                       Without this flag, a file without a source is an error.")
                .action(ArgAction::SetTrue),
        )
        .arg(rename_arg())
}

type OutputRow = Vec<String>;
type LogRow = Vec<String>;

/// Column with the local path of the extracted code of a function, written when the functions are extracted.
const LOCAL_FUNCTION_PATH: &str = "local_function_path";
/// Column with the line of the first parse error, relative to the function or to the file. Empty without parse error.
const PARSE_ERROR_LINE: &str = "parse_error_line";
/// Column with the column of the first parse error, relative to the function or to the file. Empty without parse error.
const PARSE_ERROR_COLUMN: &str = "parse_error_column";

/// Entry point of the program
///
/// # Arguments
///
/// * `input_path` - Path to the input csv file to use. It has the columns `local_path` and `file`, and the repository columns.
/// * `output_path` - Path to the output csv file storing the functions statistics.
/// * `logs_path` - Path to the output csv file storing the files statistics.
/// * `keywords_file_paths` - Paths to the files containing the list of extensions and keywords to use.
/// * `regex_syntax` - Whether to interpret the keywords as regular expressions. If false, the keywords are interpreted as whole words to match.
/// * `case_sensitive` - Whether keywords are matched case-sensitively.
/// * `opt_languages` - Optional list of languages to parse. If not specified, all supported languages are parsed.
/// * `fail_policy` - The policy to apply when a parse error is encountered. It can be one of the following:
///   * `ignore`: continue parsing and write the statistics of the file or function with parse error as if there was no error.
///   * `skip-file`: replace the file statistics with an error row in the output file, does not extract any function from the file.
///   * `skip-function`: replace the function statistics with an error row in the output file.
/// * `threads` - The number of threads to use.
/// * `seed` - The seed used to shuffle the input file.
/// * `force` - Whether to override the output file if it already exists.
/// * `ignore_comments` - Whether to ignore comments when extracting functions.
/// * `lambdas` - Whether to extract lambda functions as well.
/// * `write_out` - Whether to write the extracted functions to files. If false, only the statistics are computed and written to the output file.
/// * `infer_source` - Whether to infer the source of the repositories without one (see [`Repo::from_csv_row`](crate::utils::repo::Repo::from_csv_row)).
/// * `renames` - Columns of the input read under other names.
/// * `logger` - The logger to use to display information about the progress of the program.
pub fn run(
    input_path: &str,
    output_path: Option<&str>,
    logs_path: Option<&str>,
    keywords_file_paths: &[&str],
    regex_syntax: bool,
    case_sensitive: bool,
    fail_policy: &str,
    threads: usize,
    seed: u64,
    force: bool,
    ignore_comments: bool,
    lambdas: bool,
    write_out: bool,
    infer_source: bool,
    renames: &[(String, String)],
    logger: &Logger,
) -> Result<()> {
    let supported_languages: HashSet<&'static str> = vec![
        "c",
        "c++",
        "c#",
        "java",
        "python",
        "fortran",
        "typescript",
        "go",
        "scala",
        "rust",
    ]
    .into_iter()
    .collect::<HashSet<_>>();

    let keyword_files: KeywordFiles = logger.run_task("Loading keywords", || {
        KeywordFiles::new(regex_syntax)
            .case_sensitive(case_sensitive)
            .add_files(keywords_file_paths, true)
    })?;
    let languages = keyword_files.languages();

    for lang in &languages {
        if !supported_languages.contains(lang.to_lowercase().as_str()) {
            warn!("Unsupported language: {lang}. Its files are skipped.");
        }
    }

    info!("Selected languages: {}", languages.join(", "));

    let default_output_path: String = format!("{input_path}.functions.csv");
    let output_path: &str = output_path.unwrap_or(&default_output_path);
    log_output_file(output_path, false, force)?;

    let default_logs_path: String = format!("{input_path}.function_logs.csv");
    let logs_path: &str = logs_path.unwrap_or(&default_logs_path);

    log_output_file(logs_path, false, force)?;

    let mut input_file = open_csv(
        input_path,
        Some(Schema::from_iter(RepoColumn::fields().into_iter().chain([
            Field::new(LOCAL_PATH.into(), DataType::String),
            Field::new(FILE.into(), DataType::String),
        ]))),
        None,
        renames,
    )?;
    require_columns(&input_file, input_path, &[LOCAL_PATH, FILE])?;
    ensure!(
        infer_source || has_column(&input_file, RepoColumn::Source.name()),
        "{input_path} has no column source. Add it, read another column as source with \
         --rename OLD=source, or infer the sources with --infer-source."
    );

    let n_files_before = input_file.height();

    info!(
        "  {} files found in the input file, filtering by selected languages",
        n_files_before
    );

    // Keep only the files written in the selected languages
    let name_mask: BooleanChunked = input_file
        .column(LOCAL_PATH)?
        .str()?
        .iter()
        .map(|opt_name| {
            opt_name
                .and_then(|s| keyword_files.file_language(s))
                .is_some_and(|lang| supported_languages.contains(lang.to_lowercase().as_str()))
        })
        .collect();
    input_file = input_file.filter(&name_mask)?;

    let n_files = input_file.height();

    info!(
        "  {} files found after filtering ({:.2} %)",
        n_files,
        if n_files_before == 0 {
            0.0
        } else {
            n_files as f64 / n_files_before as f64 * 100.0
        }
    );

    log_seed(seed);

    let mut shuffled_idx = (0..input_file.height()).collect::<Vec<usize>>();

    // Load the ids from the input file in random order.
    logger.run_task("Loading files in random order", || {
        let mut rng: StdRng = SeedableRng::seed_from_u64(seed);
        shuffled_idx.shuffle(&mut rng);
        Ok(())
    })?;

    let shuffled_rows = shuffled_idx
        .into_iter()
        .map(|idx| RepoFile::from_csv_row(&input_file, idx, infer_source));

    let word_counter: Matcher = Matcher::words_matcher();

    // Open the log file for the projects or create it if it does not exist.
    let mut output_file = CSVFile::new(output_path, FileMode::Overwrite)?;

    // Write the header.
    let output_prefix = Function::header()
        .into_iter()
        .chain([ERROR])
        .chain(write_out.then_some(LOCAL_FUNCTION_PATH))
        .chain(["language", "loc", "words"]);
    let output_suffix = [
        "loop_statements",
        "loop_nestings",
        "if_statements",
        "if_nestings",
        "functions_calls",
        "function_calls_nestings",
        "params",
        "param_kw_match",
        "return_kw_match",
        PARSE_ERROR_LINE,
        PARSE_ERROR_COLUMN,
    ];
    let output_header: Vec<String> = output_prefix
        .map(|s| s.to_string())
        .chain(keyword_files.paths.iter().cloned())
        .chain(output_suffix.iter().map(|s| s.to_string()))
        .collect();
    output_file.write_header(&output_header)?;
    let mut function_errors = ErrorSummary::<Function>::new(&output_header)?;

    let mut logs_file = CSVFile::new(logs_path, FileMode::Overwrite)?;

    // Write the header.
    let logs_prefix =
        RepoFile::header()
            .into_iter()
            .chain([ERROR, "language", "functions", "functions_with_kw"]);
    let logs_header: Vec<String> = logs_prefix
        .map(|s| s.to_string())
        .chain(keyword_files.paths.iter().cloned())
        .chain([PARSE_ERROR_LINE, PARSE_ERROR_COLUMN].map(|s| s.to_string()))
        .collect();
    logs_file.write_header(&logs_header)?;
    let mut file_errors = ErrorSummary::<RepoFile>::new(&logs_header)?;

    let iter = Mutex::new(shuffled_rows.into_iter());

    // Set when a thread fails, so that the other threads stop instead of processing the remaining files.
    let failed = AtomicBool::new(false);

    // Every thread comes with a sender channel.
    // The sender channel is used to send information about the extracted functions back to the main thread.
    // The receiver channel is used by the main thread to collect and write the information to the log file.
    let (tx, rx) =
        crossbeam_channel::unbounded::<Option<Result<(Vec<OutputRow>, Option<LogRow>)>>>();
    crossbeam::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|_| {
                let my_tx = tx.clone();
                // The main loop of the thread.
                // Download the repositories until the iterator is empty.
                loop {
                    // Lock the repository iterator and retrieve the next item.
                    let next_item: Option<Result<RepoFile>> = if failed.load(Ordering::Relaxed) {
                        None
                    } else {
                        iter.lock().unwrap().next()
                    };

                    match next_item {
                        Some(row) => match row {
                            Ok(file) => match analyze_file(
                                &file,
                                &keyword_files,
                                fail_policy,
                                ignore_comments,
                                lambdas,
                                write_out,
                                &word_counter,
                            ) {
                                Ok(s) => {
                                    my_tx.send(Some(Ok(s))).unwrap();
                                }
                                Err(e) => {
                                    failed.store(true, Ordering::Relaxed);
                                    my_tx.send(Some(Err(e))).unwrap();
                                    break;
                                }
                            },
                            Err(e) => {
                                failed.store(true, Ordering::Relaxed);
                                let _ = my_tx.send(Some(Err(e)));
                                break;
                            }
                        },
                        None => {
                            // When the iterator is empty, sends a None message to the main thread to signal the end of the thread.
                            my_tx.send(None).unwrap();
                            break;
                        }
                    }
                }
            });
        }

        let mut ended_threads = 0;

        let progress = ProgressBar::new(n_files as u64);
        progress.set_style(
            indicatif::ProgressStyle::default_bar().template("{elapsed} {wide_bar} {percent}%")?,
        );

        // Writes received messages to the log file.
        // The order is therefore non-deterministic although the list of projects is.
        while let Ok(msg) = rx.recv() {
            match msg {
                Some(msg_content) => {
                    let (output_rows, opt_log_row) = msg_content?;
                    for row in output_rows {
                        function_errors.add(&row)?;
                        output_file.write_record(row)?;
                    }
                    if let Some(log_row) = opt_log_row {
                        file_errors.add(&log_row)?;
                        logs_file.write_record(log_row)?;
                    }
                    progress.inc(1);
                }
                None => {
                    // When a None message is received, the sender thread is considered finished.
                    // When all threads are finished, the main thread can exit.
                    ended_threads += 1;
                    if ended_threads == threads {
                        break;
                    }
                }
            }
        }
        progress.finish();
        anyhow::Ok(())
    })
    .map_err(|e| anyhow!("Error in thread pool: {e:?}"))??;
    file_errors.log();
    function_errors.log();
    Ok(())
}

/// Analyze a file and extract the functions whose body contains one of the provided keywords.
/// Returns statistics about the functions.
///
/// # Arguments
///
/// * `file` - The file to analyze.
/// * `keywords_files` - The files containing the list of keywords to search for in the functions.
/// * `fail_policy` - The policy to apply when a parse error is encountered.
/// * `ignore_comments` - Whether to ignore comments when extracting functions, in addition to ignoring them during keyword matching.
/// * `lambdas` - Whether to extract lambda functions as well.
/// * `write_out` - Whether to write the extracted functions to files. If false, only the statistics are computed and written to the output file.
/// * `word_counter` - The matcher to use to count the words in the functions.
/// # Returns
///
/// A string containing the statistics of the functions in the file. Specifically:
/// * The path to the file containing the function.
/// * The number of lines of code.
/// * The number of words.
/// * The number of keywords matched.
/// * The number of loops.
/// * The maximum loop nesting level.
/// * The number of conditional statements.
/// * The maximum conditional nesting level.
///
fn analyze_file(
    file: &RepoFile,
    keywords_files: &KeywordFiles,
    fail_policy: &str,
    ignore_comments: bool,
    lambdas: bool,
    write_out: bool,
    word_counter: &Matcher,
) -> Result<(Vec<OutputRow>, Option<LogRow>)> {
    let path: &str = &file.local_path;
    let language = keywords_files.file_language(path).with_context(|| {
        format!("Could not find the language for file {path} in the keywords files")
    })?;
    let grammar = language_to_grammar(&language)
        .with_context(|| format!("Unsupported language: {language}"))?;
    // Initializes the parser
    let mut parser: Parser = Parser::new();
    parser.set_language(&grammar.lang)?;
    match load_file(path, 1024 * 1024 * 1024)? {
        Ok(source_code) => {
            // Creates a folder to store the functions of the file
            let target_folder: String = format!("{path}.functions");
            if write_out {
                create_dir(&target_folder)?;
            }

            // Parses the source code of the file
            let tree: Tree = parser
                .parse(&source_code, None)
                .with_context(|| format!("Failed to parse file {path}"))?;

            let file_has_parse_error: bool = tree.root_node().has_error();

            if file_has_parse_error && fail_policy == "skip-file" {
                let position: Option<(usize, usize)> = find_first_error_position(&tree.root_node());
                debug!(
                    "File {path}: {}",
                    RowError::new(ErrorCode::ParseError, position_detail(position))
                );
                Ok((
                    Vec::new(),
                    Some(file_error_row(
                        file,
                        ErrorCode::ParseError,
                        &language,
                        keywords_files,
                        position,
                    )),
                ))
            } else if file_has_parse_error && fail_policy == "abort" {
                bail!("Parse error in file {path}")
            } else {
                let root: Node<'_> = tree.root_node();
                let (output, total_functions, functions_with_kw, functions_with_specific_kw) =
                    extract_functions(
                        file,
                        &root,
                        &language,
                        &grammar,
                        &source_code,
                        keywords_files,
                        fail_policy,
                        ignore_comments,
                        lambdas,
                        write_out,
                        word_counter,
                        &mut parser,
                    )?;

                let error_position: Option<(usize, usize)> = if file_has_parse_error {
                    find_first_error_position(&root)
                } else {
                    None
                };

                let log_row: LogRow = file
                    .row()
                    .into_iter()
                    .chain([
                        error_cell(None),
                        language.to_string(),
                        total_functions.to_string(),
                        functions_with_kw.to_string(),
                    ])
                    .chain(functions_with_specific_kw.iter().map(|x| x.to_string()))
                    .chain(position_cells(error_position))
                    .collect();

                Ok((output, Some(log_row)))
            }
        }

        // If the file is too large, return an error row
        Err(size) => {
            debug!(
                "File {path}: {}",
                RowError::new(ErrorCode::TooLarge, format!("{size} bytes"))
            );
            Ok((
                Vec::new(),
                Some(file_error_row(
                    file,
                    ErrorCode::TooLarge,
                    &language,
                    keywords_files,
                    None,
                )),
            ))
        }
    }
}

/// Returns the log row of a file skipped because of a parse error or of its size: the file, its error, its language,
/// empty statistics, and the position of the parse error.
///
/// # Arguments
///
/// * `file` - The file.
/// * `error` - Why the file was skipped.
/// * `language` - The language of the file.
/// * `keyword_files` - The keyword files.
/// * `parse_error` - The position of the first parse error in the file, if any.
fn file_error_row(
    file: &RepoFile,
    error: ErrorCode,
    language: &str,
    keyword_files: &KeywordFiles,
    parse_error: Option<(usize, usize)>,
) -> LogRow {
    file.row()
        .into_iter()
        .chain([error.to_string(), language.to_string()])
        // functions, functions_with_kw, and one statistic per keyword file
        .chain(std::iter::repeat_n(String::new(), 2 + keyword_files.len()))
        .chain(position_cells(parse_error))
        .collect()
}

/// Returns the row of a function skipped because of a parse error: the function, its error, its language,
/// empty statistics, and the position of the parse error.
/// The function is not extracted, so its `local_function_path` is empty.
///
/// # Arguments
///
/// * `function` - The function.
/// * `write_out` - Whether the functions are extracted to files.
/// * `language` - The language of the function.
/// * `keyword_files` - The keyword files.
/// * `parse_error` - The position of the first parse error, relative to the function.
fn function_error_row(
    function: &Function,
    write_out: bool,
    language: &str,
    keyword_files: &KeywordFiles,
    parse_error: Option<(usize, usize)>,
) -> OutputRow {
    const STATISTICS_AFTER_KEYWORDS: usize = 9;
    function
        .row()
        .into_iter()
        .chain([ErrorCode::ParseError.to_string()])
        .chain(write_out.then(String::new))
        .chain([language.to_string()])
        // loc, words, one statistic per keyword file, and the statistics after the keywords
        .chain(std::iter::repeat_n(
            String::new(),
            2 + keyword_files.len() + STATISTICS_AFTER_KEYWORDS,
        ))
        .chain(position_cells(parse_error))
        .collect()
}

/// Returns the cells of the position of a parse error: its line and its column, or empty cells without error.
///
/// # Arguments
///
/// * `position` - The line and the column of the parse error, if any.
fn position_cells(position: Option<(usize, usize)>) -> [String; 2] {
    match position {
        Some((line, column)) => [line.to_string(), column.to_string()],
        None => [String::new(), String::new()],
    }
}

/// Returns the position of a parse error, as details of its error.
///
/// # Arguments
///
/// * `position` - The line and the column of the parse error, if any.
fn position_detail(position: Option<(usize, usize)>) -> String {
    match position {
        Some((line, column)) => format!("line {line}, column {column}"),
        None => String::new(),
    }
}

/// Returns the name of a function, without its parameters and whitespace. Anonymous functions have no name.
///
/// # Arguments
///
/// * `function` - The node of the function.
/// * `grammar` - The grammar of the language.
/// * `source` - The source code of the whole file.
fn function_name(function: &Node, grammar: &Grammar, source: &[u8]) -> String {
    if grammar.anon_function_nodes.contains(function.kind()) {
        return String::new();
    }
    let mut name: String = String::from_utf8_lossy(
        find_signature_fields(function, grammar.name_field, grammar)
            .first()
            .map(|n| node_source_code(n, source))
            .unwrap_or(b""),
    )
    .to_string();
    if let Some(idx) = name.find('(') {
        name.truncate(idx);
    }
    name.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Returns the position of the first parse error in a function, relative to the start of the function.
///
/// # Arguments
///
/// * `function` - The node of the function.
/// * `function_position` - The line and column where the function starts in the file.
fn relative_error_position(
    function: &Node,
    function_position: (usize, usize),
) -> Option<(usize, usize)> {
    find_first_error_position(function).map(|(row, col)| {
        let error_row = row - function_position.0 + 1;
        if row == function_position.0 {
            (error_row, col - function_position.1 + 1)
        } else {
            (error_row, col)
        }
    })
}

/// Extracts the functions from a subtree of a source file and writes them to individual files
/// if they contain one of the provided keywords. Returns statistics about all the functions
/// in the subtree.
///
///
/// # Arguments
///
/// * `file` - The file the functions are extracted from.
/// * `root` - The root node of the subtree.
/// * `language` - The language of the source file.
/// * `grammar` - The grammar of the language.
/// * `source` - The source code of the source file.
/// * `keyword_files` - The keyword files containing the keywords to search for in the functions.
/// * `fail_policy` - The policy to apply when a parse error is encountered.
/// * `ignore_comments` - Whether to ignore comments when extracting functions, in addition to ignoring them during keyword matching.
/// * `lambdas` - Whether to extract lambda functions as well.
/// * `word_counter` - The matcher to use to count the words in the functions.
/// * `write_out` - Whether to write the extracted functions to files.
/// * `parser` - The parser to use to parse the functions.
///
/// # Returns
///
/// A tuple containing the statistics of the functions in the file and the function number after processing the file node
///
fn extract_functions(
    file: &RepoFile,
    root: &Node,
    language: &str,
    grammar: &Grammar,
    source: &[u8],
    keyword_files: &KeywordFiles,
    fail_policy: &str,
    ignore_comments: bool,
    lambdas: bool,
    write_out: bool,
    word_counter: &Matcher,
    parser: &mut Parser,
) -> Result<(Vec<OutputRow>, usize, usize, Vec<usize>)> {
    let file_path: &str = &file.local_path;
    let target_folder = format!("{file_path}.functions");

    // Initializes the builder to store the statistics of the functions in the file
    let mut rows: Vec<OutputRow> = Vec::new();
    let mut functions: usize = 0;
    let mut functions_with_kw: usize = 0;
    let mut functions_with_specific_kw: Vec<usize> = vec![0; keyword_files.paths.len()];

    // Simulating call stack
    let mut call_stack: Vec<Node> = Vec::new();
    call_stack.push(*root);
    let mut cursor = root.walk();

    while let Some(node) = call_stack.pop() {
        if grammar.is_function(&node, lambdas) {
            let has_error: bool = node.has_error();

            let function_position: (usize, usize) = (
                node.start_position().row + 1,
                node.start_position().column + 1,
            );

            if language == "java" && find_fields(&node, "body").is_empty() {
                continue;
            } else if has_error && fail_policy == "skip-function" {
                let function = Function {
                    file,
                    name: function_name(&node, grammar, source),
                    line: function_position.0,
                    column: function_position.1,
                };
                let position: Option<(usize, usize)> =
                    relative_error_position(&node, function_position);
                debug!(
                    "Function {} of {}: {}",
                    function.name,
                    file.local_path,
                    RowError::new(ErrorCode::ParseError, position_detail(position))
                );
                rows.push(function_error_row(
                    &function,
                    write_out,
                    language,
                    keyword_files,
                    position,
                ));
                functions += 1;
            } else {
                // Function source code
                let function_source_code: &[u8] = node_source_code(&node, source);

                let error_position: Option<(usize, usize)> = if has_error {
                    relative_error_position(&node, function_position)
                } else {
                    None
                };

                // Fetch the code of the function and remove comments from it
                let function_code_with_strings: &Vec<u8> =
                    &remove_kind_from_source(function_source_code, &node, &grammar.comment_nodes);
                // Re parse the function without comments to get the correct tree
                let tree_without_comments: Tree = parser
                    .parse(function_code_with_strings, None)
                    .with_context(|| {
                        format!("Error parsing code for function at line {}, column {} in file {file_path}", function_position.0, function_position.1)
                    })?;

                // Remove string literals from the function code
                let function_code = &remove_kind_from_source(
                    function_code_with_strings,
                    &tree_without_comments.root_node(),
                    &grammar.string_literal_nodes,
                );

                let matches: Vec<usize> =
                    keyword_files.count_matches_in_text(language, function_code);

                if matches.iter().any(|x| *x > 0) {
                    let function_path: Option<String> = write_out.then(|| {
                        Path::new(&target_folder)
                            .join(format!("{}-{}", function_position.0, function_position.1))
                            .to_string_lossy()
                            .into_owned()
                    });
                    if let Some(function_path) = &function_path {
                        std::fs::write(
                            function_path,
                            if ignore_comments {
                                function_code_with_strings
                            } else {
                                function_source_code
                            },
                        )?;
                    }

                    // Count the number of loops, conditionals and parameters if the function
                    let (loops, loop_nesting) = count_nodes_of_kind(&node, &grammar.loop_nodes);
                    let (conditionals, conditional_nesting) =
                        count_nodes_of_kind(&node, &grammar.cond_nodes);
                    let (calls, calls_nesting) =
                        count_nodes_of_kind(&node, &grammar.function_call_nodes);

                    let params_vec: Vec<Node<'_>> =
                        find_signature_fields(&node, grammar.param_seq_field, grammar);

                    let name: String = function_name(&node, grammar, source);

                    let mut n_param: usize = 0;
                    let mut param_match: usize = 0;
                    for params in params_vec {
                        for (param, declared_names) in parameters(&params, grammar) {
                            let type_matches: bool = grammar
                                .param_type_field
                                .and_then(|field| param.child_by_field_name(field))
                                .map(|x| node_source_code(&x, source))
                                .is_some_and(|x| keyword_files.has_matches_in_text(language, x));

                            n_param += declared_names;
                            if type_matches {
                                param_match += declared_names;
                            }
                        }
                    }

                    let return_type_match = match grammar.return_type_field {
                        Some(field) => {
                            // Safe unwrap: whole source code was read as utf8 before
                            // Safe unwrap: the pattern is already checked above
                            find_signature_fields(&node, field, grammar)
                                .first()
                                .map(|x| node_source_code(x, source))
                                .filter(|x| keyword_files.has_matches_in_text(language, x))
                                .map(|_| 1)
                                .unwrap_or(0)
                        }
                        None => 0,
                    };

                    let function = Function {
                        file,
                        name,
                        line: function_position.0,
                        column: function_position.1,
                    };
                    let row: OutputRow = function
                        .row()
                        .into_iter()
                        .chain([error_cell(None)])
                        .chain(function_path)
                        .chain([
                            language.to_string(),
                            count_text_lines(function_code_with_strings).to_string(),
                            word_counter
                                .count_matches_in_text(function_code_with_strings)
                                .to_string(),
                        ])
                        .chain(matches.iter().map(|m| m.to_string()))
                        .chain([
                            loops.to_string(),
                            loop_nesting.to_string(),
                            conditionals.to_string(),
                            conditional_nesting.to_string(),
                            calls.to_string(),
                            calls_nesting.to_string(),
                            n_param.to_string(),
                            param_match.to_string(),
                            return_type_match.to_string(),
                        ])
                        .chain(position_cells(error_position))
                        .collect();
                    rows.push(row);
                    functions_with_kw += 1;
                    for (i, m) in matches.iter().enumerate() {
                        if *m > 0 {
                            functions_with_specific_kw[i] += 1;
                        }
                    }
                }
                functions += 1;
            }
        } else {
            for c in node
                .children(&mut cursor)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
            {
                call_stack.push(c);
            }
        }
    }
    Ok((
        rows,
        functions,
        functions_with_kw,
        functions_with_specific_kw,
    ))
}

/// Returns the source code of a node in the parse tree
///
/// # Arguments
///
/// * `n` - The node to extract the source code from.
/// * `source` - The source code of the whole file.
fn node_source_code<'a>(n: &Node, source: &'a [u8]) -> &'a [u8] {
    &source[n.start_byte()..n.end_byte()]
}

/// Grammar of a programming language.
struct Grammar {
    /// The programming language the grammar belongs to.
    lang: Language,

    /// Nodes representing comments.
    comment_nodes: HashSet<&'static str>,

    /// Nodes representing string literals.
    string_literal_nodes: HashSet<&'static str>,

    /// Nodes representing loops.
    loop_nodes: HashSet<&'static str>,

    /// Nodes representing conditional statements.
    cond_nodes: HashSet<&'static str>,

    /// Nodes representing named functions or methods.
    named_function_nodes: HashSet<&'static str>,

    /// Nodes representing anonymours functions or lambdas.
    anon_function_nodes: HashSet<&'static str>,

    /// Nodes representing function or method calls.
    function_call_nodes: HashSet<&'static str>,

    /// Nodes representing a parameter of a function or method.
    param_nodes: HashSet<&'static str>,

    /// The field name of the parameter type.
    param_type_field: Option<&'static str>,

    /// The field name of the return type.
    return_type_field: Option<&'static str>,

    /// The field name of the function or method name, which also holds the names declared by a parameter.
    name_field: &'static str,

    /// The field holding the parameter lists of a function, either in the function itself or deeper in its
    /// signature, such as in the declarator of a C function.
    param_seq_field: &'static str,

    /// The field of a function holding its body. It is skipped when looking for the name or the return type.
    function_body_field: &'static str,
}

impl Grammar {
    /// Returns whether a node represents a function or method in the grammar.
    ///
    /// # Arguments
    /// * `node` - The node to check.
    /// * `with_lambdas` - Whether lambda functions count as functions. If false, only named functions do.
    fn is_function(&self, node: &Node, with_lambdas: bool) -> bool {
        let kind: &str = node.kind();
        self.named_function_nodes.contains(kind)
            || (with_lambdas && self.anon_function_nodes.contains(kind))
    }
}

/// Returns the grammar for the C programming language.
fn c_grammar() -> Grammar {
    Grammar {
        lang: tree_sitter_c::LANGUAGE.into(),
        comment_nodes: vec!["comment"].into_iter().collect(),
        string_literal_nodes: vec!["string_literal"].into_iter().collect(),
        loop_nodes: vec!["for_statement", "while_statement", "do_statement"]
            .into_iter()
            .collect(),
        cond_nodes: vec!["if_statement", "switch_statement", "conditional_expression"]
            .into_iter()
            .collect(),
        named_function_nodes: vec!["function_definition"].into_iter().collect(),
        anon_function_nodes: HashSet::new(),
        function_call_nodes: vec!["call_expression"].into_iter().collect(),
        param_nodes: vec!["parameter_declaration"].into_iter().collect(),
        param_type_field: Some("type"),
        return_type_field: Some("type"),
        name_field: "declarator",
        param_seq_field: "parameters",
        function_body_field: "body",
    }
}

/// Returns the grammar for the C++ programming language.
fn cpp_grammar() -> Grammar {
    Grammar {
        lang: tree_sitter_cpp::LANGUAGE.into(),
        comment_nodes: vec!["comment"].into_iter().collect(),
        string_literal_nodes: vec!["string_literal", "raw_string_literal"]
            .into_iter()
            .collect(),
        loop_nodes: vec![
            "for_range_loop",
            "for_statement",
            "while_statement",
            "do_statement",
        ]
        .into_iter()
        .collect(),
        cond_nodes: vec!["if_statement", "switch_statement", "conditional_expression"]
            .into_iter()
            .collect(),
        named_function_nodes: vec!["function_definition"].into_iter().collect(),
        anon_function_nodes: vec!["lambda_expression"].into_iter().collect(),
        function_call_nodes: vec!["call_expression"].into_iter().collect(),
        param_nodes: vec![
            "parameter_declaration",
            "optional_parameter_declaration",
            "variadic_parameter_declaration",
        ]
        .into_iter()
        .collect(),
        param_type_field: Some("type"),
        return_type_field: Some("type"),
        name_field: "declarator",
        param_seq_field: "parameters",
        function_body_field: "body",
    }
}

/// Returns the grammar for the C# programming language.
fn cs_grammar() -> Grammar {
    Grammar {
        lang: tree_sitter_c_sharp::LANGUAGE.into(),
        comment_nodes: vec!["comment"].into_iter().collect(),
        string_literal_nodes: vec![
            "string_literal",
            "verbatim_string_literal",
            "raw_string_literal",
            "string_content",
        ]
        .into_iter()
        .collect(),
        loop_nodes: vec![
            "for_statement",
            "foreach_statement",
            "while_statement",
            "do_statement",
        ]
        .into_iter()
        .collect(),
        cond_nodes: vec![
            "if_statement",
            "switch_statement",
            "switch_expression",
            "conditional_expression",
        ]
        .into_iter()
        .collect(),
        named_function_nodes: vec![
            "method_declaration",
            "constructor_declaration",
            "operator_declaration",
        ]
        .into_iter()
        .collect(),
        anon_function_nodes: vec!["lambda_expression", "anonymous_method_expression"]
            .into_iter()
            .collect(),
        function_call_nodes: vec!["invocation_expression"].into_iter().collect(),
        param_nodes: vec!["parameter"].into_iter().collect(),
        param_type_field: Some("type"),
        return_type_field: Some("returns"),
        name_field: "name",
        param_seq_field: "parameters",
        function_body_field: "body",
    }
}

/// Returns the grammar for the TypeScript programming language.
fn ts_grammar() -> Grammar {
    Grammar {
        lang: tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        comment_nodes: vec!["comment"].into_iter().collect(),
        string_literal_nodes: vec!["string_fragment"].into_iter().collect(),
        loop_nodes: vec![
            "for_statement",
            "for_in_statement",
            "while_statement",
            "do_statement",
        ]
        .into_iter()
        .collect(),
        cond_nodes: vec!["if_statement", "switch_statement", "ternary_expression"]
            .into_iter()
            .collect(),
        named_function_nodes: vec!["function_declaration", "method_definition"]
            .into_iter()
            .collect(),
        anon_function_nodes: vec!["arrow_function"].into_iter().collect(),
        function_call_nodes: vec!["new_expression", "call_expression"]
            .into_iter()
            .collect(),
        param_nodes: vec!["required_parameter", "optional_parameter"]
            .into_iter()
            .collect(),
        param_type_field: Some("type"),
        return_type_field: Some("return_type"),
        name_field: "name",
        param_seq_field: "parameters",
        function_body_field: "body",
    }
}

/// Returns the grammar for the Go programming language.
fn go_grammar() -> Grammar {
    Grammar {
        lang: tree_sitter_go::LANGUAGE.into(),
        comment_nodes: vec!["comment"].into_iter().collect(),
        string_literal_nodes: vec!["raw_string_literal", "interpreted_string_literal"]
            .into_iter()
            .collect(),
        loop_nodes: vec!["for_statement"].into_iter().collect(),
        cond_nodes: vec![
            "if_statement",
            "type_switch_statement",
            "expression_switch_statement",
        ]
        .into_iter()
        .collect(),
        named_function_nodes: vec!["function_declaration", "method_declaration"]
            .into_iter()
            .collect(),
        anon_function_nodes: vec!["func_literal"].into_iter().collect(),
        function_call_nodes: vec!["call_expression"].into_iter().collect(),
        param_nodes: vec!["parameter_declaration", "variadic_parameter_declaration"]
            .into_iter()
            .collect(),
        param_type_field: Some("type"),
        return_type_field: Some("result"),
        name_field: "name",
        param_seq_field: "parameters",
        function_body_field: "body",
    }
}

/// Returns the grammar for the Java programming language.
fn java_grammar() -> Grammar {
    Grammar {
        lang: tree_sitter_java::LANGUAGE.into(),
        comment_nodes: vec!["line_comment", "block_comment"].into_iter().collect(),
        string_literal_nodes: vec!["string_literal"].into_iter().collect(),
        loop_nodes: vec![
            "for_statement",
            "enhanced_for_statement",
            "while_statement",
            "do_statement",
        ]
        .into_iter()
        .collect(),
        cond_nodes: vec!["if_statement", "ternary_expression", "switch_expression"]
            .into_iter()
            .collect(),
        named_function_nodes: vec![
            "method_declaration",
            "constructor_declaration",
            "compact_constructor_declaration",
        ]
        .into_iter()
        .collect(),
        anon_function_nodes: vec!["lambda_expression"].into_iter().collect(),
        function_call_nodes: vec!["method_invocation", "explicit_constructor_invocation"]
            .into_iter()
            .collect(),
        param_nodes: vec!["formal_parameter", "spread_parameter"]
            .into_iter()
            .collect(),
        param_type_field: Some("type"),
        return_type_field: Some("type"),
        name_field: "name",
        param_seq_field: "parameters",
        function_body_field: "body",
    }
}

/// Returns the grammar for the Scala programming language.
fn scala_grammar() -> Grammar {
    Grammar {
        lang: tree_sitter_scala::LANGUAGE.into(),
        comment_nodes: vec!["comment", "block_comment"].into_iter().collect(),
        string_literal_nodes: vec!["string", "interpolated_string"].into_iter().collect(),
        loop_nodes: vec!["for_expression", "while_expression", "do_while_expression"]
            .into_iter()
            .collect(),
        cond_nodes: vec!["if_expression", "match_expression"]
            .into_iter()
            .collect(),
        named_function_nodes: vec!["function_definition"].into_iter().collect(),
        anon_function_nodes: vec!["lambda_expression"].into_iter().collect(),
        function_call_nodes: vec!["call_expression"].into_iter().collect(),
        param_nodes: vec!["parameter"].into_iter().collect(),
        param_type_field: Some("type"),
        return_type_field: Some("return_type"),
        name_field: "name",
        param_seq_field: "parameters",
        function_body_field: "body",
    }
}

/// Returns the grammar for the Fortran programming language.
fn fortran_grammar() -> Grammar {
    Grammar {
        lang: tree_sitter_fortran::LANGUAGE.into(),
        comment_nodes: vec!["preproc_comment", "comment"].into_iter().collect(),
        string_literal_nodes: vec!["string_literal"].into_iter().collect(),
        loop_nodes: vec![
            "do_loop_statement",
            "do_label_statement",
            "where_statement",
            "forall_statement",
        ]
        .into_iter()
        .collect(),
        cond_nodes: vec![
            "if_statement",
            "arithmetic_if_statement",
            "select_case_statement",
            "select_rank_statement",
            "select_type_statement",
        ]
        .into_iter()
        .collect(),
        named_function_nodes: vec!["function", "subroutine"].into_iter().collect(),
        anon_function_nodes: HashSet::new(),
        function_call_nodes: vec!["call_expression", "subroutine_call"]
            .into_iter()
            .collect(),
        param_nodes: vec!["identifier"].into_iter().collect(),
        param_type_field: None,
        return_type_field: None,
        name_field: "name",
        param_seq_field: "parameters",
        function_body_field: "body",
    }
}

/// Returns the grammar for the Python programming language.
fn python_grammar() -> Grammar {
    Grammar {
        lang: tree_sitter_python::LANGUAGE.into(),
        comment_nodes: vec!["comment"].into_iter().collect(),
        string_literal_nodes: vec!["string"].into_iter().collect(),
        loop_nodes: vec!["for_statement", "while_statement"]
            .into_iter()
            .collect(),
        cond_nodes: vec!["if_statement", "conditional_expression", "match_statement"]
            .into_iter()
            .collect(),
        named_function_nodes: vec!["function_definition"].into_iter().collect(),
        anon_function_nodes: vec!["lambda"].into_iter().collect(),
        function_call_nodes: vec!["call"].into_iter().collect(),
        param_nodes: vec![
            "identifier",
            "typed_parameter",
            "default_parameter",
            "typed_default_parameter",
            "list_splat_pattern",
            "dictionary_splat_pattern",
        ]
        .into_iter()
        .collect(),
        param_type_field: None,
        return_type_field: None,
        name_field: "name",
        param_seq_field: "parameters",
        function_body_field: "body",
    }
}

/// Returns the grammar for the Rust programming language.
fn rust_grammar() -> Grammar {
    Grammar {
        lang: tree_sitter_rust::LANGUAGE.into(),
        comment_nodes: vec!["line_comment", "block_comment"].into_iter().collect(),
        string_literal_nodes: vec!["string_literal", "raw_string_literal"]
            .into_iter()
            .collect(),
        loop_nodes: vec!["for_expression", "loop_expression", "while_expression"]
            .into_iter()
            .collect(),
        cond_nodes: vec!["if_expression", "match_expression"]
            .into_iter()
            .collect(),
        named_function_nodes: vec!["function_item"].into_iter().collect(),
        anon_function_nodes: vec!["closure_expression"].into_iter().collect(),
        function_call_nodes: vec!["call_expression"].into_iter().collect(),
        param_nodes: vec!["parameter"].into_iter().collect(),
        param_type_field: Some("type"),
        return_type_field: Some("return_type"),
        name_field: "name",
        param_seq_field: "parameters",
        function_body_field: "body",
    }
}

/// Returns the grammar corresponding to the given language.
///
/// # Arguments
///
/// * `language` - The language of the file.
///
/// # Returns
///
/// The grammar corresponding to the language or `None` if the language is not supported.
fn language_to_grammar(lang: &str) -> Option<Grammar> {
    match lang.to_lowercase().as_str() {
        "c" => Some(c_grammar()),
        "c++" => Some(cpp_grammar()),
        "c#" => Some(cs_grammar()),
        "java" => Some(java_grammar()),
        "fortran" => Some(fortran_grammar()),
        "python" => Some(python_grammar()),
        "typescript" => Some(ts_grammar()),
        "go" => Some(go_grammar()),
        "scala" => Some(scala_grammar()),
        "rust" => Some(rust_grammar()),
        _ => None,
    }
}

/// Counts the number of nodes of given kinds in a tree.
///
/// # Arguments
///
/// * `node` - The root node of the tree.
/// * `kind` - The kinds of nodes to count.
///
/// # Returns
///
/// A tuple containing the number of nodes of the given kind and the maximum nesting level of these nodes.
///
/// # Example
///
/// The function applied to a node representing the following code will return `(2, 2)` if the kind is `if_statement`:
///
/// ```c
/// int main(int a, int b) {
///     if (b > 0) {
///         if (a > b) {
///             return a;
///         } else {
///             return b;
///         }
///     }
///     return 0;
///  }
/// ```
///
fn count_nodes_of_kind(root: &Node, kinds: &HashSet<&str>) -> (usize, usize) {
    let mut node_count = 0;
    let mut max_nesting = 0;

    let mut cursor = root.walk();

    // Simulating call stack
    let mut call_stack: Vec<(Node, usize)> = Vec::new();
    call_stack.push((*root, 1));

    while let Some((node, depth)) = call_stack.pop() {
        let is_of_kind = kinds.contains(node.kind());

        if is_of_kind {
            node_count += 1;
            max_nesting = max_nesting.max(depth);
        }

        // We don't reverse nodes for performance (yields the same result)
        for child in node.children(&mut cursor) {
            call_stack.push((child, if is_of_kind { depth + 1 } else { depth }));
        }
    }

    (node_count, max_nesting)
}

fn find_first_node<'a>(
    node: &Node<'a>,
    pred: &dyn Fn(&Node) -> bool,
    breadth: bool,
) -> Vec<Node<'a>> {
    let mut cursor = node.walk();
    let mut call_stack: Vec<(Node, usize)> = Vec::new();
    call_stack.push((*node, 0));

    let mut res: Vec<Node<'a>> = Vec::new();
    let mut max_depth: Option<usize> = None;

    while let Some((node, depth)) = call_stack.pop() {
        if max_depth.filter(|&d| depth > d).is_some() {
            return res;
        } else if pred(&node) {
            if breadth {
                res.push(node);
                if max_depth.is_none() {
                    max_depth = Some(depth);
                }
            } else {
                return vec![node];
            }
        } else if breadth {
            let mut end_queue: Vec<(Node, usize)> =
                node.children(&mut cursor).map(|c| (c, depth + 1)).collect();
            end_queue.extend(call_stack);
            call_stack = end_queue;
        } else {
            for c in node
                .children(&mut cursor)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
            {
                call_stack.push((c, 0));
            }
        }
    }
    vec![]
}

/// Finds the first error node in the tree
///
/// # Arguments
///
/// * `root` - The root node of the tree.
///
/// # Returns
///
/// The first error node found in the tree, or `None` if no error node is found.
fn find_first_error_node<'a>(root: &Node<'a>) -> Option<Node<'a>> {
    find_first_node(root, &|n: &Node| n.is_error() || n.is_missing(), false)
        .into_iter()
        .next()
}

fn find_first_error_position(root: &Node) -> Option<(usize, usize)> {
    find_first_error_node(root).map(|n| (n.start_position().row + 1, n.start_position().column + 1))
}

fn find_fields<'a>(root: &Node<'a>, field: &str) -> Vec<Node<'a>> {
    let mut res: Vec<Node<'a>> = Vec::new();
    let mut ids: HashSet<usize> = HashSet::new();

    let mut cursor = root.walk();

    // Simulating call stack
    let mut call_stack: Vec<Node> = Vec::new();
    call_stack.push(*root);

    while let Some(node) = call_stack.pop() {
        for c in node.children_by_field_name(field, &mut node.walk()) {
            res.push(c);
            ids.insert(c.id());
        }

        // We don't reverse nodes for performance (yields the same result)
        for c in node
            .children(&mut cursor)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            if !ids.contains(&c.id()) {
                call_stack.push(c);
            }
        }
    }

    res
}

/// Finds the nodes in a field of the signature of a function, that is, outside of its body and of its
/// parameter lists. The nodes are those of the first node of the signature having this field.
///
/// # Arguments
///
/// * `function` - The node of the function.
/// * `field` - The name of the field to find.
/// * `grammar` - The grammar of the language.
///
/// # Returns
///
/// The named nodes in the field, or no node if the signature has no such field.
fn find_signature_fields<'a>(function: &Node<'a>, field: &str, grammar: &Grammar) -> Vec<Node<'a>> {
    // Simulating call stack
    let mut call_stack: Vec<Node<'a>> = vec![*function];

    while let Some(node) = call_stack.pop() {
        let mut cursor = node.walk();
        let found: Vec<Node<'a>> = node
            .children_by_field_name(field, &mut cursor)
            .filter(|c| c.is_named())
            .collect();
        if !found.is_empty() {
            return found;
        }

        let mut signature_children: Vec<Node<'a>> = Vec::new();
        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                let skipped: bool = matches!(
                    cursor.field_name(),
                    Some(name) if name == grammar.function_body_field || name == grammar.param_seq_field
                );
                if !skipped {
                    signature_children.push(cursor.node());
                }
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
        call_stack.extend(signature_children.into_iter().rev());
    }

    Vec::new()
}

/// Returns the parameters declared directly in a parameter list, each with the number of names it declares.
///
/// # Arguments
///
/// * `list` - The node of the parameter list.
/// * `grammar` - The grammar of the language.
fn parameters<'a>(list: &Node<'a>, grammar: &Grammar) -> Vec<(Node<'a>, usize)> {
    let mut cursor = list.walk();
    list.children(&mut cursor)
        .filter(|c| grammar.param_nodes.contains(c.kind()))
        .map(|c| {
            let declared_names: usize = c
                .children_by_field_name(grammar.name_field, &mut c.walk())
                .count()
                .max(1);
            (c, declared_names)
        })
        .collect()
}

fn find_kind<'a>(root: &Node<'a>, kinds: &HashSet<&str>) -> Vec<Node<'a>> {
    let mut res: Vec<Node<'a>> = Vec::new();

    let mut cursor = root.walk();

    // Simulating call stack
    let mut call_stack: Vec<Node> = Vec::new();
    call_stack.push(*root);

    while let Some(node) = call_stack.pop() {
        if kinds.contains(node.kind()) {
            res.push(node);
        } else {
            // We don't reverse nodes for performance (yields the same result)
            for c in node.children(&mut cursor) {
                call_stack.push(c);
            }
        }
    }

    res
}

fn remove_kind_from_source(source: &[u8], root: &Node, kinds: &HashSet<&str>) -> Vec<u8> {
    let mut nodes = find_kind(root, kinds);
    nodes.sort_by_key(|b| std::cmp::Reverse(b.start_byte()));
    // Disable mutability
    let nodes = nodes;

    let root_start = root.start_byte();
    let mut new_source = source.to_vec();
    for n in nodes {
        new_source.drain(n.start_byte() - root_start..n.end_byte() - root_start);
    }
    new_source
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;

    use crate::utils::dataframes;
    use crate::utils::fs::*;
    use crate::utils::logger::test_logger;
    use crate::utils::repo::{COLUMN, FUNCTION_NAME, LINE, URL};
    use anyhow::ensure;
    use polars::prelude::SortMultipleOptions;

    use super::*;

    const TEST_DATA: &str = "tests/data/phases/parse";

    fn test_parse(
        input_file_path: &str,
        keywords: &[&str],
        ignore_comments: bool,
        lambdas: bool,
        write_out: bool,
        should_pass: bool,
    ) -> Result<()> {
        let input_df = open_csv(input_file_path, None, None, &[])?;
        let input_df: Vec<&str> = dataframes::str(&input_df, LOCAL_PATH)?;

        let output_file_path = format!("{input_file_path}.functions.csv");
        delete_file(&output_file_path, true)?;

        let logs_file_path = format!("{input_file_path}.function_logs.csv");
        delete_file(&logs_file_path, true)?;

        if write_out {
            for path in input_df.iter() {
                delete_dir(format!("{path}.functions"), true)?;
            }
        }

        if should_pass {
            run(
                input_file_path,
                None,
                None,
                keywords,
                false,
                false,
                "ignore",
                8,
                0,
                false,
                ignore_comments,
                lambdas,
                write_out,
                false,
                &[],
                test_logger(),
            )?;

            let logs_df = open_csv(&logs_file_path, None, None, &[])?;
            let sorted_logs_df = logs_df.sort(vec![LOCAL_PATH], SortMultipleOptions::new())?;

            let expected_logs_df = open_csv(
                &format!("{input_file_path}.function_logs.csv.expected"),
                None,
                None,
                &[],
            )?;
            let sorted_expected_logs_df =
                expected_logs_df.sort(vec![LOCAL_PATH], SortMultipleOptions::new())?;
            assert_eq!(sorted_expected_logs_df, sorted_logs_df);

            let output_df = open_csv(&output_file_path, None, None, &[])?;

            if write_out {
                let sorted_output_df =
                    output_df.sort(vec![LOCAL_FUNCTION_PATH], SortMultipleOptions::new())?;
                let expected_df =
                    open_csv(&format!("{output_file_path}.expected"), None, None, &[])?;
                let sorted_expected_df =
                    expected_df.sort(vec![LOCAL_FUNCTION_PATH], SortMultipleOptions::new())?;
                assert_eq!(sorted_expected_df, sorted_output_df);

                for path in dataframes::str(&sorted_output_df, LOCAL_FUNCTION_PATH)? {
                    let path = Path::new(path);
                    ensure!(path.exists(), "Parsed file not found: {}", path.display());
                    let expected_path_name = format!(
                        "{}.expected/{}",
                        path.parent()
                            .with_context(|| "Failed to get parent directory")?
                            .to_str()
                            .with_context(|| "Failed to convert parent directory to string")?,
                        path.file_name()
                            .with_context(|| "Failed to get file name")?
                            .to_str()
                            .with_context(|| "Failed to convert file name to string")?
                    );
                    let expected_path = Path::new(&expected_path_name);
                    assert_eq!(
                        std::fs::read_to_string(path)?,
                        std::fs::read_to_string(expected_path)?
                    );
                }
            } else {
                let sorted_output_df =
                    output_df.sort(vec![LOCAL_PATH, LINE, COLUMN], SortMultipleOptions::new())?;
                let expected_df = open_csv(
                    &format!("{output_file_path}.count.expected"),
                    None,
                    None,
                    &[],
                )?;
                let sorted_expected_df =
                    expected_df.sort(vec![LOCAL_PATH, LINE, COLUMN], SortMultipleOptions::new())?;
                assert_eq!(sorted_expected_df, sorted_output_df);
            }
        } else {
            ensure!(run(
                input_file_path,
                None,
                None,
                keywords,
                false,
                false,
                "ignore",
                8,
                0,
                false,
                ignore_comments,
                lambdas,
                write_out,
                false,
                &[],
                test_logger()
            )
            .is_err());
        }

        delete_file(&output_file_path, true)?;
        delete_file(&logs_file_path, true)?;

        if write_out {
            for path in input_df {
                delete_dir(format!("{path}.functions"), true)?;
            }
        }
        Ok(())
    }

    #[test]
    fn parse_fp() -> Result<()> {
        let keywords = vec![
            "tests/data/keywords/fp_types.json",
            "tests/data/keywords/fp_transcendental.json",
            "tests/data/keywords/fp_others.json",
            "tests/data/keywords/long_double.json",
        ];

        let input_file_path = format!("{TEST_DATA}/to_parse.csv");

        test_parse(&input_file_path, &keywords, false, true, true, true)
    }

    #[test]
    fn parse_go() -> Result<()> {
        let keywords = vec![
            "tests/data/keywords/fp_types.json",
            "tests/data/keywords/fp_transcendental.json",
            "tests/data/keywords/fp_others.json",
        ];

        let input_file_path = format!("{TEST_DATA}/parse_go.csv");

        test_parse(&input_file_path, &keywords, false, true, true, true)
    }

    /// Runs the parser with a failure policy and returns the content of the functions file and of the logs file.
    fn parse_with_policy(
        input_file_path: &str,
        keywords: &[&str],
        fail_policy: &str,
    ) -> Result<(String, String)> {
        parse_with_options(input_file_path, keywords, fail_policy, false, &[])
    }

    /// Same as [`parse_with_policy`], with the options reading the repository columns.
    fn parse_with_options(
        input_file_path: &str,
        keywords: &[&str],
        fail_policy: &str,
        infer_source: bool,
        renames: &[(String, String)],
    ) -> Result<(String, String)> {
        let output_path = format!("{input_file_path}.{fail_policy}.functions.csv");
        let logs_path = format!("{input_file_path}.{fail_policy}.function_logs.csv");
        let result = run(
            input_file_path,
            Some(&output_path),
            Some(&logs_path),
            keywords,
            false,
            false,
            fail_policy,
            1,
            0,
            true,
            false,
            false,
            false,
            infer_source,
            renames,
            test_logger(),
        )
        .and_then(|_| {
            Ok((
                std::fs::read_to_string(&output_path)?,
                std::fs::read_to_string(&logs_path)?,
            ))
        });
        delete_file(&output_path, true)?;
        delete_file(&logs_path, true)?;
        result
    }

    #[test]
    fn skip_file_writes_error_log_row() -> Result<()> {
        let (functions, logs) = parse_with_policy(
            &format!("{TEST_DATA}/invalid.csv"),
            &["tests/data/keywords/c_float.json"],
            "skip-file",
        )?;
        assert_eq!(functions.lines().count(), 1);
        assert_eq!(
            logs.lines().nth(1),
            Some("0,local,,,,tests/data/phases/parse/invalid.c,invalid.c,,E030,c,,,,1,25")
        );
        Ok(())
    }

    #[test]
    fn skip_function_writes_error_function_row() -> Result<()> {
        let (functions, logs) = parse_with_policy(
            &format!("{TEST_DATA}/invalid.csv"),
            &["tests/data/keywords/c_float.json"],
            "skip-function",
        )?;
        assert_eq!(
            functions.lines().nth(1),
            Some("0,local,,,,tests/data/phases/parse/invalid.c,invalid.c,main,1,5,,E030,c,,,,,,,,,,,,,1,21")
        );
        assert_eq!(
            logs.lines().nth(1),
            Some("0,local,,,,tests/data/phases/parse/invalid.c,invalid.c,,,c,1,0,0,1,25")
        );
        Ok(())
    }

    #[test]
    fn abort_stops_on_parse_error() {
        assert!(parse_with_policy(
            &format!("{TEST_DATA}/invalid.csv"),
            &["tests/data/keywords/c_float.json"],
            "abort",
        )
        .is_err());
    }

    #[test]
    fn unsupported_languages_are_skipped() -> Result<()> {
        let input_file_path = format!("{TEST_DATA}/unsupported_language.csv");
        write_file(
            &input_file_path,
            b"repo_id,source,local_path,file\n0,local,tests/data/phases/parse/invalid.c,invalid.c\n1,local,tests/data/phases/parse/header.h,header.h\n",
        )?;
        let result = parse_with_policy(
            &input_file_path,
            &["tests/data/keywords/fp_types.json"],
            "ignore",
        );
        delete_file(&input_file_path, false)?;
        let (_, logs) = result?;
        assert_eq!(logs.lines().count(), 2);
        ensure!(logs.contains("invalid.c"));
        Ok(())
    }

    /// Returns the rows of a CSV text, each one mapping the columns to the cells.
    fn records(csv: &str) -> Result<Vec<HashMap<String, String>>> {
        let mut reader = ::csv::Reader::from_reader(csv.as_bytes());
        let header = reader.headers()?.clone();
        reader
            .records()
            .map(|record| {
                Ok(header
                    .iter()
                    .map(str::to_string)
                    .zip(record?.iter().map(str::to_string))
                    .collect())
            })
            .collect()
    }

    /// Returns the row whose column has a value.
    fn find<'a>(
        rows: &'a [HashMap<String, String>],
        column: &str,
        value: &str,
    ) -> Result<&'a HashMap<String, String>> {
        rows.iter()
            .find(|row| row[column] == value)
            .with_context(|| format!("No row with {column} {value}"))
    }

    #[test]
    fn github_files_have_links() -> Result<()> {
        let input_file_path = format!("{TEST_DATA}/github.csv");
        write_file(
            &input_file_path,
            b"repo_id,source,repo_name,repo_license,latest_commit,local_path,file\n\
              7,github,owner/repo,mit,abc1234,tests/data/phases/parse/weird.go,src/weird.go\n\
              8,github,owner/other,,,tests/data/phases/parse/several_functions.go,several_functions.go\n",
        )?;
        let result = parse_with_policy(
            &input_file_path,
            &["tests/data/keywords/fp_types.json"],
            "ignore",
        );
        delete_file(&input_file_path, false)?;
        let (functions, logs) = result?;
        let functions = records(&functions)?;
        let logs = records(&logs)?;

        // With a commit, the links point to the commit
        let function = find(&functions, FUNCTION_NAME, "polarToCartesian")?;
        assert_eq!(
            function[URL],
            "https://github.com/owner/repo/blob/abc1234/src/weird.go#L9"
        );
        assert_eq!((&*function[LINE], &*function[COLUMN]), ("9", "1"));
        assert_eq!(function[RepoColumn::RepoLicense.name()], "mit");
        assert_eq!(
            find(&logs, FILE, "src/weird.go")?[URL],
            "https://github.com/owner/repo/blob/abc1234/src/weird.go"
        );

        // Without a commit, the links point to the default branch
        assert_eq!(
            find(&functions, FUNCTION_NAME, "sumFloats")?[URL],
            "https://github.com/owner/other/blob/HEAD/several_functions.go#L18"
        );
        assert_eq!(
            find(&logs, FILE, "several_functions.go")?[URL],
            "https://github.com/owner/other/blob/HEAD/several_functions.go"
        );
        Ok(())
    }

    #[test]
    fn source_is_required_unless_inferred() -> Result<()> {
        let input_file_path = format!("{TEST_DATA}/no_source.csv");
        write_file(
            &input_file_path,
            b"id,repo_name,path,file\n7,owner/repo,tests/data/phases/parse/weird.go,weird.go\n",
        )?;
        let keywords = ["tests/data/keywords/fp_types.json"];
        let renames = [
            ("id".to_string(), RepoColumn::RepoId.name().to_string()),
            ("path".to_string(), LOCAL_PATH.to_string()),
        ];
        let without_inference =
            parse_with_options(&input_file_path, &keywords, "ignore", false, &renames);
        let with_inference =
            parse_with_options(&input_file_path, &keywords, "ignore", true, &renames);
        delete_file(&input_file_path, false)?;

        ensure!(without_inference.is_err());
        let functions = records(&with_inference?.0)?;
        let function = find(&functions, FUNCTION_NAME, "polarToCartesian")?;
        assert_eq!(function[RepoColumn::Source.name()], "github");
        assert_eq!(
            function[URL],
            "https://github.com/owner/repo/blob/HEAD/weird.go#L9"
        );
        Ok(())
    }

    #[test]
    fn invalid_file() -> Result<()> {
        let keywords = vec!["tests/data/keywords/c_float.json"];

        let input_file_path = format!("{TEST_DATA}/invalid.csv");

        test_parse(&input_file_path, &keywords, false, true, true, true)
    }

    #[test]
    fn empty() -> Result<()> {
        let keywords = vec!["tests/data/keywords/scala_float.json"];

        let input_file_path = format!("{TEST_DATA}/empty.csv");

        test_parse(&input_file_path, &keywords, false, true, true, true)
    }

    #[test]
    fn ignore_comments_go() -> Result<()> {
        let keywords = vec![
            "tests/data/keywords/fp_types.json",
            "tests/data/keywords/fp_transcendental.json",
            "tests/data/keywords/fp_others.json",
        ];

        let input_file_path = format!("{TEST_DATA}/fn_comments_go.csv");

        test_parse(&input_file_path, &keywords, true, true, true, true)
    }

    #[test]
    fn parse_go_count() -> Result<()> {
        let keywords = vec![
            "tests/data/keywords/fp_types.json",
            "tests/data/keywords/fp_transcendental.json",
            "tests/data/keywords/fp_others.json",
        ];

        let input_file_path = format!("{TEST_DATA}/parse_go_count.csv");

        test_parse(&input_file_path, &keywords, false, true, false, true)
    }
}
