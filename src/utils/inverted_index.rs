use crate::utils::bow::Word;
use std::collections::HashMap;

/// Inverted index data structure mapping tokens in a global corpus to the functions they appear in, along with the count of occurrences and positional information.
pub struct InvertedIndex<'a> {
    map: HashMap<Word, Vec<(&'a str, usize, usize, usize)>>, // token -> Vec<(function_path, count, token_position, cumulative_count)}
}

impl<'a> Default for InvertedIndex<'a> {
    fn default() -> Self {
        InvertedIndex::new()
    }
}

impl<'a> InvertedIndex<'a> {
    pub fn new() -> Self {
        InvertedIndex {
            map: HashMap::default(),
        }
    }

    pub fn add(
        &mut self,
        token: &Word,
        function_path: &'a str,
        count: usize,
        token_position: usize,
        cumulative_count: usize,
    ) {
        //token_position is the index of the token.
        // cumulative_count is the number of words seen up to and including this token including duplicates
        self.map.entry(token.to_owned()).or_default().push((
            function_path,
            count,
            token_position,
            cumulative_count,
        ));
    }

    pub fn get(&self, token: &Vec<u8>) -> Option<&Vec<(&'a str, usize, usize, usize)>> {
        self.map.get(token)
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn len_tokens(&self) -> usize {
        self.map.values().map(|v| v.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn token_frequency(&self, token: &Vec<u8>, count_duplicates: bool) -> usize {
        if let Some(functions) = self.get(token) {
            if count_duplicates {
                functions.iter().map(|(_, count, _, _)| *count).sum()
            } else {
                functions.len()
            }
        } else {
            0
        }
    }
}
