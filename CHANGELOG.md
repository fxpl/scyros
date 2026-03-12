# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

