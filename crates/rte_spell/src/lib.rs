//! Spelling: words of prose, dictionary engines and suggestions.
//!
//! The front end ([`Checker`]) splits nothing itself; callers give it the
//! [`words`] of a text. It applies the user dictionary and ignored words,
//! caches verdicts and asks one engine ([`Speller`]): the built-in word list
//! ([`Builtin`]), a Hunspell dictionary ([`Hunspell`]) or another engine
//! (the Windows spell checker lives in `rte_platform`). Engines can differ
//! in their verdicts; the front end makes words and overrides the same.

mod builtin;
mod hunspell;
mod suggest;
mod user;
mod words;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::time::Instant;

pub use builtin::Builtin;
pub use hunspell::{Hunspell, MAX_HUNSPELL_BYTES};
pub use suggest::candidates;
pub use user::{USER_MAX_BYTES, USER_MAX_WORDS, encode_user, merge_user, parse_user, valid_word};
pub use words::{MAX_WORD_BYTES, Word, normalized, words};

/// A dictionary engine. Calls may block (a native engine cannot be
/// interrupted); `deadline` is checked between lookups where possible.
pub trait Speller: Send {
    fn name(&self) -> &'static str;
    fn check(&mut self, word: &str) -> Result<bool, String>;
    fn suggest(&mut self, word: &str, max: usize, deadline: Instant)
    -> Result<Vec<String>, String>;
}

/// Most cached verdicts, and the bytes of their words.
pub const CACHE_MAX_WORDS: usize = 65_536;
pub const CACHE_MAX_BYTES: usize = 4 * 1024 * 1024;

/// The front end of an engine: user words, ignored words and a cache.
pub struct Checker {
    engine: Box<dyn Speller>,
    user: HashSet<String>,
    ignored: HashSet<String>,
    cache: HashMap<String, bool>,
    cache_bytes: usize,
}

impl Checker {
    pub fn new(engine: Box<dyn Speller>) -> Checker {
        Checker {
            engine,
            user: HashSet::new(),
            ignored: HashSet::new(),
            cache: HashMap::new(),
            cache_bytes: 0,
        }
    }

    pub fn engine(&self) -> &'static str {
        self.engine.name()
    }

    /// Replaces the engine (after an engine error); verdicts are cleared.
    pub fn set_engine(&mut self, engine: Box<dyn Speller>) {
        self.engine = engine;
        self.clear_cache();
    }

    pub fn add_user(&mut self, word: &str) {
        self.user.insert(normalized(word).into_owned());
        self.clear_cache();
    }

    pub fn ignore(&mut self, word: &str) {
        self.ignored.insert(normalized(word).into_owned());
        self.clear_cache();
    }

    pub fn clear_cache(&mut self) {
        self.cache.clear();
        self.cache_bytes = 0;
    }

    pub fn cached(&self) -> usize {
        self.cache.len()
    }

    fn overridden(&self, word: &str) -> bool {
        let lower = || word.to_lowercase();
        [&self.user, &self.ignored]
            .iter()
            .any(|set| set.contains(word) || set.contains(&lower()))
    }

    /// The word is spelled correctly. Words with letters outside ASCII that
    /// the engine does not know are not reported (an English dictionary).
    pub fn is_correct(&mut self, word: &str) -> Result<bool, String> {
        let word = normalized(word);
        if let Some(&known) = self.cache.get(word.as_ref()) {
            return Ok(known);
        }
        let ok = self.overridden(&word) || self.engine.check(&word)? || !word.is_ascii();
        if self.cache.len() >= CACHE_MAX_WORDS || self.cache_bytes + word.len() > CACHE_MAX_BYTES {
            self.clear_cache();
        }
        self.cache_bytes += word.len();
        self.cache.insert(word.into_owned(), ok);
        Ok(ok)
    }

    pub fn suggest(
        &mut self,
        word: &str,
        max: usize,
        deadline: Instant,
    ) -> Result<Vec<String>, String> {
        self.engine.suggest(&normalized(word), max, deadline)
    }
}
