//! Edits, transactions and position mapping.

use std::fmt;
use std::ops::Range;

/// Part of a [`Inserted::Shared`] text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Piece {
    Bytes(Box<[u8]>),
    /// The bytes of this range of the snapshot the transaction applies to,
    /// shared with it (no bytes are copied).
    Copy(Range<u64>),
}

impl Piece {
    pub fn len(&self) -> usize {
        match self {
            Piece::Bytes(b) => b.len(),
            Piece::Copy(r) => (r.end - r.start) as usize,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Text an [`Edit`] inserts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Inserted {
    Bytes(Box<[u8]>),
    /// Pieces joined in order; copied ranges share the base's buffers.
    Shared(Vec<Piece>),
}

impl Inserted {
    pub fn len(&self) -> usize {
        match self {
            Inserted::Bytes(b) => b.len(),
            Inserted::Shared(pieces) => pieces.iter().map(Piece::len).sum(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Bytes this edit owns (copied ranges own none).
    pub fn owned_len(&self) -> usize {
        match self {
            Inserted::Bytes(b) => b.len(),
            Inserted::Shared(pieces) => pieces
                .iter()
                .map(|p| match p {
                    Piece::Bytes(b) => b.len(),
                    Piece::Copy(_) => 0,
                })
                .sum(),
        }
    }

    /// The owned bytes of a [`Inserted::Bytes`]; empty for shared text.
    pub fn bytes(&self) -> &[u8] {
        match self {
            Inserted::Bytes(b) => b,
            Inserted::Shared(_) => &[],
        }
    }
}

/// Replacement of `range` (byte offsets in the base snapshot) by `text`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub range: Range<u64>,
    pub text: Inserted,
}

impl Edit {
    pub fn new(range: Range<u64>, text: impl Into<Box<[u8]>>) -> Edit {
        Edit {
            range,
            text: Inserted::Bytes(text.into()),
        }
    }

    pub fn insert(at: u64, text: impl Into<Box<[u8]>>) -> Edit {
        Edit::new(at..at, text)
    }

    pub fn delete(range: Range<u64>) -> Edit {
        Edit::new(range, Box::<[u8]>::default())
    }

    /// Replaces `range` with the bytes of `source` in the base snapshot.
    pub fn copy(range: Range<u64>, source: Range<u64>) -> Edit {
        Edit::shared(range, vec![Piece::Copy(source)])
    }

    /// Replaces `range` with `pieces` joined (copies share the base's bytes).
    pub fn shared(range: Range<u64>, pieces: Vec<Piece>) -> Edit {
        Edit {
            range,
            text: Inserted::Shared(pieces),
        }
    }
}

/// Sorted, non-overlapping edits applied atomically.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Transaction {
    edits: Vec<Edit>,
}

impl Transaction {
    pub fn new(edits: Vec<Edit>) -> Transaction {
        Transaction { edits }
    }

    pub fn single(edit: Edit) -> Transaction {
        Transaction { edits: vec![edit] }
    }

    pub fn edits(&self) -> &[Edit] {
        &self.edits
    }

    pub(crate) fn validate(&self, len: u64) -> Result<(), EditError> {
        let mut prev: Option<&Edit> = None;
        // The base plus all inserted bytes bounds the result and every
        // intermediate tree (shared copies insert without allocating).
        let mut total = len;
        for (index, edit) in self.edits.iter().enumerate() {
            let r = &edit.range;
            if r.start > r.end || r.end > len {
                return Err(EditError::OutOfBounds { index, len });
            }
            let inserted = match &edit.text {
                Inserted::Bytes(b) => Some(b.len() as u64),
                Inserted::Shared(pieces) => {
                    let mut sum = Some(0u64);
                    for p in pieces {
                        let n = match p {
                            Piece::Copy(s) if s.start > s.end || s.end > len => {
                                return Err(EditError::OutOfBounds { index, len });
                            }
                            Piece::Copy(s) => s.end - s.start,
                            Piece::Bytes(b) => b.len() as u64,
                        };
                        sum = sum.and_then(|sum| sum.checked_add(n));
                    }
                    sum
                }
            };
            total = match inserted.and_then(|n| total.checked_add(n)) {
                Some(total) if total <= MAX_LEN => total,
                _ => return Err(EditError::TooLarge { index }),
            };
            if let Some(p) = prev {
                let same_insertion = p.range.is_empty() && r.is_empty() && p.range.start == r.start;
                if r.start < p.range.end || same_insertion {
                    return Err(EditError::Unordered { index });
                }
            }
            prev = Some(edit);
        }
        Ok(())
    }
}

/// Largest document length: positions and their deltas fit `i64`.
pub const MAX_LEN: u64 = i64::MAX as u64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditError {
    /// Edit `index` is reversed or extends past the document length.
    OutOfBounds { index: usize, len: u64 },
    /// Edit `index` overlaps or precedes the previous edit, or repeats its insertion point.
    Unordered { index: usize },
    /// With edit `index` the text would pass [`MAX_LEN`].
    TooLarge { index: usize },
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EditError::OutOfBounds { index, len } => {
                write!(f, "edit {index} is out of bounds (document length {len})")
            }
            EditError::Unordered { index } => {
                write!(f, "edit {index} overlaps or precedes the previous edit")
            }
            EditError::TooLarge { index } => {
                write!(f, "edit {index} makes the text larger than {MAX_LEN} bytes")
            }
        }
    }
}

impl std::error::Error for EditError {}

/// Which side a position takes when its location is replaced or inserted at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bias {
    Left,
    Right,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub old: Range<u64>,
    pub new_len: u64,
}

/// Result of applying a transaction: maps positions from the old to the new snapshot.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChangeSet {
    changes: Vec<Change>,
    /// `deltas[i]`: total length change of the changes before `i`.
    deltas: Vec<i64>,
}

