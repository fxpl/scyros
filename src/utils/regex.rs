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

//! A module that provides utilities for regex pattern matching in texts and files.

use crate::utils::bow::Bow;

use super::error::*;
use super::fs::*;
use super::json::*;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::io::BufRead;
use std::io::BufReader;

use regex::bytes::Regex;

/// A struct that holds a regex pattern and provides methods to find matches in texts and files.
#[derive(Debug)]
pub struct Matcher {
    /// The regex pattern to match against.
    /// If None, the matcher does not match anything.
    regex: Option<Regex>,
}

impl Matcher {
    /// Returns a matcher that finds wordsin a text.
    pub fn words_matcher() -> Self {
        Matcher {
            // Safe unwrap as the pattern is valid
            regex: Some(Regex::new(r"\b\w+\b").unwrap()),
        }
    }

    /// Returns an empty matcher that does not match anything.
    pub fn empty_matcher() -> Self {
        Matcher { regex: None }
    }

    /// Takes a sequence of keywords and returns a regex pattern that looks for any of them.
    ///
    /// # Arguments
    /// * `keywords` - A sequence of keywords to look for.
    /// * `case_sensitive` - Whether the search should be case sensitive.
    /// * `whole_words` - Whether substrings should be matched or only whole words.
    ///
    /// # Type Parameters
    /// * `T` - A type that can be converted to a string.
    /// * `I` - An iterable type that yields items of type `T`.
    ///
    ///  # Returns
    ///  A regex pattern looking for any of the keywords or an error if the pattern is invalid.
    pub fn keywords_matcher<I, T>(
        keywords: I,
        case_sensitive: bool,
        whole_words: bool,
    ) -> Result<Self, Error>
    where
        I: IntoIterator<Item = T>,
        T: ToString,
    {
        let joined_keywords = keywords
            .into_iter()
            .filter_map(|s| Some(s.to_string()).filter(|s| !s.is_empty()))
            .collect::<Vec<String>>()
            .join("|");
        if !joined_keywords.is_empty() {
            let new_pattern: String = if whole_words {
                format!(r"\b(?:{})\b", joined_keywords)
            } else {
                joined_keywords
            };

            let new_pattern_with_sensitivity: String = if case_sensitive {
                new_pattern
            } else {
                format!("(?i){}", new_pattern)
            };
            Ok(Self {
                regex: Some(map_err(
                    Regex::new(&new_pattern_with_sensitivity),
                    &format!("Invalid regex pattern: {}", new_pattern_with_sensitivity),
                )?),
            })
        } else {
            Ok(Self::words_matcher())
        }
    }

    /// Takes a sequence of sets of local keywords and a set of shared global keywords and returns a sequence of regex patterns
    /// that look for any of the local keywords or any of the global keywords.
    ///
    /// # Arguments
    ///
    /// * `local_keywords` - A map from values to sets of local keywords.
    /// * `global_keywords` - A set of globally shared keywords.
    /// * `case_sensitive` - A boolean indicating whether the search should be case sensitive.
    /// * `whole_words` - A boolean indicating whether the search should be for whole words.
    ///
    /// # Returns
    ///
    /// A map from values to regex patterns looking for both local and global keywords.
    pub fn keywords_matchers<T>(
        local_keywords: &HashMap<T, HashSet<String>>,
        global_keywords: &HashSet<String>,
        case_sensitive: bool,
        whole_words: bool,
    ) -> Result<HashMap<T, Matcher>, Error>
    where
        T: Eq + Hash + Clone,
    {
        let mut res = HashMap::<T, Matcher>::new();
        for (ext, kw) in local_keywords {
            let joined_keywords = Self::keywords_matcher(
                kw.iter().chain(global_keywords.iter()).cloned(),
                case_sensitive,
                whole_words,
            )?;
            res.insert(ext.clone(), joined_keywords);
        }

        Ok(res)
    }

    /// Counts the number of matches of a pattern in a text.
    ///
    /// # Arguments
    ///
    /// * `text` - The text to search for the pattern.
    pub fn count_matches_in_text(&self, text: &[u8]) -> usize {
        self.regex
            .as_ref()
            .map(|r| r.find_iter(text).count())
            .unwrap_or(0)
    }

    /// Checks if the matcher finds any matches in a text.
    ///
    /// # Arguments
    ///
    /// * `text` - The text to search for the pattern.
    pub fn has_matches_in_text(&self, text: &[u8]) -> bool {
        self.regex
            .as_ref()
            .map(|r| r.is_match(text))
            .unwrap_or(false)
    }

