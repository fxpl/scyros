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

//pub mod alt_parse;
pub mod download;
pub mod duplicate_files;
pub mod duplicate_ids;
pub mod extract_benchmarks;
pub mod filter_languages;
pub mod filter_metadata;
pub mod forks;
pub mod ids;
pub mod languages;
pub mod latest_commit;
pub mod metadata;
pub mod parse;
pub mod pull_request;

use clap::{Arg, ArgAction, ArgMatches};

/// Returns the `--rename` flag of the commands reading an input file.
pub fn rename_arg() -> Arg {
    Arg::new("rename")
        .long("rename")
        .value_name("OLD=NEW")
        .help(
            "Read the column OLD of the input file as the column NEW. Can be given several times. \
             The other options and the output files use the new names.",
        )
        .value_parser(parse_rename)
        .action(ArgAction::Append)
}

/// Returns the values of the `--rename` flag, as (old name, new name) pairs.
///
/// # Arguments
///
/// * `matches` - The parsed arguments of the command.
pub fn renames(matches: &ArgMatches) -> Vec<(String, String)> {
    matches
        .get_many::<(String, String)>("rename")
        .into_iter()
        .flatten()
        .cloned()
        .collect()
}

/// Parses a value of the `--rename` flag, of the form `OLD=NEW`.
///
/// # Arguments
///
/// * `value` - The value.
fn parse_rename(value: &str) -> Result<(String, String), String> {
    match value.split_once('=') {
        Some((old, new)) if !old.is_empty() && !new.is_empty() => {
            Ok((old.to_string(), new.to_string()))
        }
        _ => Err(format!("'{value}' is not of the form OLD=NEW")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renames_are_parsed() {
        assert_eq!(
            parse_rename("id=repo_id"),
            Ok(("id".to_string(), "repo_id".to_string()))
        );
        assert!(parse_rename("id").is_err());
        assert!(parse_rename("=repo_id").is_err());
        assert!(parse_rename("id=").is_err());
    }
}
