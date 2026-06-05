use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};

pub struct CandidateEntry {
    pub matches: usize,
    pub length: usize,
    pub last_token_seen_pos: usize,
    pub last_token_seen_cumul_count: usize, // the number of words seen up to and including the last token seen for this candidate
}

pub struct CandidateMap<'a> {
    entries: HashMap<&'a str, CandidateEntry>,
    match_histogram: HashMap<usize, HashSet<&'a str>>,
    pending_updates: Vec<(&'a str, usize, usize, usize)>, // (function, new_matches, last_token_seen_pos)
    min_length: usize,
    max_length: usize,
}

impl<'a> Default for CandidateMap<'a> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> CandidateMap<'a> {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            match_histogram: HashMap::new(),
            min_length: usize::MAX,
            max_length: 0,
            pending_updates: Vec::new(),
        }
    }

    pub fn get_token_matches(&self, function: &str) -> usize {
        self.entries
            .get(function)
            .map(|entry| entry.matches)
            .unwrap_or(0)
    }

    pub fn add_pending_update(
        &mut self,
        function: &'a str,
        new_matches: usize,
        last_token_seen_pos: usize,
        last_token_seen_cumul_count: usize,
    ) {
        self.pending_updates.push((
            function,
            new_matches,
            last_token_seen_pos,
            last_token_seen_cumul_count,
        ));
    }

    pub fn apply_pending_updates(&mut self, function_lengths: &HashMap<&str, usize>) {
        let updates = self.pending_updates.drain(..).collect::<Vec<_>>();
        for (function, new_matches, last_token_seen_pos, last_token_seen_cumul_count) in updates {
            self.add_candidate(
                function,
                function_lengths,
                new_matches,
                last_token_seen_pos,
                last_token_seen_cumul_count,
            );
        }
    }

    pub fn add_candidate(
        &mut self,
        function: &'a str,
        function_lengths: &std::collections::HashMap<&str, usize>,
        new_matches: usize,
        last_token_seen_pos: usize,
        last_token_seen_cumul_count: usize,
    ) {
        let entry = match self.entries.entry(function) {
            Entry::Occupied(occupied) => occupied.into_mut(),
            Entry::Vacant(vacant) => {
                let length = function_lengths.get(function).copied().unwrap_or(0);
                let last_token_seen_pos = 0; // Initialize to 0 for new candidates
                let last_token_seen_cumul_count = 0; // Initialize to 0 for new candidates
                self.min_length = self.min_length.min(length);
                self.max_length = self.max_length.max(length);
                vacant.insert(CandidateEntry {
                    matches: 0,
                    length,
                    last_token_seen_pos,
                    last_token_seen_cumul_count,
                })
            }
        };

        // Update the match histogram
        if entry.matches > 0 {
            if let Some(bucket) = self.match_histogram.get_mut(&entry.matches) {
                bucket.remove(&function);
            }
        }

        entry.matches += new_matches;
        entry.last_token_seen_pos = last_token_seen_pos;
        entry.last_token_seen_cumul_count = last_token_seen_cumul_count;
        self.match_histogram
            .entry(entry.matches)
            .or_default()
            .insert(function);
    }

    pub fn length_range(&self) -> Option<(usize, usize)> {
        if self.entries.is_empty() {
            None
        } else {
            Some((self.min_length, self.max_length))
        }
    }

    pub fn get_candidates_with_n_matches(&self, n: usize, mode: &str) -> HashSet<&'a str> {
        if mode == "exact" {
            self.match_histogram.get(&n).cloned().unwrap_or_default()
        } else if mode == "at_least" {
            self.match_histogram
                .iter()
                .filter(|(&matches, _)| matches >= n)
                .flat_map(|(_, bucket)| bucket.clone())
                .collect()
        } else {
            panic!("Invalid mode: {}", mode);
        }
    }

    pub fn get_last_token_seen_pos(&self, function: &str) -> (usize, usize) {
        self.entries
            .get(function)
            .map(|entry| (entry.last_token_seen_pos, entry.last_token_seen_cumul_count))
            .unwrap_or((0, 0))
    }

    pub fn update_last_token_seen_pos(
        &mut self,
        function: &str,
        new_pos: usize,
        new_cumul_count: usize,
    ) {
        if let Some(entry) = self.entries.get_mut(function) {
            entry.last_token_seen_pos = new_pos;
            entry.last_token_seen_cumul_count = new_cumul_count;
        }
    }

    pub fn count_candidates_with_n_matches(&self, n: usize, mode: &str) -> usize {
        if mode == "exact" {
            self.match_histogram
                .get(&n)
                .map(|bucket| bucket.len())
                .unwrap_or(0)
        } else if mode == "at_least" {
            self.match_histogram
                .iter()
                .filter(|(&matches, _)| matches >= n)
                .map(|(_, bucket)| bucket.len())
                .sum()
        } else {
            panic!("Invalid mode: {}", mode);
        }
    }

    pub fn verification_cost_estimate(&self, n: usize, origin_word_count: &usize) -> usize {
        let mut number_of_candidates = self.count_candidates_with_n_matches(n, "at_least"); //the candidates that have already reached n matches

        let mut survivors = 0usize;
        for candidate in &self.pending_updates {
            let function = candidate.0;
            let current_matches = self.get_token_matches(function);
            if n > 1 && current_matches == n - 1 {
                // if n==1 the pending list is empty as they have already been applied
                survivors += 1;
            }
        }
        number_of_candidates += survivors; //add the candidates that are about to reach n matches
                                           // I am disregarding the candidates with less than n-1 matches that will also reach n_matches due to new_matches>1
                                           // But as I understand it they should always satisfy property 1
                                           // A candidate doesn't get to come back after being eliminated once
                                           // Also it's a very rare edge case
        let length_range = self.length_range().unwrap_or((usize::MAX, 0));
        let average_length = if length_range.0 == usize::MAX {
            0
        } else {
            (length_range.0 + length_range.1) / 2
        };
        number_of_candidates * (*origin_word_count + average_length)
    }
}
