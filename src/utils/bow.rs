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

use anyhow::{Context, Result};

pub type Token = Vec<u8>;

/// Bag of Words (BoW) structure for counting token occurrences.
/// BoW are invariant to the order of insertion. All operations assume tokens are in byte slice form.
pub struct Bow {
    /// Internal map storing token frequencies.
    map: HashMap<Token, u32>,
    /// Whether to convert tokens to lower case before adding them to the bag of words.
    lowercase: bool,
}

impl Default for Bow {
    fn default() -> Self {
        Bow::new(false)
    }
}

impl Bow {
    /// Creates a new, empty Bag of Words.
    ///
    /// # Arguments
    ///
    /// * `lowercase` - Whether to convert tokens to lower case before adding them to the bag of words.
    pub fn new(lowercase: bool) -> Self {
        Bow {
            map: HashMap::new(),
            lowercase,
        }
    }

    /// Adds a token to the Bag of Words
    ///
    /// # Arguments
    ///
    /// * `token` - The token to be added in byte slice form.
    pub fn add(&mut self, token: &[u8]) {
        let token: Token = if self.lowercase {
            token
                .iter()
                .map(|b| b.to_ascii_lowercase())
                .collect::<Token>()
        } else {
            token.to_owned()
        };
        *self.map.entry(token).or_insert(0) += 1;
    }

    /// Retrieves the frequency of a token in the Bag of Words
    ///
    /// # Arguments
    ///
    /// * `token` - The token whose frequency is to be retrieved in byte slice form.
    pub fn freq(&self, token: &[u8]) -> u32 {
        let token: &[u8] = if self.lowercase {
            &token
                .iter()
                .map(|b| b.to_ascii_lowercase())
                .collect::<Token>()
        } else {
            token
        };
        *self.map.get(token).unwrap_or(&0)
    }

    /// Returns the total frequency of all tokens in the Bag of Words.
    pub fn sum(&self) -> u32 {
        self.map.values().sum()
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
        let mut ordered_bow: Vec<(Token, u32)> = self.map.into_iter().collect();
        ordered_bow.sort_by(|a, b| a.0.cmp(&b.0));
        ordered_bow
            .into_iter()
            .map(|(token, freq)| format!("{}:{}", String::from_utf8_lossy(&token), freq))
            .collect::<Vec<_>>()
            .join("|")
            .into_bytes()
    }

    /// Extends the Bag of Words with another Bag of Words, summing the frequencies of shared tokens.
    ///
    /// # Arguments
    ///
    /// * `other` - The other Bag of Words to be extended into this one.
    pub fn extend(&mut self, other: Bow) {
        for (token, freq) in other.map {
            *self.map.entry(token).or_insert(0) += freq;
        }
    }

    pub fn token_rankings(self) -> HashMap<Token, usize> {
        let mut freq_vec: Vec<(Token, u32)> = self.map.into_iter().collect();
        freq_vec.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
        freq_vec
            .into_iter()
            .enumerate()
            .map(|(rank, (token, _))| (token, rank))
            .collect()
    }

    pub fn sort_by<'a>(
        &self,
        token_rankings: &'a HashMap<Token, usize>,
    ) -> Result<Vec<(&'a Token, u32, u32)>> {
        let mut ranked: Vec<(usize, &'a Token, u32)> = self
            .map
            .iter()
            .map(|(token, freq)| {
                let (ranking_token, rank) =
                    token_rankings.get_key_value(token).with_context(|| {
                        format!(
                            "Token not found in rankings: {}",
                            String::from_utf8_lossy(token)
                        )
                    })?;
                Ok((*rank, ranking_token, *freq))
            })
            .collect::<Result<_>>()?;

        ranked.sort_unstable_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(b.1)));

        let mut cumulative = 0;
        Ok(ranked
            .into_iter()
            .map(|(_, token, freq)| {
                cumulative += freq;
                (token, freq, cumulative)
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let bow = Bow::new(false);
        assert_eq!(bow.map.len(), 0);
        assert_eq!(bow.freq(b"test"), 0);
    }

    #[test]
    fn test_add_and_freq() {
        let mut bow = Bow::new(false);
        bow.add(b"hello");
        bow.add(b"hello");
        assert_eq!(bow.freq(b"hello"), 2);
        assert_eq!(bow.freq(b"Hello"), 0);

        let mut bow_lower = Bow::new(true);
        bow_lower.add(b"Hello");
        assert_eq!(bow_lower.freq(b"hello"), 1);
        assert_eq!(bow_lower.freq(b"Hello"), 1);
    }

    #[test]
    fn test_add_all() {
        let mut bow = Bow::new(false);
        let tokens = vec![b"foo", b"foo", b"bar"];
        bow.add_all(tokens);
        assert_eq!(bow.freq(b"foo"), 2);
        assert_eq!(bow.freq(b"bar"), 1);
        assert_eq!(bow.freq(b"Bar"), 0);

        let mut bow_lower = Bow::new(true);
        let tokens = vec![b"Foo", b"foo", b"Bar"];
        bow_lower.add_all(tokens);
        assert_eq!(bow_lower.freq(b"foo"), 2);
        assert_eq!(bow_lower.freq(b"bar"), 1);
    }

    #[test]
    fn test_serialize() {
        let mut bow1 = Bow::new(false);
        bow1.add(b"apple");
        bow1.add(b"banana");
        bow1.add(b"apple");

        let mut bow2 = Bow::new(false);
        bow2.add(b"banana");
        bow2.add(b"apple");
        bow2.add(b"apple");

        let serialized1 = bow1.serialize();
        let serialized2 = bow2.serialize();
        assert_eq!(serialized1, serialized2);
    }
}
