//! The edited document: text, selections, undo history, clean state and
//! edit-permission locks. [`Document::apply`] is the only mutation entry.

use rte_edit::{Command, EditOptions, History, Selection, Selections, Step, apply};
use rte_text::{ChangeSet, Snapshot};

/// Changes kept for one observer between its reads; older ones are
/// dropped (the observer then diffs snapshots).
const JOURNAL_MAX: usize = 64;

/// Independent locks; each has one owner (Architecture, App: Edit permission).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Locks {
    /// `--read-only` or the toggle command.
    pub user: bool,
    /// Set until the final snapshot is published.
    pub loading: bool,
    /// Malformed UTF-16; never cleared.
    pub encoding: bool,
    /// Read error; never cleared.
    pub load_error: bool,
}

impl Locks {
    /// Why edits are refused, if they are.
    pub fn edit_refusal(&self) -> Option<&'static str> {
        self.save_refusal().or(self.user.then_some("read-only"))
    }

    /// Why saving is refused, if it is.
    pub fn save_refusal(&self) -> Option<&'static str> {
        if self.loading {
            Some("still loading")
        } else if self.encoding {
            Some("malformed UTF-16")
        } else if self.load_error {
            Some("read error")
        } else {
            None
        }
    }
}

pub struct Document {
    pub snapshot: Snapshot,
    pub selections: Selections,
    pub history: History,
    /// Snapshot last loaded or committed by a successful save.
    pub saved: Snapshot,
    pub locks: Locks,
    pub options: EditOptions,
    /// Applied commands with their exact changes, recorded while `Some`
    /// (syntax highlighting reads and clears it). Undo, redo and loads are
    /// not recorded.
    pub journal: Option<Vec<Journaled>>,
}

/// A command's change: the snapshots before and after it.
pub struct Journaled {
    pub before: Snapshot,
    pub after: Snapshot,
    pub changes: ChangeSet,
}

impl Document {
    pub fn new(options: EditOptions) -> Document {
        let snapshot = Snapshot::empty();
        Document {
            saved: snapshot.clone(),
            snapshot,
            selections: Selections::single(Selection::caret(0)),
            history: History::default(),
            locks: Locks::default(),
            options,
            journal: None,
        }
    }

    pub fn is_dirty(&self) -> bool {
        !self.snapshot.same_tree(&self.saved)
    }

    /// Replaces the text with loaded content (clean; selections clamped).
    pub fn load(&mut self, snapshot: Snapshot) {
        let len = snapshot.len();
        self.selections = self
            .selections
            .map(|s| Selection::new(s.anchor.min(len), s.head.min(len)));
        self.saved = snapshot.clone();
        self.snapshot = snapshot;
    }

    /// Applies an edit command and records it for undo. `typed` is the
    /// character of a single keystroke (for word-group coalescing).
    pub fn apply(&mut self, cmd: &Command, typed: Option<char>) -> Result<(), &'static str> {
        self.apply_selecting(cmd, typed, None)
    }

    /// [`Document::apply`] leaving `selections` (computed by the command's
    /// builder) instead of the mapped ones.
    pub fn apply_selecting(
        &mut self,
        cmd: &Command,
        typed: Option<char>,
        selections: Option<Selections>,
    ) -> Result<(), &'static str> {
        if let Some(reason) = self.locks.edit_refusal() {
            return Err(reason);
        }
        let (next, sels, changes) = apply(&self.snapshot, &self.selections, cmd)
            .expect("commands build valid transactions");
        let sels = selections.unwrap_or(sels);
        if let Some(journal) = &mut self.journal {
            if journal.len() >= JOURNAL_MAX {
                journal.clear();
            }
            journal.push(Journaled {
                before: self.snapshot.clone(),
                after: next.clone(),
                changes,
            });
        }
        let inserted = cmd.tx.edits().iter().map(|e| e.text.len() as u64).sum();
        self.history.record(
            Step {
                before: self.snapshot.clone(),
                after: next.clone(),
                sels_before: self.selections.clone(),
                sels_after: sels.clone(),
            },
            cmd.kind,
            inserted,
            typed,
        );
        self.snapshot = next;
        self.selections = sels;
        Ok(())
    }

    /// Moves the selections; the next edit starts a new undo step.
    pub fn select(&mut self, selections: Selections) {
        self.history.seal();
        self.selections = selections;
    }

    /// Restores the state before the last step. `Ok(false)`: nothing to undo.
    pub fn undo(&mut self) -> Result<bool, &'static str> {
        if let Some(reason) = self.locks.edit_refusal() {
            return Err(reason);
        }
        Ok(match self.history.undo() {
            Some((snapshot, sels)) => {
                self.snapshot = snapshot;
                self.selections = sels;
                true
            }
            None => false,
        })
    }

    pub fn redo(&mut self) -> Result<bool, &'static str> {
        if let Some(reason) = self.locks.edit_refusal() {
            return Err(reason);
        }
        Ok(match self.history.redo() {
            Some((snapshot, sels)) => {
                self.snapshot = snapshot;
                self.selections = sels;
                true
            }
            None => false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rte_edit::{Clip, insert, paste};

    fn doc(text: &[u8]) -> Document {
        let mut doc = Document::new(EditOptions::default());
        doc.load(Snapshot::from_bytes(text));
        doc
    }

    #[test]
    fn every_lock_refuses_every_mutation() {
        let locks = [
            Locks {
                user: true,
                ..Locks::default()
            },
            Locks {
                loading: true,
                ..Locks::default()
            },
            Locks {
                encoding: true,
                ..Locks::default()
            },
            Locks {
                load_error: true,
                ..Locks::default()
            },
        ];
        for lock in locks {
            let mut d = doc(b"abc");
            let typing = insert(&d.snapshot, &d.selections, b"x", &d.options);
            d.apply(&typing, Some('x')).unwrap();
            d.locks = lock;
            let before = d.snapshot.clone();
            assert!(d.apply(&typing, Some('x')).is_err());
            let clip = Clip {
                pieces: vec![b"p".to_vec()],
                line_mode: false,
            };
            let pasting = paste(&d.snapshot, &d.selections, &clip, &d.options).unwrap();
            assert!(d.apply(&pasting, None).is_err());
            assert!(d.undo().is_err());
            assert!(d.redo().is_err());
            assert!(d.snapshot.same_tree(&before), "{lock:?}");
            assert_eq!(
                d.locks.save_refusal().is_some(),
                !lock.user,
                "save refusal {lock:?}"
            );
        }
    }

    #[test]
    fn a_new_document_is_clean() {
        let mut d = Document::new(EditOptions::default());
        assert!(!d.is_dirty());
        let typing = insert(&d.snapshot, &d.selections, b"x", &d.options);
        d.apply(&typing, Some('x')).unwrap();
        assert!(d.is_dirty());
        d.undo().unwrap();
        assert!(!d.is_dirty());
    }

    #[test]
    fn undo_to_the_saved_root_is_clean() {
        let mut d = doc(b"abc");
        assert!(!d.is_dirty());
        let typing = insert(&d.snapshot, &d.selections, b"x", &d.options);
        d.apply(&typing, Some('x')).unwrap();
        assert!(d.is_dirty());
        assert!(d.undo().unwrap());
        assert!(!d.is_dirty());
        assert!(d.redo().unwrap());
        assert!(d.is_dirty());
        assert!(!d.redo().unwrap());
    }
}
