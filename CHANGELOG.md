# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- A `--no-output` (or `--count`) flag for the `parse` subcommand that allows users to skip writing the extracted functions to disk and only collect their statistics ([#5](https://github.com/fxpl/scyros/issues/5), reported by [@Michago6](https://github.com/Michago6)).
- A `--lambdas` flag for the `parse` subcommand that allows users to choose whether to extract lambda functions as well. By default, lambda functions are not extracted ([#3](https://github.com/fxpl/scyros/issues/3), reported by [@
linusbrew](https://github.com/linusbrew)).
- An `overlap` similarity criterion for the `duplicate_files` subcommand, which detects near-miss duplicates, that is, files that differ by a few statements rather than matching exactly. Files are compared by how many tokens they have in common, using the prefix filtering technique of SourcererCC and its adaptive extension. The proportion of how many tokens two files must share to be considered duplicates can be set with the `--threshold` flag (defaults to 0.8).
The  `--prefix` (or `-p`) flag sets how far the prefix used to reject candidates may be deepened. A deeper prefix rejects more candidates before the full comparison, at the cost of more index lookups. Defaults to 1. The `--languages` (or `-l`) maps file extensions to languages, in the same JSON format used by the other subcommands. The `overlap` criterion compares files only against others of the same language, and leaves files whose extension belongs to no listed language uncompared. The other criteria ignore the flag. (PR [#2](https://github.com/fxpl/scyros/pull/2) by [@swartling](https://github.com/swartling))

### Changed

- The number of threads of the `download` subcommand is now given with `-n` or `--threads` instead of as a value without a flag. A number after `--keywords` was read as a keywords file.
- The `parse` and `duplicate_files` subcommands also accept `--threads` in addition to `-n`.

### Fixed

- An issue with the `--regex` flag in the `download` and `parse` subcommands that added word delimiters to the regexes when the flag was used (reported by [@Smexykex](https://github.com/Smexykex)).
- A parsing error in the `download` subcommand when the `--sub` flag was used ([#6](https://github.com/fxpl/scyros/issues/6), reported by [@Michago6](https://github.com/Michago6)).
- An issue with the `download` subcommand that caused it to not resume progress when restarted ([#7](https://github.com/fxpl/scyros/issues/7), reported by [@Smexykex](https://github.com/Smexykex)).
- An issue with the `download` subcommand that caused it to match extension prefixes instead of the full name ([#8](https://github.com/fxpl/scyros/issues/8), reported by [@Smexykex](https://github.com/Smexykex)).
- An issue with the `languages` and `metadata` subcommands that caused them to read the input file instead of the output file when resuming. A restarted run considered every project as done and stopped without processing any.
- An issue with the `pull_request` subcommand that caused it to fail when resuming if the `--ids` column was not named `id`.
- An issue with the `parse` subcommand that caused it to hang when using 0 threads. The `parse` and `download` subcommands now require at least one thread.
- An issue that caused no log message to be printed when the standard error is not a terminal, for example when it is redirected to a file or in a batch job.
- An issue with the `ids` subcommand in linear mode that caused it to ignore `--max` and to send requests forever after reaching the most recent repository when `-n` was used. Resuming from an output file containing only a header no longer fails.
- Several issues with the requests to the GitHub API used by the `ids`, `languages`, `metadata` and `pull_request` subcommands:
  - the status of the first response was used instead of the status of the final response after redirects;
  - secondary rate limits, responses with status 429, server errors and network errors were not retried, and were therefore written as error rows that are never queried again;
  - error responses were printed to the standard output;
  - every `%` in the responses was doubled, for example in the body of pull requests and comments.
- An issue with the `languages` and `metadata` subcommands that caused rows with too many columns when an error message contained a comma. Error rows of the `languages` subcommand also ended with a carriage return, which made them unusable as `--cache`.
- An issue with the `filter_languages` and `filter_metadata` subcommands that kept repositories that could not be queried when the error was not an HTTP/2 error, for example an HTTP/1.1 error.
- An issue with the `filter_metadata` subcommand that discarded repositories whose last push is earlier than their creation date, even with `--age 0`. Their age is now 0.
- An issue that caused input files to be read by column position instead of column name, so that the `languages`, `metadata`, `pull_request` and `download` subcommands failed or mixed up columns when the columns of the input file were in another order.
- An issue that caused GitHub tokens to be read from the first column of the tokens file instead of the `token` column.
- An issue that caused empty values in columns of ids, names or tokens to be read as 0 or as an empty string. They are now reported as errors. For example, a corrupted last row in the output of the `ids` subcommand made a resumed run sample ids again from the first request.
- An issue that caused a crash when a `keywords` or `extensions` field of a keywords JSON file contained a value that is not a string. Such files, and fields that are not arrays, are now reported as errors.
- An issue with the `languages` subcommand that wrote the languages of a repository in a different order in every run. They are now sorted by decreasing size, then by name.
- An issue that could lose the last rows of an output file without any error, for example when the disk is full. Output files are now flushed and write errors are reported.
- A crash of the `ids` subcommand in random mode when `--min` is not smaller than `--max`. It is now reported as an error.
- An issue with the `forks` subcommand that silently discarded entries without a value in the fork column and counted them as forks. They are now reported as errors.
- An issue that created the parent directories of a file when the file was only read, and that ignored the error when a directory had to be created where a file exists.
- An issue with the `download` subcommand that accepted a number of threads without `--skip`, although threads are only used with `--skip`.

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

