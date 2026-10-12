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

#![doc = include_str!("../docs/errors.md")]

use anyhow::{bail, Context, Result};
use polars::prelude::{DataFrame, DataType, Field};
use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use tracing::warn;

/// Documentation of the codes, with their explanations.
pub const ERRORS_DOC: &str = include_str!("../docs/errors.md");

/// Column with the error code of a row, empty when the row has no error.
pub const ERROR: &str = "error";

/// Returns the field of the error column, to read it from a CSV file.
pub fn error_field() -> Field {
    Field::new(ERROR.into(), DataType::String)
}

/// Returns the cell of the error column of a row: the code, or empty without error.
///
/// # Arguments
///
/// * `error` - The error of the row, if any.
pub fn error_cell(error: Option<ErrorCode>) -> String {
    error.map(|code| code.to_string()).unwrap_or_default()
}

/// Why a step could not process a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    /// A request got no answer from GitHub.
    Network,
    /// GitHub answered, but the answer could not be read.
    InvalidAnswer,
    /// The repository is not on GitHub.
    UnsupportedSource,
    /// The default branch of the repository has no commit.
    NoCommit,
    /// The archive of the repository could not be extracted.
    ExtractionFailed,
    /// The file or the function was skipped because of a parse error.
    ParseError,
    /// The file is too large to be parsed.
    TooLarge,
    /// GitHub answered with an HTTP error status.
    Http(u16),
}

impl ErrorCode {
    /// The codes explained in the documentation, with one HTTP status per described group.
    pub const DOCUMENTED: [ErrorCode; 13] = [
        ErrorCode::Network,
        ErrorCode::InvalidAnswer,
        ErrorCode::UnsupportedSource,
        ErrorCode::NoCommit,
        ErrorCode::ExtractionFailed,
        ErrorCode::ParseError,
        ErrorCode::TooLarge,
        ErrorCode::Http(401),
        ErrorCode::Http(403),
        ErrorCode::Http(404),
        ErrorCode::Http(409),
        ErrorCode::Http(451),
        ErrorCode::Http(500),
    ];

    /// Returns the number of the code: the HTTP status for HTTP errors.
    pub fn number(&self) -> u16 {
        match self {
            ErrorCode::Network => 1,
            ErrorCode::InvalidAnswer => 2,
            ErrorCode::UnsupportedSource => 10,
            ErrorCode::NoCommit => 11,
            ErrorCode::ExtractionFailed => 20,
            ErrorCode::ParseError => 30,
            ErrorCode::TooLarge => 31,
            ErrorCode::Http(status) => *status,
        }
    }

    /// Returns a short description of the code, printed next to it in the terminal.
    pub fn description(&self) -> &'static str {
        match self {
            ErrorCode::Network => "network failure",
            ErrorCode::InvalidAnswer => "invalid answer",
            ErrorCode::UnsupportedSource => "unsupported source",
            ErrorCode::NoCommit => "no commit",
            ErrorCode::ExtractionFailed => "extraction failed",
            ErrorCode::ParseError => "parse error",
            ErrorCode::TooLarge => "file too large",
            ErrorCode::Http(401) => "bad credentials",
            ErrorCode::Http(403) => "forbidden",
            ErrorCode::Http(404) => "not found",
            ErrorCode::Http(409) => "empty repository",
            ErrorCode::Http(451) => "unavailable for legal reasons",
            ErrorCode::Http(500..=599) => "server error",
            ErrorCode::Http(_) => "HTTP error",
        }
    }

    /// Returns the explanation of the code: its code, its description, and its explanation in the documentation.
    pub fn explanation(&self) -> String {
        let explanation: String = match self.documentation() {
            Some(explanation) => explanation.to_string(),
            None => format!("GitHub answered with the HTTP status {}.", self.number()),
        };
        format!("{self} {}: {explanation}", self.description())
    }

    /// Returns the explanation of the code in the documentation of the codes, if any.
    fn documentation(&self) -> Option<&'static str> {
        // Server errors share one explanation, under E5xx.
        let code: String = match self {
            ErrorCode::Http(500..=599) => "E5xx".to_string(),
            _ => self.to_string(),
        };
        let prefix: String = format!("  * {code} {}: ", self.description());
        ERRORS_DOC
            .lines()
            .find_map(|line| line.strip_prefix(prefix.as_str()))
    }
}