impl ChangeSet {
    pub(crate) fn from_transaction(tx: &Transaction) -> ChangeSet {
        ChangeSet::from_changes(
            tx.edits
                .iter()
                .map(|e| Change {
                    old: e.range.clone(),
                    new_len: e.text.len() as u64,
                })
                .collect(),
        )
    }

    /// Changes must be sorted and non-overlapping (as in a [`Transaction`]).
    pub fn from_changes(changes: Vec<Change>) -> ChangeSet {
        let mut deltas = Vec::with_capacity(changes.len() + 1);
        let mut delta = 0i64;
        deltas.push(0);
        for c in &changes {
            delta += c.new_len as i64 - (c.old.end - c.old.start) as i64;
            deltas.push(delta);
        }
        ChangeSet { changes, deltas }
    }

    pub fn changes(&self) -> &[Change] {
        &self.changes
    }

    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// Maps `pos` from the old snapshot into the new one.
    ///
    /// Positions strictly inside a replaced range go to its new start (`Left`)
    /// or end (`Right`); an insertion at `pos` follows the bias; positions at
    /// the edges of a non-empty range stay outside it.
    pub fn map(&self, pos: u64, bias: Bias) -> u64 {
        let mut i = self.changes.partition_point(|c| c.old.end < pos);
        let mut delta = self.deltas[i];
        while let Some(c) = self.changes.get(i) {
            if pos < c.old.start {
                break;
            }
            let new_start = (c.old.start as i64 + delta) as u64;
            let new_end = new_start + c.new_len;
            if c.old.is_empty() {
                return if bias == Bias::Left {
                    new_start
                } else {
                    new_end
                };
            }
            if pos == c.old.start {
                return new_start;
            }
            if pos < c.old.end {
                return if bias == Bias::Left {
                    new_start
                } else {
                    new_end
                };
            }
            delta = self.deltas[i + 1];
            i += 1;
        }
        (pos as i64 + delta) as u64
    }
}
