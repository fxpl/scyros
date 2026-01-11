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

use std::collections::HashMap;

pub fn parse_map(map: &str) -> HashMap<String, String> {
    map.split(';')
        .filter_map(|pair| {
            let mut parts = pair.splitn(2, ':');
            match (parts.next(), parts.next()) {
                (Some(k), Some(v)) => Some((k.to_string(), v.to_string())),
                _ => None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_parse_map() {
        let input = "key1:value1;key2:value2;key3:value3";
        let expected: HashMap<String, String> = [
            ("key1".to_string(), "value1".to_string()),
            ("key2".to_string(), "value2".to_string()),
            ("key3".to_string(), "value3".to_string()),
        ]
        .iter()
        .cloned()
        .collect();
        let result = parse_map(input);
        assert_eq!(result, expected);
    }
    #[test]
    fn test_parse_empty_map() {
        let input = "";
        let expected: HashMap<String, String> = HashMap::new();
        let result = parse_map(input);
        assert_eq!(result, expected);
    }
}
