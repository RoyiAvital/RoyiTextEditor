//! The built-in engine: the en_US word list expanded at build time.

use std::time::Instant;

use crate::Speller;
use crate::suggest::candidates;

static WORDS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/en_US.words"));
static NO_SUGGEST: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/en_US.nosuggest"));

/// A sorted word list (`\n` after every word) with its line starts.
struct List {
    bytes: &'static [u8],
    starts: Vec<u32>,
}

impl List {
    fn new(bytes: &'static [u8]) -> List {
        let mut starts = Vec::with_capacity(bytes.len() / 8);
        let mut at = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'\n' {
                starts.push(at as u32);
                at = i + 1;
            }
        }
        List { bytes, starts }
    }

    fn word(&self, i: usize) -> &[u8] {
        let start = self.starts[i] as usize;
        let end = self.bytes[start..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(self.bytes.len(), |n| start + n);
        &self.bytes[start..end]
    }

    fn contains(&self, word: &[u8]) -> bool {
        self.starts
            .binary_search_by(|&s| {
                let start = s as usize;
                let rest = &self.bytes[start..];
                let end = rest.iter().position(|&b| b == b'\n').unwrap_or(rest.len());
                rest[..end].cmp(word)
            })
            .is_ok()
    }

    fn len(&self) -> usize {
        self.starts.len()
    }
}

/// US English from SCOWL (size 60), embedded in the executable.
pub struct Builtin {
    words: List,
    hidden: List,
}

impl Default for Builtin {
    fn default() -> Builtin {
        Builtin::new()
    }
}

impl Builtin {
    /// Indexes the embedded lists (one pass, about a millisecond).
    pub fn new() -> Builtin {
        Builtin {
            words: List::new(WORDS),
            hidden: List::new(NO_SUGGEST),
        }
    }

    /// Words in the list.
    pub fn len(&self) -> usize {
        self.words.len()
    }

    pub fn is_empty(&self) -> bool {
        self.words.len() == 0
    }

    /// The `i`th word in sorted order.
    pub fn word(&self, i: usize) -> &str {
        std::str::from_utf8(self.words.word(i)).unwrap_or("")
    }

    /// `word` as written, or with its first letter lowercased (a capital at
    /// the start of a sentence).
    pub fn contains(&self, word: &str) -> bool {
        if self.words.contains(word.as_bytes()) {
            return true;
        }
        let mut chars = word.chars();
        match chars.next() {
            Some(first) if first.is_uppercase() => {
                let lower: String = first.to_lowercase().chain(chars).collect();
                self.words.contains(lower.as_bytes())
            }
            _ => false,
        }
    }
}

impl Speller for Builtin {
    fn name(&self) -> &'static str {
        "builtin"
    }

    fn check(&mut self, word: &str) -> Result<bool, String> {
        Ok(self.contains(word))
    }

    fn suggest(
        &mut self,
        word: &str,
        max: usize,
        deadline: Instant,
    ) -> Result<Vec<String>, String> {
        let words = &self.words;
        let hidden = &self.hidden;
        Ok(candidates(word, max, deadline, &mut |w: &str| {
            words.contains(w.as_bytes()) && !hidden.contains(w.as_bytes())
        }))
    }
}
