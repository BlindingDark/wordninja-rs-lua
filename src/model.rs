//! Extend wordninja 0.1.0 without modifying the dependency or copying its vocabulary.
//!
//! The splitting implementation is adapted from wordninja 0.1.0, released under
//! the MIT license. Copyright (c) 2017 Derek Anderson, Harry Cheng.
//! See the repository LICENSE for the original license text.

use lazy_static::lazy_static;
use std::collections::HashMap;
use std::{borrow::Cow, cmp::min};

lazy_static! {
    /// The default language model from Word Ninja.
    pub static ref DEFAULT_MODEL: LanguageModel<'static> =
        LanguageModel::new_model(include_str!(env!("WORDNINJA_PRESET_PATH")));
}

/// A Word Ninja language model.
#[derive(Debug, Clone)]
pub struct LanguageModel<'s> {
    // Borrow embedded vocabulary, but own user-added words without leaking memory.
    wordcost: HashMap<Cow<'s, str>, f64>,
    maxword: usize,
}

impl<'s> LanguageModel<'s> {
    /// Creates a language model from a list of words.
    pub fn new(words: &[&'s str]) -> Self {
        let count = words.len();
        let count_ln = (count as f64).ln();
        let mut wordcost = HashMap::with_capacity(count);
        let mut maxword = 0;

        for (i, word) in words.iter().enumerate() {
            // Build a cost dictionary, assuming Zipf's law and cost = -math.log(probability).
            let cost = (((i + 1) as f64) * count_ln).ln();
            wordcost.insert(Cow::Borrowed(*word), cost);

            if word.len() > maxword {
                maxword = word.len();
            }
        }

        Self { wordcost, maxword }
    }
    /// Inserts or replaces a word's cost without changing other scores.
    ///
    /// The caller must normalize the word to ASCII lowercase and supply a positive,
    /// finite cost. Splitting recognizes ASCII letters, digits, and apostrophes.
    pub fn set_word_cost(&mut self, word: String, cost: f64) {
        self.maxword = self.maxword.max(word.len());
        self.wordcost.insert(Cow::Owned(word), cost);
    }
    /// Returns a word's cost using an exact, case-sensitive lookup.
    pub fn word_cost(&self, word: &str) -> Option<f64> {
        self.wordcost.get(word).copied()
    }
    /// Creates a language model from a list of words separated by newlines, empty lines are ignored.
    pub fn new_model(model: &'s str) -> Self {
        let words: Vec<&str> = model.lines().filter(|s| !s.is_empty()).collect();
        Self::new(&words)
    }
    /// Splits a string of concatenated words.
    pub fn split<'a>(&self, s: &'a str) -> Vec<Cow<'a, str>> {
        s.split(|c: char| !(c.is_ascii_alphanumeric() || c == '\''))
            .filter(|s| !s.is_empty())
            .flat_map(|s| self.split_part(s))
            .collect()
    }

    fn split_part<'a>(&self, s: &'a str) -> impl Iterator<Item = Cow<'a, str>> {
        // Build the cost array.
        let mut cost = Vec::with_capacity(s.len() + 1);
        cost.push(0.0);
        for i in 1..s.len() + 1 {
            let (c, _) = self.best_match(s, &cost, i);
            cost.push(c);
        }

        // Backtrack to recover the minimal-cost string.
        let mut out: Vec<Cow<str>> = vec![];
        let mut i = s.len();

        while i > 0 {
            let (c, k) = self.best_match(s, &cost, i);
            assert!((c - cost[i]).abs() < f64::EPSILON);
            // Apostrophe and digit handling (added by Genesys)
            let mut new_token = true;
            let token = &s[i - k..i];

            if token != "'" {
                // ignore a lone apostrophe
                if !out.is_empty() {
                    // re-attach split 's and split digits
                    let last_out = out.iter_mut().last().unwrap();
                    let is_s = *last_out == "'s";
                    // digit followed by digit
                    let digit_follow = s.chars().nth(i - 1).unwrap().is_ascii_digit()
                        && last_out.chars().next().unwrap().is_ascii_digit();
                    if is_s || digit_follow {
                        *last_out = [token, last_out].concat().into();
                        new_token = false;
                    }
                }
            }
            // (End of Genesys addition)

            if new_token {
                out.push(token.into());
            }

            i -= k;
        }

        out.into_iter().rev()
    }

    fn best_match(&self, s: &str, cost: &[f64], i: usize) -> (f64, usize) {
        let back = min(self.maxword, i);

        cost[i - back..i]
            .iter()
            .rev()
            .enumerate()
            .map(|(k, c)| {
                let lower = s[i - k - 1..i].to_ascii_lowercase();
                let match_cost = c + self.wordcost.get(&lower[..]).unwrap_or(&f64::MAX);
                (match_cost, k + 1)
            })
            .min_by(|(x, _), (y, _)| x.partial_cmp(y).expect("Tried to compare a NaN"))
            .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple() {
        assert_eq!(
            DEFAULT_MODEL.split("derekanderson"),
            vec!["derek", "anderson"]
        );
        assert_eq!(
            DEFAULT_MODEL.split("DEREKANDERSON"),
            vec!["DEREK", "ANDERSON"]
        );
    }

    #[test]
    fn with_underscores_etc() {
        let cmp = vec!["derek", "anderson"];
        assert_eq!(DEFAULT_MODEL.split("derek anderson"), cmp);
        assert_eq!(DEFAULT_MODEL.split("derek-anderson"), cmp);
        assert_eq!(DEFAULT_MODEL.split("derek_anderson"), cmp);
        assert_eq!(DEFAULT_MODEL.split("derek/anderson"), cmp);
    }

    #[test]
    fn digits() {
        assert_eq!(
            DEFAULT_MODEL.split("win32intel"),
            vec!["win", "32", "intel"]
        );
    }

    #[test]
    fn apostrophes() {
        assert_eq!(
            DEFAULT_MODEL.split("that'sthesheriff'sbadge"),
            vec!["that's", "the", "sheriff's", "badge"]
        );
    }

    #[test]
    fn custom() {
        let model = LanguageModel::new_model("der\nek\n");
        assert_eq!(model.split("derek"), vec!["der", "ek"]);
    }

    #[test]
    fn owned_words_outlive_their_source_and_update_maxword() {
        let mut model = LanguageModel::new(&["a", "b"]);
        {
            let word = String::from("customvocabulary");
            model.set_word_cost(word, 1.0);
        }
        assert_eq!(model.word_cost("customvocabulary"), Some(1.0));
        assert_eq!(model.split("customvocabulary"), vec!["customvocabulary"]);
        assert_eq!(model.split("CUSTOMVOCABULARY"), vec!["CUSTOMVOCABULARY"]);
    }

    #[test]
    fn extending_a_clone_preserves_the_preset_and_other_scores() {
        let original = LanguageModel::new(&["a", "b"]);
        let mut model = original.clone();
        let original_a = original.word_cost("a");
        model.set_word_cost(String::from("b"), 3.0);
        model.set_word_cost(String::from("ab"), 1.0);
        assert_eq!(model.word_cost("a"), original_a);
        assert_eq!(model.word_cost("b"), Some(3.0));
        assert_eq!(model.word_cost("missing"), None);
        assert_eq!(original.word_cost("ab"), None);
        assert_ne!(original.word_cost("b"), model.word_cost("b"));
        assert_eq!(original.split("ab"), vec!["a", "b"]);
        assert_eq!(model.split("ab"), vec!["ab"]);
    }

    #[test]
    fn preset_matches_the_unmodified_dependency() {
        // The dependency is the oracle, not a second checked-in vocabulary or
        // expected-output snapshot. Exercise all entries, plus mixed boundaries.
        for word in include_str!(env!("WORDNINJA_PRESET_PATH")).lines() {
            assert_eq!(
                DEFAULT_MODEL.split(word),
                wordninja::DEFAULT_MODEL.split(word)
            );
        }
        for input in [
            "iloverust",
            "thisisatest",
            "useserde",
            "DEREKANDERSON",
            "that'sthesheriff'sbadge",
            "win32intel",
            "foo_bar/baz-qux",
            "中文useVecDeque中文",
            "wethepeopleoftheunitedstatesinordertoformamoreperfectunionestablishjustice",
        ] {
            assert_eq!(
                DEFAULT_MODEL.split(input),
                wordninja::DEFAULT_MODEL.split(input)
            );
        }
    }
}
