//! Linear undo history of snapshot roots.

use std::collections::VecDeque;

use rte_text::Snapshot;

use crate::commands::EditKind;
use crate::selection::Selections;

pub const MAX_HISTORY_STEPS: usize = 10_000;
/// Inserted bytes held by the history before the oldest steps drop.
pub const MAX_HISTORY_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct Step {
    pub before: Snapshot,
    pub after: Snapshot,
    pub sels_before: Selections,
    pub sels_after: Selections,
}

#[derive(Debug)]
struct Entry {
    step: Step,
    kind: EditKind,
    inserted: u64,
    /// The last typed character was a word character.
    last_word: bool,
}

#[derive(Debug)]
pub struct History {
    undo: VecDeque<Entry>,
    redo: Vec<Entry>,
    inserted: u64,
    sealed: bool,
    max_steps: usize,
    max_bytes: u64,
}

impl Default for History {
    fn default() -> History {
        History::with_limits(MAX_HISTORY_STEPS, MAX_HISTORY_BYTES)
    }
}

fn is_word(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

impl History {
    pub fn with_limits(max_steps: usize, max_bytes: u64) -> History {
        History {
            undo: VecDeque::new(),
            redo: Vec::new(),
            inserted: 0,
            sealed: false,
            max_steps: max_steps.max(1),
            max_bytes,
        }
    }

    /// Records an applied edit. Typing and same-direction deletions extend
    /// the previous step unless sealed; a word typed after a non-word
    /// character starts a new step.
    pub fn record(&mut self, step: Step, kind: EditKind, inserted: u64, typed: Option<char>) {
        self.redo.clear();
        let word = typed.is_some_and(is_word);
        if !self.sealed
            && kind != EditKind::Other
            && let Some(last) = self.undo.back_mut()
            && last.kind == kind
            && last.step.after.same_tree(&step.before)
            && last.step.sels_after == step.sels_before
            && !(kind == EditKind::Typing && word && !last.last_word)
        {
            last.step.after = step.after;
            last.step.sels_after = step.sels_after;
            last.inserted += inserted;
            last.last_word = word;
            self.inserted += inserted;
            self.trim();
            return;
        }
        self.sealed = false;
        self.inserted += inserted;
        self.undo.push_back(Entry {
            step,
            kind,
            inserted,
            last_word: word,
        });
        self.trim();
    }

    /// Ends coalescing: the next edit starts a new step.
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    fn trim(&mut self) {
        while self.undo.len() > 1
            && (self.undo.len() > self.max_steps || self.inserted > self.max_bytes)
        {
            let dropped = self.undo.pop_front().expect("non-empty");
            self.inserted -= dropped.inserted;
        }
    }

    /// The state before the last step, or `None` when nothing is left to undo.
    pub fn undo(&mut self) -> Option<(Snapshot, Selections)> {
        let entry = self.undo.pop_back()?;
        self.inserted -= entry.inserted;
        self.sealed = true;
        let state = (entry.step.before.clone(), entry.step.sels_before.clone());
        self.redo.push(entry);
        Some(state)
    }

    /// The state after the last undone step.
    pub fn redo(&mut self) -> Option<(Snapshot, Selections)> {
        let entry = self.redo.pop()?;
        self.inserted += entry.inserted;
        self.sealed = true;
        let state = (entry.step.after.clone(), entry.step.sels_after.clone());
        self.undo.push_back(entry);
        Some(state)
    }

    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }
}