impl ErrorCode {
    /// Reads the error of a row of a dataframe, in the `error` column.
    /// A row has no error if the column is missing or its cell is empty.
    ///
    /// # Arguments
    ///
    /// * `df` - The dataframe.
    /// * `row` - The index of the row.
    pub fn from_csv_row(df: &DataFrame, row: usize) -> Result<Option<Self>> {
        let Ok(column) = df.column(ERROR) else {
            return Ok(None);
        };
        column
            .str()
            .with_context(|| format!("Column {ERROR} does not contain strings"))?
            .get(row)
            .filter(|cell| !cell.is_empty())
            .map(str::parse)
            .transpose()
            .with_context(|| format!("Could not read the {ERROR} of row {row}"))
    }
}

/// An error of a row: its code, and details printed in the terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowError {
    /// The code written in the error column.
    pub code: ErrorCode,
    /// Details on the error, such as the message of a network error. Empty if there are none.
    pub detail: String,
}

impl RowError {
    /// Returns an error with details.
    ///
    /// # Arguments
    ///
    /// * `code` - The code of the error.
    /// * `detail` - Details on the error.
    pub fn new(code: ErrorCode, detail: impl fmt::Display) -> Self {
        Self {
            code,
            detail: detail.to_string(),
        }
    }
}

impl From<ErrorCode> for RowError {
    fn from(code: ErrorCode) -> Self {
        Self {
            code,
            detail: String::new(),
        }
    }
}

impl fmt::Display for RowError {
    /// Writes the code, its description, and the details if any, such as `E001 network failure (timeout)`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.code, self.code.description())?;
        if !self.detail.is_empty() {
            write!(f, " ({})", self.detail)?;
        }
        Ok(())
    }
}

impl std::error::Error for RowError {}

/// Counts the errors of the rows written to an output file, to print a summary at the end of a step.
pub struct ErrorSummary {
    /// The index of the error column in the rows.
    column: usize,
    /// The number of rows with each code.
    counts: HashMap<ErrorCode, usize>,
}

impl ErrorSummary {
    /// Returns an empty summary of the rows of an output file.
    ///
    /// # Arguments
    ///
    /// * `header` - The header of the output file, with an error column.
    pub fn new(header: &[&str]) -> Result<Self> {
        let column: usize = header
            .iter()
            .position(|c| *c == ERROR)
            .with_context(|| format!("The output has no {ERROR} column"))?;
        Ok(Self {
            column,
            counts: HashMap::new(),
        })
    }

    /// Counts the error of a row written to the output file, if it has one.
    ///
    /// # Arguments
    ///
    /// * `row` - The row.
    pub fn add(&mut self, row: &[String]) -> Result<()> {
        if let Some(cell) = row.get(self.column).filter(|cell| !cell.is_empty()) {
            *self.counts.entry(cell.parse()?).or_default() += 1;
        }
        Ok(())
    }

    /// Prints the number of rows with each code, the most frequent first, if there are errors.
    pub fn log(&self) {
        let mut counts: Vec<(&ErrorCode, &usize)> = self.counts.iter().collect();
        counts.sort_by(|(code_a, count_a), (code_b, count_b)| {
            count_b
                .cmp(count_a)
                .then(code_a.number().cmp(&code_b.number()))
        });
        if !counts.is_empty() {
            warn!(
                "{} rows with an error: {}. Run scyros explain <CODE> for details, and use --debug to print every error.",
                counts.iter().map(|(_, count)| **count).sum::<usize>(),
                counts
                    .iter()
                    .map(|(code, count)| format!("{code} {} x{count}", code.description()))
                    .collect::<Vec<String>>()
                    .join(", ")
            );
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "E{:03}", self.number())
    }
}

impl FromStr for ErrorCode {
    type Err = anyhow::Error;

