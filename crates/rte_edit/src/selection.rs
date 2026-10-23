//! Selections: a sorted, merged, non-empty set with a primary member.

use std::ops::Range;

/// A range between `anchor` (fixed end) and `head` (caret). `goal` is the
/// display column kept across vertical moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub anchor: u64,
    pub head: u64,
    pub goal: Option<u64>,
}

impl Selection {
    pub fn caret(pos: u64) -> Selection {
        Selection::new(pos, pos)
    }

    pub fn new(anchor: u64, head: u64) -> Selection {
        Selection {
            anchor,
            head,
            goal: None,
        }
    }

    pub fn start(&self) -> u64 {
        self.anchor.min(self.head)
    }

    pub fn end(&self) -> u64 {
        self.anchor.max(self.head)
    }

    pub fn range(&self) -> Range<u64> {
        self.start()..self.end()
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selections {
    ranges: Vec<Selection>,
    primary: usize,
}

impl Selections {
    pub fn single(selection: Selection) -> Selections {
        Selections {
            ranges: vec![selection],
            primary: 0,
        }
    }

    /// Sorts and merges `ranges`; `primary` indexes into the input. Empty
    /// input becomes a caret at 0.
    pub fn new(ranges: Vec<Selection>, primary: usize) -> Selections {
        if ranges.is_empty() {
            return Selections::single(Selection::caret(0));
        }
        let primary = primary.min(ranges.len() - 1);
        let mut indexed: Vec<(usize, Selection)> = ranges.into_iter().enumerate().collect();
        indexed.sort_by_key(|(_, s)| (s.start(), s.end()));
        let mut merged: Vec<Selection> = Vec::with_capacity(indexed.len());
        let mut new_primary = 0;
        for (index, sel) in indexed {
            match merged.last_mut() {
                Some(last) if overlaps(last, &sel) => {
                    let (start, end) = (last.start(), last.end().max(sel.end()));
                    let forward = last.anchor <= last.head;
                    *last = if forward {
                        Selection::new(start, end)
                    } else {
                        Selection::new(end, start)
                    };
                }
                _ => merged.push(sel),
            }
            if index == primary {
                new_primary = merged.len() - 1;
            }
        }
        Selections {
            ranges: merged,
            primary: new_primary,
        }
    }

    pub fn as_slice(&self) -> &[Selection] {
        &self.ranges
    }

    pub fn len(&self) -> usize {
        self.ranges.len()
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn primary(&self) -> Selection {
        self.ranges[self.primary]
    }

    pub fn primary_index(&self) -> usize {
        self.primary
    }

    /// Applies `f` to every selection and normalizes the result.
    pub fn map(&self, f: impl FnMut(&Selection) -> Selection) -> Selections {
        Selections::new(self.ranges.iter().map(f).collect(), self.primary)
    }

    /// Only the primary selection.
    pub fn only_primary(&self) -> Selections {
        Selections::single(self.primary())
    }
}

/// Overlapping ranges merge; an empty selection touching another merges too.
fn overlaps(a: &Selection, b: &Selection) -> bool {
    b.start() < a.end() || (b.start() == a.end() && (a.is_empty() || b.is_empty()))
}
