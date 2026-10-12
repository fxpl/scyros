// Copyright 2026 Andrea Gilot
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

use crate::utils::dataframes;

use super::fs::*;
use super::json::*;
use anyhow::ensure;
use anyhow::{Context, Error, Result};
use curl::easy::{Easy, List as CurlList};
use json::JsonValue;
use polars::prelude::{DataFrame, DataType, Field, Schema};
use reqwest::Url;
use std::iter::FromIterator as _;
/// Reads the tokens of a GitHub token file and checks that each of them is accepted by the GitHub API.
///
/// A valid GitHub token file is a CSV file with a header "token" and that contains at least one token.
/// Every line in the file must contain exactly one token.
///
/// # Arguments
///
/// * `file_path` - The token file
///
/// # Returns
///
/// The tokens in the file, or an error if the file is invalid or one of the tokens is rejected.
pub fn read_tokens(file_path: &str) -> Result<Vec<String>> {
    let token_file: DataFrame = open_csv(
        file_path,
        Some(Schema::from_iter(vec![Field::new(
            "token".into(),
            DataType::String,
        )])),
        Some(vec!["token"]),
        &[],
    )
    .with_context(|| format!("Invalid token file {file_path}"))?;

    let tokens: Vec<String> = dataframes::str(&token_file, "token")?
        .into_iter()
        .map(String::from)
        .collect();

    ensure!(!tokens.is_empty(), "Token file is empty");

    for (i, token) in tokens.iter().enumerate() {
        let mut headers: CurlList = CurlList::new();

        let mut easy = Easy::new();

        easy.url("https://api.github.com").and_then(|_| {
            easy.get(true)
                .and_then(|_| headers.append(&format!("Authorization: token {token}")))
                .and_then(|_| headers.append("User-Agent: Rust-curl"))
                .and_then(|_| easy.http_headers(headers))
        })?;

        let perform = easy.perform();
        if perform.is_err() {
            perform.with_context(|| format!("Token in line {} is invalid", i + 2))?
        }
        ensure!(
            easy.response_code()? == 200,
            "Token in line {} is invalid: response code {}",
            i + 2,
            easy.response_code()?
        );
    }
    Ok(tokens)
}

/// Objects that can be converted to CSV rows.
// TODO: Tests
pub trait ToCSV: Default {
    type Key;
    /// Converts the object to a CSV row.
    fn to_csv(&self, key: Self::Key) -> String;

    /// Returns the CSV header for the object as a vector of strings.
    fn header() -> &'static [&'static str];
}

/// Objects that can be created from GitHub JSON responses.
// TODO: Tests
pub trait FromGitHub {
    type Complement;

    /// Parses a date time field from a JSON object and returns its epoch representation.
    ///
    /// # Arguments
    ///
    /// * `json` - The JSON object to parse.
    /// * `field` - The field name containing the date time string.
    ///
    /// # Returns
    ///
    /// The epoch representation of the date time or an error if the field could not be parsed.
    fn parse_date_time(json: &JsonValue, field: &str) -> Result<i64> {
        Ok(chrono::NaiveDateTime::parse_from_str(
            &get_field::<String>(json, field)?,
            "%Y-%m-%dT%H:%M:%SZ",
        )?
        .and_utc()
        .timestamp())
    }

    /// Parses a JSON object to create an instance of this object.
    ///
    /// # Arguments
    ///
    /// * `json` - The JSON object to parse.
    /// * `complement` - Additional data needed for parsing.
    fn parse_json(json: &json::JsonValue, complement: Self::Complement) -> Result<Self, Error>
    where
        Self: Sized;
}

/// Returns the GitHub link to a page. Each segment of its path is percent-encoded.
///
/// # Arguments
///
/// * `segments` - Segments of the path of the page.
pub fn github_url<'a>(segments: impl IntoIterator<Item = &'a str>) -> String {
    // Safe unwraps: the URL is valid and can be a base.
    let mut url: Url = Url::parse("https://github.com").unwrap();
    url.path_segments_mut().unwrap().extend(segments);
    url.into()
}

/// Returns whether a folder looks like the root folder of a GitHub zip archive.
/// Its name is `<owner>-<repository>-<commit>`, with the first 7 characters of the commit.
///
/// # Arguments
///
/// * `folder` - Name of the folder.
pub fn is_zipball_root(folder: &str) -> bool {
    folder.rsplit_once('-').is_some_and(|(_, commit)| {
        commit.len() >= 7 && commit.chars().all(|c| c.is_ascii_hexdigit())
    })
}

/// Returns the commit of a GitHub zip archive: GitHub writes its full SHA in the comment of the archive.
/// Returns `None` for other comments.
///
/// # Arguments
///
/// * `comment` - Comment of the archive.
pub fn zipball_commit(comment: &[u8]) -> Option<String> {
    let commit: &str = std::str::from_utf8(comment).ok()?.trim();
    (!commit.is_empty() && commit.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| commit.to_string())
}

#[cfg(test)]
mod tests {

    use super::*;
    use anyhow::ensure;
    use std::path::Path;

    #[test]
    fn github_urls() {
        assert_eq!(
            github_url(["owner", "repo", "blob", "abc", "my dir", "a#b?.c"]),
            "https://github.com/owner/repo/blob/abc/my%20dir/a%23b%3F.c"
        );
    }

    #[test]
    fn zipball_roots() {
        assert!(is_zipball_root("francesco-scrapper-recent-id-2-38d21aa"));
        assert!(!is_zipball_root("src"));
        assert!(!is_zipball_root("my-lib"));
        assert!(!is_zipball_root("release-1234"));
    }

    #[test]
    fn zipball_commits() {
        assert_eq!(
            zipball_commit(b"7fd1a60b01f91b314f59955a4e4d4e80d8edf11d").as_deref(),
            Some("7fd1a60b01f91b314f59955a4e4d4e80d8edf11d")
        );
        assert_eq!(zipball_commit(b""), None);
        assert_eq!(zipball_commit(b"made by hand"), None);
    }

    #[test]
    #[ignore = "requires network access and valid GitHub tokens"]
    fn valid_tokens() -> Result<()> {
        let token_path = Path::new("ghtokens.csv");
        ensure!(token_path.exists(), "Token file does not exist");
        read_tokens(
            token_path
                .to_str()
                .with_context(|| "Path is not valid unicode")?,
        )?;
        Ok(())
    }

    #[test]
    fn invalid_three_token_file() -> Result<()> {
        ensure!(read_tokens("tests/data/dummy_tokens.csv").is_err());
        Ok(())
    }

    #[test]
    fn invalid_non_existent_file() -> Result<()> {
        ensure!(read_tokens("tests/data/non_existent.csv").is_err());
        Ok(())
    }

    #[test]
    fn invalid_empty_file() -> Result<()> {
        ensure!(read_tokens("tests/data/empty.csv").is_err());
        Ok(())
    }

    #[test]
    fn invalid_title() -> Result<()> {
        ensure!(read_tokens("tests/data/invalid_token_title.csv").is_err());
        Ok(())
    }

    #[test]
    fn invalid_title_only_file() -> Result<()> {
        ensure!(read_tokens("tests/data/token_title_only.csv").is_err());
        Ok(())
    }

    #[test]
    fn two_token_same_line() -> Result<()> {
        ensure!(read_tokens("tests/data/two_tokens_same_line.csv").is_err());
        Ok(())
    }

    #[test]
    fn invalid_file() -> Result<()> {
        ensure!(read_tokens("tests/data/invalid_csv.csv").is_err());
        Ok(())
    }
}
