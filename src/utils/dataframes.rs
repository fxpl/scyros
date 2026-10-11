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

//! Utility functions for working with DataFrames.

use anyhow::{bail, ensure, Context, Result};
use polars::frame::DataFrame;
use polars::prelude::Column;

/// Returns the column with the given name, or an error if it does not exist or contains null values.
///
/// # Arguments
/// * `df` - The DataFrame containing the column.
/// * `column` - The name of the column.
fn column_without_nulls<'a>(df: &'a DataFrame, column: &str) -> Result<&'a Column> {
    let col: &Column = df.column(column)?;
    ensure!(
        col.null_count() == 0,
        "Column {column} contains {} null values",
        col.null_count()
    );
    Ok(col)
}

/// Extracts a column of 32 bits integers from a DataFrame and returns it as a vector. The column must not contain null values.
///
/// # Arguments
/// * `df` - The DataFrame containing the column.
/// * `column` - The name of the column to extract.
///
/// # Returns
/// A vector containing the values of the column, or an error if the column does not exist, cannot be converted to 32 bits integers, or contains null values.
pub fn i32(df: &DataFrame, column: &str) -> Result<Vec<i32>> {
    let i32_col = column_without_nulls(df, column)?
        .i32()
        .with_context(|| format!("Could not convert column {column} to 32 bits integers"))?;
    Ok(i32_col.into_no_null_iter().collect())
}

/// Extracts a column of 32 bits unsigned integers from a DataFrame and returns it as a vector. The column must not contain null values.
///
/// # Arguments
/// * `df` - The DataFrame containing the column.
/// * `column` - The name of the column to extract.
///
/// # Returns
/// A vector containing the values of the column, or an error if the column does not exist, cannot be converted to 32 bits unsigned integers, or contains null values.
pub fn u32(df: &DataFrame, column: &str) -> Result<Vec<u32>> {
    let u32_col = column_without_nulls(df, column)?.u32().with_context(|| {
        format!("Could not convert column {column} to 32 bits unsigned integers")
    })?;
    Ok(u32_col.into_no_null_iter().collect())
}

/// Extracts a column of strings from a DataFrame and returns it as a vector. The column must not contain null values.
///
/// # Arguments
/// * `df` - The DataFrame containing the column.
/// * `column` - The name of the column to extract.
///
/// # Returns
/// A vector containing the values of the column, or an error if the column does not exist, cannot be converted to strings, or contains null values.
pub fn str<'a>(df: &'a DataFrame, column: &str) -> Result<Vec<&'a str>> {
    let str_col = column_without_nulls(df, column)?
        .str()
        .with_context(|| format!("Could not convert column {column} to strings"))?;
    Ok(str_col.no_null_iter().collect())
}

/// Checks if a DataFrame contains all the specified columns.
///
/// # Arguments
/// * `df` - The DataFrame to check.
/// * `columns` - An iterable of column names to check for.
pub fn has_columns<'a>(df: &DataFrame, columns: impl IntoIterator<Item = &'a str>) -> bool {
    let df_columns: Vec<&str> = df.get_column_names().iter().map(|s| s.as_str()).collect();
    columns.into_iter().all(|col| df_columns.contains(&col))
}

/// Checks if a DataFrame contains a column with a given name.
///
/// # Arguments
/// * `df` - The DataFrame to check.
/// * `column` - The name of the column to check for.
pub fn has_column(df: &DataFrame, column: &str) -> bool {
    has_columns(df, [column])
}

/// Returns an error if a DataFrame read from a file does not have some columns.
/// The error lists the missing columns and the columns of the file, and explains how to rename them.
///
/// # Arguments
/// * `df` - The DataFrame.
/// * `path` - The path to the file, for the error message.
/// * `required` - The columns the DataFrame must have.
pub fn require_columns(df: &DataFrame, path: &str, required: &[&str]) -> Result<()> {
    let missing: Vec<&str> = required
        .iter()
        .copied()
        .filter(|column| !has_column(df, column))
        .collect();
    let Some(first) = missing.first() else {
        return Ok(());
    };
    let columns: Vec<&str> = df.get_column_names().iter().map(|s| s.as_str()).collect();
    bail!(
        "{path} has no {} {}.\n\
         The command reads the {} {}, and the file has the columns {}.\n\
         Rename the columns of the file, or read them under other names with --rename OLD=NEW, \
         for example --rename <column of the file>={first}.",
        plural("column", missing.len()),
        missing.join(", "),
        plural("column", required.len()),
        required.join(", "),
        columns.join(", ")
    )
}

/// Returns a word followed by an `s` if there are several things.
///
/// # Arguments
/// * `word` - The word.
/// * `count` - The number of things.
fn plural(word: &str, count: usize) -> String {
    if count == 1 {
        word.to_string()
    } else {
        format!("{word}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use polars::df;

    #[test]
    fn null_values_are_rejected() -> Result<()> {
        let df = df!(
            "id" => [Some(1u32), None],
            "name" => [Some("a"), None],
        )?;
        ensure!(u32(&df, "id").is_err());
        ensure!(str(&df, "name").is_err());

        let df = df!("id" => [1u32, 2], "name" => ["a", "b"])?;
        assert_eq!(u32(&df, "id")?, vec![1, 2]);
        assert_eq!(str(&df, "name")?, vec!["a", "b"]);
        Ok(())
    }

    #[test]
    fn missing_columns_are_explained() -> Result<()> {
        let df = df!("id" => [1u32], "name" => ["a"])?;
        require_columns(&df, "in.csv", &["id"])?;
        let one = require_columns(&df, "in.csv", &["repo_id"])
            .unwrap_err()
            .to_string();
        ensure!(one.contains("in.csv has no column repo_id."));
        ensure!(one.contains("reads the column repo_id, and the file has the columns id, name"));
        ensure!(one.contains("--rename <column of the file>=repo_id"));
        let two = require_columns(&df, "in.csv", &["repo_id", "source"])
            .unwrap_err()
            .to_string();
        ensure!(two.contains("has no columns repo_id, source."));
        Ok(())
    }
}
