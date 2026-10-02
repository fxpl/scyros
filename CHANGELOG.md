# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- A `--case-sensitive` flag for the `download` and `parse` subcommands. The documentation now states that file extensions are always case-sensitive ([#9](https://github.com/fxpl/scyros/issues/9), reported by [@Sparcraps](https://github.com/Sparcraps)).
- A `--no-output` (or `--count`) flag for the `parse` subcommand that allows users to skip writing the extracted functions to disk and only collect their statistics ([#5](https://github.com/fxpl/scyros/issues/5), reported by [@Michago6](https://github.com/Michago6)).
- A `--lambdas` flag for the `parse` subcommand that allows users to choose whether to extract lambda functions as well. By default, lambda functions are not extracted ([#3](https://github.com/fxpl/scyros/issues/3), reported by [@
linusbrew](https://github.com/linusbrew)).
- An `overlap` similarity criterion for the `duplicate_files` subcommand, which detects near-miss duplicates, that is, files that differ by a few statements rather than matching exactly. Files are compared by how many tokens they have in common, using the prefix filtering technique of SourcererCC and its adaptive extension. The proportion of how many tokens two files must share to be considered duplicates can be set with the `--threshold` flag (defaults to 0.8).
The  `--prefix` (or `-p`) flag sets how far the prefix used to reject candidates may be deepened. A deeper prefix rejects more candidates before the full comparison, at the cost of more index lookups. Defaults to 1. The `--languages` (or `-l`) maps file extensions to languages, in the same JSON format used by the other subcommands. The `overlap` criterion compares files only against others of the same language, and leaves files whose extension belongs to no listed language uncompared. The other criteria ignore the flag. (PR [#2](https://github.com/fxpl/scyros/pull/2) by [@swartling](https://github.com/swartling))

### Changed

- The `download` subcommand takes its number of threads with `-n` or `--threads` instead of as a value without a flag, and rejects it without `--skip`. The `parse` and `duplicate_files` subcommands also accept `--threads`.

### Fixed

- An issue with the `--regex` flag in the `download` and `parse` subcommands that added word delimiters to the regexes when the flag was used (reported by [@Smexykex](https://github.com/Smexykex)).
- A parsing error in the `download` subcommand when the `--sub` flag was used ([#6](https://github.com/fxpl/scyros/issues/6), reported by [@Michago6](https://github.com/Michago6)).
- An issue with the `download` subcommand that caused it to not resume progress when restarted ([#7](https://github.com/fxpl/scyros/issues/7), reported by [@Smexykex](https://github.com/Smexykex)).
- An issue with the `download` subcommand that caused it to match extension prefixes instead of the full name ([#8](https://github.com/fxpl/scyros/issues/8), reported by [@Smexykex](https://github.com/Smexykex)).
- Resuming: the `languages` and `metadata` subcommands read the input file instead of the output file and processed nothing, `pull_request` failed when `--ids` was not `id`, and `ids` failed on an output file with only a header.
- Requests to the GitHub API (`ids`, `languages`, `metadata`, `pull_request`): the final status after redirects is used, rate limits, server errors and network errors are retried instead of being written as error rows, `%` is no longer doubled, and nothing is printed to the standard output.
- Error rows of the `languages` and `metadata` subcommands no longer break the CSV format when the message contains a comma, and start with `http/` or `error:`. The `filter_languages` and `filter_metadata` subcommands discard both, instead of only HTTP/2 errors.
- The `download` subcommand retries failed downloads, stops all threads after an error instead of working on without logging, no longer hangs when a thread panics, and logs archives that cannot be extracted as error rows. Re-extracting over symbolic links and paths that are not valid UTF-8 no longer stop the run.
- Keyword matching (`download`, `parse`): keywords such as `c++` or `#include` now match as whole words, the longer of two overlapping keywords (`long double`, `long`) is always preferred, and an inline regex flag only applies to its own keyword. Results of earlier runs can differ. The Ada keyword `**` of the example file `fp_others.json` is fixed as well.
- The `ids` subcommand stops at `--max` or at the most recent repository in linear mode, and reports `--min` not smaller than `--max` instead of crashing.
- The `filter_metadata` subcommand no longer discards repositories whose last push is earlier than their creation date.
- The `languages` subcommand lists the languages of a repository in the same order in every run.
- The `forks` subcommand reports entries without a fork value instead of counting them as forks.
- The `duplicate_files` subcommand stops after the first error and requires at least one thread. With several threads, the file standing for a group of duplicates is now always its first file in the input instead of changing between runs. The duplicates map is no longer overwritten without `--force`, a path listed several times in the input counts as one file, and a `count` column of the input is replaced instead of producing `count_right`.
- The `parse` subcommand no longer hangs with 0 threads, skips files in languages it cannot parse instead of stopping the run (for example `.h` files mapped to `c_header`), and applies the `--failures` policies as described: `skip-file` and `skip-function` write rows with -1 for every statistic, and `abort` stops all threads.
- Input files are read by column name instead of position, and tokens are read from the `token` column instead of the first one.
- Empty values in id, name or token columns, and values that are not strings in keywords files, are reported as errors instead of being read as 0 or an empty string, or causing a crash.
- Log messages are printed when the standard error is not a terminal, for example in a batch job.
- Output files are flushed, so that write errors, for example on a full disk, are reported.
- Paths with mixed separators, such as `.\projects\/0/`, in the outputs ([#10](https://github.com/fxpl/scyros/issues/10), reported by [@Sparcraps](https://github.com/Sparcraps)).
- The `--cache` flag of the `languages` and `metadata` subcommands could assign the row of a project to another one.
- Reading a file no longer creates its parent directories, and creating a directory where a file exists is reported as an error.

### Removed

- The `extract_benchmarks` subcommand has been removed as it was too brittle.

## [0.3.2] - 2026-05-07

### Added

- A `--regex` flag for the `download` and `parse` subcommands that allows users to specify whether the keywords in the keywords JSON files should be interpreted as regular expressions or as whole words to match. By default, keywords are interpreted as whole words to match. (PR [#1](https://github.com/fxpl/scyros/pull/1) by [@Smexykex](https://github.com/Smexykex))

### Fixed

- An issue with the `--header` flag in the `duplicate_files` subcommand that did not produce any output when the specified header was different from 'name'. 

### Changed

- In `bow` similarity mode, the `duplicate_files` subcommand now computes the bag of words of the file content by converting all words to lower case.

## [0.3.1] - 2026-04-23

### Fixed

- Issue that prevented the Nix flake from working correctly.

## [0.3.0] - 2026-04-23

### Added

- A `--version` flag that prints the version information of the program.
- The `--debug` flag prints library debug information in the logs. Additional debug information has been added to the `download` subcommand, including the number of threads spawned and the regexes used for keyword matching.
- A `--ignore-comments` flag for the `parse` subcommand that sets the parser to ignore comments when extracting functions in individual source files.
- A `--order` flag for every subcommand that allow users to choose whether to process the rows of the input CSV file in sequential order or in random order. By default, rows are processed in random order to minimize the impact of any ordering bias in the input data. 
- A `--sub` flag for the `download` subcommand that allows users to specify the number of repositories to download. By default, all repositories in the input CSV file are downloaded.
- The `parse` subcommand now outputs a column `return_kw_match` that indicates whether the return type of a function matches any of the keywords present in the keywords JSON files.
- The `parse` subcommand now accepts Rust source files.

### Fixed

- In the `download` subcommand, the github tokens are now optional when the `--skip` flag is used.

### Changed

- The `download` subcommand does not produce a column `id`, `latest_commit` and `name` in the output logs when the `--skip` flag is used.
- The short flag for the `--threads` argument in the `download` subcommand has been removed to avoid confusion with the new `--sub` flag.

## [0.2.4] - 2026-03-14

### Added

- Nix flake for the project

### Fixed

- Wrong parameter passed to `filter_metadata` subcommand.

### Changed

- Bumped Rust version to 1.93 to accommodate Nix flake requirements.

## [0.2.3] - 2026-03-14

### Fixed

- Crash caused by Clang library linking issues.

## [0.2.2] - 2026-03-14

### Changed

- Bumped Rust version to 1.94

### Added

- GitHub releases produce binaries for Linux, macOS, and Windows on both x86_64 and arm64 architectures.

## [0.2.1] - 2026-03-13

### Changed

- Bumped Rust version to 1.88

## [0.2.0] - 2026-03-12


### Added

- The `duplicate_files` subcommand can now use any column of the input CSV file as the file path column (instead of the default 'name' column).
- The `parse` subcommand now saves the position of parse errors in the source file (instead of whether there was one).
- The `parse` subcommand now saves the position of extracted functions in the output file.
- A new subsubcommand for mining pull requests from a list of repositories: `pr`.
- Keywords and extensions fields are no longer required in the keywords JSON files for the `filter_languages`, `download`, and `parse` subcommands. 

### Changed

- Help documentation for every command is now more detailed and includes the expected format of the input and output files.
- Error messages are now more informative and include backtraces by default to facilitate debugging.
- Logging now clearly indicates what is an info message, a warning, or an error. 


### Fixed

- Made the `download` subcommand more robust to API errors and interruptions. 
- Metadata collection now correctly handles repositories with no primary programming language.

