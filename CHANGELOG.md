# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- A `--version` flag that prints the version information of the program.
- The `--debug` flag prints library debug information in the logs. Additional debug information has been added to the `download` subcommand, including the number of threads spawned and the regexes used for keyword matching.
- A `--ignore-comments` flag for the `parse` subcommand that sets the parser to ignore comments when extracting functions in individual source files.
- A `--order` flag for every subcommand that allow users to choose whether to process the rows of the input CSV file in sequential order or in random order. By default, rows are processed in random order to minimize the impact of any ordering bias in the input data. 

### Fixed

- In the `download` subcommand, the github tokens are now optional when the `--skip` flag is used.

### Changed

- The `download` subcommand does not produce a column `id`, `latest_commit` and `name` in the output logs when the `--skip` flag is used.

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

