use super::error::*;
use super::fs::*;
use super::json::*;
use curl::easy::{Easy, List as CurlList};
use json::JsonValue;
use polars::prelude::{DataType, Field, Schema};
use std::iter::FromIterator as _;

/// Checks if a file is a valid GitHub token file.
///
/// A valid GitHub token file is a CSV file with a header "token" and that contains at least one token.
/// Every line in the file must contain exactly one token.
///
/// # Arguments
///
/// * `file_path` - The token file
pub fn is_valid_token_file(file_path: &str) -> Result<(), Error> {
    let token_file = map_err(
        open_csv(
            file_path,
            Some(Schema::from_iter(vec![Field::new(
                "token".into(),
                DataType::String,
            )])),
            Some(vec!["token"]),
        ),
        &format!("Invalid token file {}", file_path),
    )?;

    if token_file.height() == 0 {
        Error::new("Token file is empty").to_res()
    } else {
        // Safe unwrap
        for (i, token) in token_file
            .column("token")
            .unwrap()
            .str()
            .unwrap()
            .iter()
            .enumerate()
        {
            let mut headers: CurlList = CurlList::new();

            let mut easy = Easy::new();

            map_err(
                easy.url("https://api.github.com").and_then(|_| {
                    easy.get(true)
                        .and_then(|_| {
                            headers.append(&format!("Authorization: token {}", token.unwrap()))
                        })
                        .and_then(|_| headers.append("User-Agent: Rust-curl"))
                        .and_then(|_| easy.http_headers(headers))
                }),
                "Could not make curl request",
            )?;

            if easy.perform().is_err() || easy.response_code().unwrap() != 200 {
                Error::new(&format!("Token in line {} is invalid", i + 2)).to_res()?;
            }
        }
        Ok(())
    }
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

/// Objects that can be created from GitHub JSON responses and converted to CSV rows.
// TODO: Tests
pub trait FromGitHub: ToCSV {
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
    fn parse_date_time(json: &JsonValue, field: &str) -> Result<i64, Error> {
        Ok(map_err(
            chrono::NaiveDateTime::parse_from_str(
                &get_field::<String>(json, field)?,
                "%Y-%m-%dT%H:%M:%SZ",
            ),
            &format!("Could not parse {} field epoch as a DateTime", field),
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

#[cfg(test)]
mod tests {

    use super::*;
    use std::path::Path;

    #[test]
    fn valid_tokens() {
        let token_path = Path::new("ghtokens.csv");
        if token_path.exists() {
            assert!(is_valid_token_file(token_path.to_str().unwrap()).is_ok());
        }
    }

    #[test]
    fn invalid_three_token_file() {
        assert!(is_valid_token_file("tests/data/dummy_tokens.csv").is_err());
    }

    #[test]
    fn invalid_non_existent_file() {
        assert!(is_valid_token_file("tests/data/non_existent.csv").is_err());
    }

    #[test]
    fn invalid_empty_file() {
        assert!(is_valid_token_file("tests/data/empty.csv").is_err());
    }

    #[test]
    fn invalid_title() {
        assert!(is_valid_token_file("tests/data/invalid_token_title.csv").is_err());
    }

    #[test]
    fn invalid_title_only_file() {
        assert!(is_valid_token_file("tests/data/token_title_only.csv").is_err());
    }

    #[test]
    fn two_token_same_line() {
        assert!(is_valid_token_file("tests/data/two_tokens_same_line.csv").is_err());
    }

    #[test]
    fn invalid_file() {
        assert!(is_valid_token_file("tests/data/invalid_csv.csv").is_err());
    }
}
