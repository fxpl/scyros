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

use crate::utils::error::*;
use polars::frame::DataFrame;

pub fn i32(df: &DataFrame, column: &str) -> Result<Vec<i32>, Error> {
    let i32_col = map_err(
        map_err(
            df.column(column),
            &format!("Could not find column {}", column),
        )?
        .i32(),
        "Could not convert column to 32 bits integers",
    )?;
    Ok(i32_col.into_no_null_iter().collect())
}

pub fn u32(df: &DataFrame, column: &str) -> Result<Vec<u32>, Error> {
    let u32_col = map_err(
        map_err(
            df.column(column),
            &format!("Could not find column {}", column),
        )?
        .u32(),
        "Could not convert column to 32 bits unsigned integers",
    )?;
    Ok(u32_col.into_no_null_iter().collect())
}

pub fn has_column(df: &DataFrame, column: &str) -> bool {
    df.get_column_names()
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<&str>>()
        .contains(&column)
}