    /// Counts the number of matches of a pattern in a file.
    ///
    /// # Arguments
    ///
    /// * `path` - The path to the file to search for the pattern.
    pub fn count_matches_in_file(&self, path: &str) -> Result<usize, Error> {
        let mut count: usize = 0;
        for l in BufReader::new(open_file(path, FileMode::Read)?).lines() {
            let line = map_err(l, &format!("Could not read lines from {}", path))?;
            count += self.count_matches_in_text(line.as_bytes());
        }
        Ok(count)
    }

    /// Returns a bag of words (a map from words matching the pattern to their frequency) from a text.
    ///
    /// # Arguments
    ///
    /// * `text` - The text to analyze.
    pub fn bag_of_words(&self, text: &[u8]) -> Bow {
        let mut bow: Bow = Bow::new();
        if let Some(re) = &self.regex {
            bow.add_all(re.find_iter(text).map(|w| w.as_bytes()));
        }
        bow
    }
}

/// Counts the number of lines in a text.
///
/// # Arguments
///
/// * `text` - The text to count the lines of.
pub fn count_text_lines(text: &[u8]) -> usize {
    text.lines().count()
}

/// A structure representing a collection of files enumerating keywords to match against for different programming languages.
/// Programming languages are identified by their name, and a mapping from file extensions to programming languages is also provided.
///
/// A keyword file is a JSON file with the following structure:
/// ```json
/// {
///  "languages": [
///    {
///      "name": "LanguageName",
///      "extensions": [".ext1", ".ext2", ...],
///      "keywords": ["localKeyword1", "localKeyword2", ...]
///    },
///    ...
///  ]
///  "keywords": ["globalKeyword1", "globalKeyword2", ...]
/// }
/// ```
/// The "languages" field contains an array of programming languages, each with a name, a list of file extensions, and a list of local keywords, i.e.,
/// keywords to be matched only for that language. The "keywords" field contains a list of global keywords to be matched for all languages.
///
/// The matchers produced from this file will be the following regex patterns:
/// LanguageName -> [\blocalKeyword1\b|\blocalKeyword2\b|...|\bglobalKeyword1\b|\bglobalKeyword2\b|...] (case insensitive)
/// ...
///
/// Note that the keywords are matched as whole words but case insensitively.
/// Adding an other keyword file will add a new matcher for each language, in addition to the existing ones.
///
/// # Invariants:
/// * The size of the matchers vectors is equal to the number of paths
pub struct KeywordFiles {
    /// The paths to keyword-storing files
    pub paths: Vec<String>,
    /// The matchers for each programming language
    pub matchers: HashMap<String, Vec<Matcher>>,
    /// A mapping from file extensions to programming languages
    pub extensions_to_language: HashMap<String, String>,
}

impl Default for KeywordFiles {
    /// Creates a default, empty KeywordFiles instance.
    fn default() -> Self {
        KeywordFiles::new()
    }
}

impl KeywordFiles {
    /// Creates a new, empty KeywordFiles instance.
    pub fn new() -> KeywordFiles {
        KeywordFiles {
            paths: Vec::new(),
            matchers: HashMap::new(),
            extensions_to_language: HashMap::new(),
        }
    }

    /// Returns the number of keyword files in the collection
    pub fn len(&self) -> usize {
        self.paths.len()
    }

    /// Checks if there are no keyword files in the collection
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    /// Add several keyword files to the collection
    /// For each file, updates the matchers and the extensions to language map
    ///
    /// # Arguments
    ///
    /// * `paths` - The paths to the keyword files to add
    ///
    /// # Returns
    /// A new KeywordFiles instance with the added files or an error if any file could not
    /// be processed.
    pub fn add_files(self, paths: &[&str]) -> Result<KeywordFiles, Error> {
        if paths.is_empty() {
            Ok(self)
        } else {
            self.add_file(paths[0])?.add_files(&paths[1..])
        }
    }

