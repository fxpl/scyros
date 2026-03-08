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

use anyhow::{Context, Result};
use polars::frame::DataFrame;

/// Extracts a column of 32 bits integers from a DataFrame and returns it as a vector. The column must not contain null values.
///
/// # Arguments
/// * `df` - The DataFrame containing the column.
/// * `column` - The name of the column to extract.
///
/// # Returns
/// A vector containing the values of the column, or an error if the column does not exist, cannot be converted to 32 bits integers, or contains null values.
pub fn i32(df: &DataFrame, column: &str) -> Result<Vec<i32>> {
    let i32_col = df
        .column(column)?
        .i32()
        .with_context(|| format!("Could not convert column {} to 32 bits integers", column))?;
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
    let u32_col = df.column(column)?.u32().with_context(|| {
        format!(
            "Could not convert column {} to 32 bits unsigned integers",
            column
        )
    })?;
    Ok(u32_col.into_no_null_iter().collect())
}

/// Checks if a DataFrame contains a column with a given name.
///
/// # Arguments
/// * `df` - The DataFrame to check.
/// * `column` - The name of the column to check for.
pub fn has_column(df: &DataFrame, column: &str) -> bool {
    df.get_column_names()
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<&str>>()
        .contains(&column)
}