    /// Reads a code, such as `E404`. The `E` can be omitted.
    fn from_str(s: &str) -> Result<Self> {
        let digits: &str = s.strip_prefix(['E', 'e']).unwrap_or(s);
        let number: u16 = digits
            .parse()
            .with_context(|| format!("{s} is not an error code, such as E404"))?;
        Ok(match number {
            1 => ErrorCode::Network,
            2 => ErrorCode::InvalidAnswer,
            10 => ErrorCode::UnsupportedSource,
            11 => ErrorCode::NoCommit,
            20 => ErrorCode::ExtractionFailed,
            30 => ErrorCode::ParseError,
            31 => ErrorCode::TooLarge,
            400..=599 => ErrorCode::Http(number),
            _ => bail!("{s} is not an error code. The codes are listed in `scyros explain --help`"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::ensure;

    #[test]
    fn codes_are_written_and_read() -> Result<()> {
        assert_eq!(ErrorCode::Network.to_string(), "E001");
        assert_eq!(ErrorCode::Http(404).to_string(), "E404");
        for code in ErrorCode::DOCUMENTED {
            assert_eq!(code.to_string().parse::<ErrorCode>()?, code);
        }
        assert_eq!("404".parse::<ErrorCode>()?, ErrorCode::Http(404));
        assert_eq!("e31".parse::<ErrorCode>()?, ErrorCode::TooLarge);
        assert!("E003".parse::<ErrorCode>().is_err());
        assert!("E200".parse::<ErrorCode>().is_err());
        assert!("not found".parse::<ErrorCode>().is_err());
        Ok(())
    }

    /// Every code is explained in the documentation, and the documentation explains no other code.
    #[test]
    fn documentation_matches_the_codes() {
        for code in ErrorCode::DOCUMENTED {
            assert!(code.documentation().is_some(), "{code} is not documented");
        }
        let documented_lines: usize = ERRORS_DOC
            .lines()
            .filter(|line| line.starts_with("  * E"))
            .count();
        assert_eq!(documented_lines, ErrorCode::DOCUMENTED.len());
    }

    #[test]
    fn errors_are_read_from_rows() -> Result<()> {
        let df = polars::df!(ERROR => [Some("E404"), Some(""), None])?;
        assert_eq!(ErrorCode::from_csv_row(&df, 0)?, Some(ErrorCode::Http(404)));
        assert_eq!(ErrorCode::from_csv_row(&df, 1)?, None);
        assert_eq!(ErrorCode::from_csv_row(&df, 2)?, None);
        let without_column = polars::df!("other" => ["E404"])?;
        assert_eq!(ErrorCode::from_csv_row(&without_column, 0)?, None);
        Ok(())
    }

    #[test]
    fn row_errors_are_written_with_their_details() {
        assert_eq!(
            RowError::new(ErrorCode::Network, "timeout").to_string(),
            "E001 network failure (timeout)"
        );
        assert_eq!(
            RowError::from(ErrorCode::Http(404)).to_string(),
            "E404 not found"
        );
    }

    #[test]
    fn summaries_count_the_codes() -> Result<()> {
        let mut summary = ErrorSummary::new(&["repo_id", ERROR])?;
        for cell in ["E404", "", "E404", "E001"] {
            summary.add(&["1".to_string(), cell.to_string()])?;
        }
        assert_eq!(
            summary.counts,
            HashMap::from([(ErrorCode::Http(404), 2), (ErrorCode::Network, 1)])
        );
        ensure!(ErrorSummary::new(&["repo_id"]).is_err());
        Ok(())
    }

    #[test]
    fn explanations() {
        assert_eq!(
            ErrorCode::Http(404).explanation(),
            "E404 not found: the repository was deleted, turned private, or never existed."
        );
        assert!(ErrorCode::Http(503)
            .explanation()
            .starts_with("E503 server error: GitHub failed to answer"));
        assert_eq!(
            ErrorCode::Http(418).explanation(),
            "E418 HTTP error: GitHub answered with the HTTP status 418."
        );
    }
}