    /// Add a keyword file to the collection
    /// Updates the matchers and the extensions to language map
    ///
    /// # Arguments
    ///
    /// * `path` - The path to the keyword file to add
    ///
    /// # Returns
    ///
    /// A new KeywordFiles instance with the added file or an error if the file could not
    /// be processed.
    pub fn add_file(self, path: &str) -> Result<KeywordFiles, Error> {
        // Add the argument to the list of paths
        let mut updated_paths: Vec<String> = self.paths.clone();
        updated_paths.push(path.to_string());

        // Opens the json file and load the top level fields
        let json = open_json_from_path(path)?;
        let categories = json_to_map(&json);

        let mut local_kw = HashMap::<String, HashSet<String>>::new();
        let mut extensions_to_language = self.extensions_to_language.clone();

        let cat1 = "languages";
        let languages = ok_or_else(
            categories.get(cat1),
            &format!("Keyword file {} does not contain a {} field", path, cat1),
        )?;

        for l in languages.members() {
            let language = json_to_map(l);

            let name = ok_or_else(
                language.get("name").and_then(|n| n.as_str()),
                &format!("Keyword file {} contains a language with no name", path),
            )?;

            let extensions = json_to_set(ok_or_else(
                language.get("extensions"),
                &format!("Language {} in {} has no extensions field", name, path),
            )?);

            let keywords = json_to_set(ok_or_else(
                language.get("keywords"),
                &format!("Language {} in {} has no keywords field", name, path),
            )?);

            for ext in extensions {
                match extensions_to_language.get(&ext) {
                    Some(value) if value != name => {
                        Error::new(&format!(
                            "Extension {} is associated with both {} and {} when loading {}",
                            &ext,
                            value,
                            name,
                            updated_paths.join(", ")
                        ))
                        .to_res()?;
                    }
                    None => {
                        extensions_to_language.insert(ext.clone(), name.to_string());
                    }
                    _ => (),
                }
                extensions_to_language.insert(ext, name.to_string());
            }
            local_kw.insert(name.to_string(), keywords.clone());
        }

        let cat2 = "keywords";
        let global_kw = json_to_set(ok_or_else(
            categories.get(cat2),
            &format!("Keyword file {} does not contain a {} field", path, cat2),
        )?);

        let file_matchers = Matcher::keywords_matchers(&local_kw, &global_kw, false, true)?;
        let mut updated_matchers = self.matchers;

        for (lang, entry) in updated_matchers.iter_mut() {
            if !file_matchers.contains_key(lang) {
                entry.push(Matcher::empty_matcher());
            }
        }

        // When a new language is added, we add empty matchers for other files
        for (lang, matcher) in file_matchers {
            match updated_matchers.get_mut(&lang) {
                None => {
                    let mut empty_matchers = Vec::new();
                    for _ in 0..self.paths.len() {
                        empty_matchers.push(Matcher::empty_matcher());
                    }
                    empty_matchers.push(matcher);
                    updated_matchers.insert(lang.to_string(), empty_matchers);
                }
                Some(entry) => entry.push(matcher),
            }
        }

        Ok(KeywordFiles {
            paths: updated_paths,
            matchers: updated_matchers,
            extensions_to_language,
        })
    }

    /// Counts the number of matches for each matcher of a given language in a file.
    ///
    /// # Arguments
    /// * `lang` - The programming language whose matchers to use.
    /// * `path` - The path to the file to analyze.
    ///
    /// # Returns
    /// A vector containing the number of matches for each matcher of the given language or an error if the file could not be processed.
    pub fn count_matches_in_file(&self, lang: &str, path: &str) -> Result<Vec<usize>, Error> {
        match self.matchers.get(lang) {
            Some(m) => m.iter().map(|m| m.count_matches_in_file(path)).collect(),
            None => Ok(vec![0, self.paths.len()]),
        }
    }

    /// Counts the number of matches for each matcher of a given language in a text.
    ///
    /// # Arguments
    /// * `lang` - The programming language whose matchers to use.
    /// * `text` - The text to analyze.
    ///
    /// # Returns
    /// A vector containing the number of matches for each matcher of the given language in the text.
    pub fn count_matches_in_text(&self, lang: &str, text: &[u8]) -> Vec<usize> {
        match self.matchers.get(lang) {
            Some(m) => m.iter().map(|m| m.count_matches_in_text(text)).collect(),
            None => vec![0, self.paths.len()],
        }
    }

