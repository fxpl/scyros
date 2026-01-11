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

use super::error::*;
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
pub fn open_json_from_path(path: &str) -> Result<JsonValue, Error> {
    map_err(
        json::parse(&map_err(
            std::fs::read_to_string(path),
            &format!("Could not read {}", path),
        )?),
        &format!("Could not parse {}", path),
    )
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
    type Output;
    fn parse(json: JsonValue) -> Result<Self::Output, Error>;
}

impl FromJson for i32 {
    type Output = i32;
    fn parse(json: JsonValue) -> Result<i32, Error> {
        ok_or_else(json.as_i32(), &format!("Could not parse {} as i32", json))
    }
}

impl FromJson for i64 {
    type Output = i64;
    fn parse(json: JsonValue) -> Result<i64, Error> {
        ok_or_else(json.as_i64(), &format!("Could not parse {} as i64", json))
    }
}

impl FromJson for u32 {
    type Output = u32;
    fn parse(json: JsonValue) -> Result<u32, Error> {
        ok_or_else(json.as_u32(), &format!("Could not parse {} as u32", json))
    }
}

impl FromJson for u64 {
    type Output = u64;
    fn parse(json: JsonValue) -> Result<u64, Error> {
        ok_or_else(json.as_u64(), &format!("Could not parse {} as u64", json))
    }
}

impl FromJson for String {
    type Output = String;
    fn parse(json: JsonValue) -> Result<String, Error> {
        ok_or_else(
            json.as_str().map(|s| s.to_owned()),
            &format!("Could not parse {} as String", json),
        )
    }
}

impl FromJson for bool {
    type Output = bool;
    fn parse(json: JsonValue) -> Result<bool, Error> {
        ok_or_else(json.as_bool(), &format!("Could not parse {} as bool", json))
    }
}

pub fn field_is_null(json: &JsonValue, key: &str) -> Result<bool, Error> {
    if json.is_null() {
        return Error::new("Cannot get field from null json").to_res();
    }
    if !json.has_key(key) {
        Error::new(&format!("Value {} does not have {} field", json, key)).to_res()
    } else {
        Ok(json[key].is_null())
    }
}

pub fn get_field<T: FromJson>(json: &JsonValue, key: &str) -> Result<T::Output, Error> {
    if json.is_null() {
        return Error::new("Cannot get field from null json").to_res();
    }
    if !json.has_key(key) {
        Error::new(&format!("Value {} does not have {} field", json, key)).to_res()
    } else {
        T::parse(json[key].clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_json_from_path() {
        assert!(open_json_from_path("tests/data/keywords/fp_others.json").is_ok());
        assert!(open_json_from_path("tests/data/keywords/nonexistent.json").is_err());
        assert!(open_json_from_path("tests/data/small_file.csv").is_err());
    }

    #[test]
    fn test_json_to_set() {
        let json = json::parse(r#"["a", "b", "c"]"#).unwrap();
        let set = json_to_set(&json);
        assert_eq!(set.len(), 3);
        assert!(set.contains("a"));
        assert!(set.contains("b"));
        assert!(set.contains("c"));
    }
}
