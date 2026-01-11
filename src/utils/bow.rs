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

//! Simple Bag of Words (BoW) implementation for counting token occurrences.

use std::collections::HashMap;

/// Bag of Words (BoW) structure for counting token occurrences.
/// BoW are invariant to the order of insertion. All operations assume tokens are in byte slice form.
pub struct Bow {
    /// Internal map storing token counts.
    map: HashMap<Vec<u8>, usize>,
}

impl Default for Bow {
    fn default() -> Self {
        Bow::new()
    }
}

impl Bow {
    /// Creates a new, empty Bag of Words.
    pub fn new() -> Self {
        Bow {
            map: HashMap::new(),
        }
    }

    /// Adds a token to the Bag of Words
    ///
    /// # Arguments
    ///
    /// * `token` - The token to be added in byte slice form.
    pub fn add(&mut self, token: &[u8]) {
        *self.map.entry(token.to_owned()).or_insert(0) += 1;
    }

    /// Retrieves the frequency of a token in the Bag of Words
    ///
    /// # Arguments
    ///
    /// * `token` - The token whose frequency is to be retrieved in byte slice form.
    pub fn freq(&self, token: &[u8]) -> usize {
        *self.map.get(token).unwrap_or(&0)
    }

    /// Adds multiple tokens to the Bag of Words
    ///
    /// # Arguments
    ///
    /// * `tokens` - A collection of tokens to be added
    pub fn add_all<I>(&mut self, tokens: I)
    where
        I: IntoIterator,
        I::Item: AsRef<[u8]>,
    {
        for token in tokens {
            self.add(token.as_ref());
        }
    }

    /// Serializes the Bag of Words into a byte vector. The result is invariant to the order of insertion.
    pub fn serialize(self) -> Vec<u8> {
        let mut ordered_bow: Vec<(Vec<u8>, usize)> = self.map.into_iter().collect();
        ordered_bow.sort_by(|a, b| a.0.cmp(&b.0));
        ordered_bow
            .into_iter()
            .map(|(word, count)| format!("{}:{}", String::from_utf8_lossy(&word), count))
            .collect::<Vec<_>>()
            .join("|")
            .into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let bow = Bow::new();
        assert_eq!(bow.map.len(), 0);
        assert_eq!(bow.freq(b"test"), 0);
    }

    #[test]
    fn test_add_and_freq() {
        let mut bow = Bow::new();
        bow.add(b"hello");
        bow.add(b"hello");
        assert_eq!(bow.freq(b"hello"), 2);
        assert_eq!(bow.freq(b"Hello"), 0);
    }

    #[test]
    fn test_add_all() {
        let mut bow = Bow::new();
        let tokens = vec![b"foo", b"foo", b"bar"];
        bow.add_all(tokens);
        assert_eq!(bow.freq(b"foo"), 2);
        assert_eq!(bow.freq(b"bar"), 1);
        assert_eq!(bow.freq(b"Bar"), 0);
    }

    #[test]
    fn test_serialize() {
        let mut bow1 = Bow::new();
        bow1.add(b"apple");
        bow1.add(b"banana");
        bow1.add(b"apple");

        let mut bow2 = Bow::new();
        bow2.add(b"banana");
        bow2.add(b"apple");
        bow2.add(b"apple");

        let serialized1 = bow1.serialize();
        let serialized2 = bow2.serialize();
        assert_eq!(serialized1, serialized2);
    }
}