    /// Checks if any matcher of a given language finds matches in a text.
    ///
    /// # Arguments
    /// * `lang` - The programming language whose matchers to use.
    /// * `text` - The text to analyze.
    ///
    /// # Returns
    /// True if any matcher of the given language finds matches in the text, false otherwise.
    pub fn has_matches_in_text(&self, lang: &str, text: &[u8]) -> bool {
        match self.matchers.get(lang) {
            Some(v) => v.iter().any(|m| m.has_matches_in_text(text)),
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_matches_test() {
        let text = b"Parole, parole, parole, paroleParole parole_parole parole_Parole";

        let matcher_lower_unsensitive_whole =
            Matcher::keywords_matcher(["parole"], false, true).unwrap();
        let matcher_lower_unsensitive_part =
            Matcher::keywords_matcher(["parole"], false, false).unwrap();
        let matcher_lower_sensitive_whole =
            Matcher::keywords_matcher(["parole"], true, true).unwrap();
        let matcher_lower_sensitive_part =
            Matcher::keywords_matcher(["parole"], true, false).unwrap();
        let matcher_upper_unsensitive_whole =
            Matcher::keywords_matcher(["Parole"], false, true).unwrap();
        let matcher_upper_unsensitive_part =
            Matcher::keywords_matcher(["Parole"], false, false).unwrap();
        let matcher_upper_sensitive_whole =
            Matcher::keywords_matcher(["Parole"], true, true).unwrap();
        let matcher_upper_sensitive_part =
            Matcher::keywords_matcher(["Parole"], true, false).unwrap();

        assert_eq!(
            matcher_lower_unsensitive_whole.count_matches_in_text(text),
            3
        );
        assert_eq!(
            matcher_lower_unsensitive_part.count_matches_in_text(text),
            9
        );
        assert_eq!(matcher_lower_sensitive_whole.count_matches_in_text(text), 2);
        assert_eq!(matcher_lower_sensitive_part.count_matches_in_text(text), 6);
        assert_eq!(
            matcher_upper_unsensitive_whole.count_matches_in_text(text),
            3
        );
        assert_eq!(
            matcher_upper_unsensitive_part.count_matches_in_text(text),
            9
        );
        assert_eq!(matcher_upper_sensitive_whole.count_matches_in_text(text), 1);
        assert_eq!(matcher_upper_sensitive_part.count_matches_in_text(text), 3);
    }

    #[test]
    fn count_words_test() {
        let matcher = Matcher::words_matcher();
        assert_eq!(matcher.count_matches_in_text(b""), 0);
        assert_eq!(matcher.count_matches_in_text(b"word"), 1);
        assert_eq!(matcher.count_matches_in_text(b" word  word word "), 3);
        assert_eq!(matcher.count_matches_in_text(b"word\nword\nword"), 3);
        assert_eq!(matcher.count_matches_in_text(b"<word>"), 1);
    }

    #[test]
    fn count_text_lines_test() {
        assert_eq!(count_text_lines(b""), 0);
        assert_eq!(count_text_lines(b"word"), 1);
        assert_eq!(count_text_lines(b"word\nword\nword"), 3);
    }

    // #[test]
    // fn bag_of_words_test() {
    //     let matcher = Matcher::words_matcher();
    //     let text = b"word1 word2 word3,     word1 word2 Word2 (Word1_3);";
    //     let bow = matcher.bag_of_words(text);
    //     assert_eq!(bow.len(), 5);
    //     assert_eq!(bow.get(&b"word1"[..]).unwrap(), &2);
    //     assert_eq!(bow.get(&b"Word2"[..]).unwrap(), &1);
    //     assert_eq!(bow.get(&b"Word1_3"[..]).unwrap(), &1);
    //     assert_eq!(bow.get(&b"word2"[..]).unwrap(), &2);
    //     assert_eq!(bow.get(&b"word3"[..]).unwrap(), &1);
    // }

    #[test]
    fn keywords_patterns_test() {
        let local_keywords: HashMap<usize, HashSet<String>> = [
            (
                3,
                ["word1".to_string(), "word2".to_string()]
                    .iter()
                    .cloned()
                    .collect(),
            ),
            (
                6,
                ["word3".to_string(), "word4".to_string()]
                    .iter()
                    .cloned()
                    .collect(),
            ),
        ]
        .iter()
        .cloned()
        .collect();
        let global_keywords: HashSet<String> = ["word5".to_string(), "word6".to_string()]
            .iter()
            .cloned()
            .collect();
        let patterns =
            Matcher::keywords_matchers(&local_keywords, &global_keywords, false, true).unwrap();
        assert_eq!(patterns.len(), 2);

        let text = b"word1 word2 word3 word4 word5 word6";

        assert_eq!(patterns.get(&3).unwrap().count_matches_in_text(text), 4);
        assert_eq!(patterns.get(&6).unwrap().count_matches_in_text(text), 4);
    }
}
