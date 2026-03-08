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

use anyhow::{ensure, Context, Error, Result};
use json::JsonValue;
use std::collections::{HashMap, HashSet};

/// Opens a JSON file from a path.
///
/// # Arguments
///
/// * `path` - The path to the JSON file.
///
/// # Returns
///
/// The JSON value of the file or an error if the file could not be opened, read, or parsed.
pub fn open_json_from_path(path: &str) -> Result<JsonValue> {
    json::parse(&std::fs::read_to_string(path)?)
        .with_context(|| format!("Could not parse JSON file at path {}", path))
}

/// Converts a JSON array to a HashSet of strings.
///
/// # Arguments
///
/// * `json` - The JSON array to convert.
pub fn json_to_set(json: &JsonValue) -> HashSet<String> {
    let mut set = HashSet::<String>::new();
    json.members().for_each(|x| {
        set.insert(x.as_str().unwrap().to_owned());
    });
    set
}

pub fn json_to_map<'a>(json: &'a JsonValue) -> HashMap<String, &'a JsonValue> {
    let mut map = HashMap::<String, &'a JsonValue>::new();
    for (k, v) in json.entries() {
        map.insert(k.to_owned(), v);
    }
    map
}

pub trait FromJson {
    /// The output type of the parsing operation.
    type Output;
    /// Parses a JSON value to the target type.
    /// # Arguments
    /// * `json` - The JSON value to parse.
    /// # Returns
    /// The parsed value or an error if the JSON value cannot be parsed the target type.
    fn parse(json: JsonValue) -> Result<Self::Output>;
}

impl FromJson for i32 {
    type Output = i32;
    fn parse(json: JsonValue) -> Result<i32> {
        json.as_i32()
            .with_context(|| format!("Could not parse {} as i32", json))
    }
}

impl FromJson for i64 {
    type Output = i64;
    fn parse(json: JsonValue) -> Result<i64> {
        json.as_i64()
            .with_context(|| format!("Could not parse {} as i64", json))
    }
}

impl FromJson for u32 {
    type Output = u32;
    fn parse(json: JsonValue) -> Result<u32> {
        json.as_u32()
            .with_context(|| format!("Could not parse {} as u32", json))
    }
}

impl FromJson for u64 {
    type Output = u64;
    fn parse(json: JsonValue) -> Result<u64> {
        json.as_u64()
            .with_context(|| format!("Could not parse {} as u64", json))
    }
}

impl FromJson for String {
    type Output = String;
    fn parse(json: JsonValue) -> Result<String> {
        json.as_str()
            .map(|s| s.to_owned())
            .with_context(|| format!("Could not parse {} as String", json))
    }
}

impl FromJson for bool {
    type Output = bool;
    fn parse(json: JsonValue) -> Result<bool> {
        json.as_bool()
            .with_context(|| format!("Could not parse {} as bool", json))
    }
}

/// Checks if a field in a JSON object is null.
///
/// # Arguments
/// * `json` - The JSON object to check.
/// * `key` - The name of the field to check.
///
/// # Returns
/// A boolean indicating whether the field is null, or an error if the field does not exist or if the JSON object is null.
pub fn field_is_null(json: &JsonValue, key: &str) -> Result<bool> {
    ensure!(!json.is_null(), "Cannot get field from null json");
    ensure!(
        json.has_key(key),
        "Value {} does not have {} field",
        json,
        key
    );
    Ok(json[key].is_null())
}
/// Gets a field from a JSON object and parses it to a given type.
///
/// # Arguments
/// * `json` - The JSON object to get the field from.
/// * `key` - The name of the field to get.
///
/// # Returns
/// The value of the field parsed to the given type, or an error if the field does not exist, cannot be parsed to the given type, or if the JSON object is null.
pub fn get_field<T: FromJson>(json: &JsonValue, key: &str) -> Result<T::Output, Error> {
    ensure!(!json.is_null(), "Cannot get field from null json");
    ensure!(
        json.has_key(key),
        "Value {} does not have {} field",
        json,
        key
    );
    T::parse(json[key].clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_json_from_path() -> Result<()> {
        ensure!(open_json_from_path("tests/data/keywords/fp_others.json").is_ok());
        ensure!(open_json_from_path("tests/data/keywords/nonexistent.json").is_err());
        ensure!(open_json_from_path("tests/data/small_file.csv").is_err());
        Ok(())
    }

    #[test]
    fn test_json_to_set() -> Result<()> {
        let json = json::parse(r#"["a", "b", "c"]"#)?;
        let set = json_to_set(&json);
        assert_eq!(set.len(), 3);
        ensure!(set.contains("a"));
        ensure!(set.contains("b"));
        ensure!(set.contains("c"));
        Ok(())
    }
}
