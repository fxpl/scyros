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

use anyhow::{ensure, Context, Result};
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
    Ok(str_col.into_no_null_iter().collect())
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
}
