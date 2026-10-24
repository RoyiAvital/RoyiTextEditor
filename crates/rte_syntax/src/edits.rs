//! Tree-sitter edits from a transaction's changes.

use std::ops::Range;

use rte_text::{Change, ChangeSet, Snapshot};
use tree_sitter::{InputEdit, Point};

/// Point (row, byte column) of `offset` in `snapshot`.
pub(crate) fn point(snapshot: &Snapshot, offset: u64) -> Point {
    let row = snapshot.line_of(offset);
    let start = snapshot.line_start(row).unwrap_or(0);
    Point::new(row as usize, (offset - start) as usize)
}

/// Growth of a change.
fn growth(c: &Change) -> i64 {
    c.new_len as i64 - (c.old.end - c.old.start) as i64
}

/// The edit replacing `old` (positions in `old_text`) by the `new_len` bytes
/// at `new_start` of `new_text`, for a tree whose text before `old.start` is
/// still the old text (edits are applied last to first).
fn edit(
    old_text: &Snapshot,
    new_text: &Snapshot,
    old: Range<u64>,
    new_start: u64,
    new_len: u64,
) -> InputEdit {
    let start_position = point(old_text, old.start);
    let new_end = new_start + new_len;
    let mut new_end_position = start_position;
    if new_len > 0 {
        let rows = new_text.line_of(new_end) - new_text.line_of(new_start);
        if rows == 0 {
            new_end_position.column += new_len as usize;
        } else {
            let line_start = new_text.line_start(new_text.line_of(new_end)).unwrap_or(0);
            new_end_position.row += rows as usize;
            new_end_position.column = (new_end - line_start) as usize;
        }
    }
    InputEdit {
        start_byte: old.start as usize,
        old_end_byte: old.end as usize,
        new_end_byte: (old.start + new_len) as usize,
        start_position,
        old_end_position: point(old_text, old.end),
        new_end_position,
    }
}

/// Edits turning a tree of `old` into one of `new`, to apply in order.
///
/// The changes are applied last to first, so the start and old end of each
/// one are positions in `old`; the new end follows from its inserted text.
pub fn input_edits(old: &Snapshot, new: &Snapshot, changes: &ChangeSet) -> Vec<InputEdit> {
    input_edits_near(old, new, changes, 0..u64::MAX)
}

/// Every change's new text touches `near`: [`input_edits_near`] is then
/// exact everywhere.
pub fn changes_near(changes: &ChangeSet, near: Range<u64>) -> bool {
    let mut delta = 0i64;
    changes.changes().iter().all(|c| {
        let start = (c.old.start as i64 + delta) as u64;
        delta += growth(c);
        start <= near.end && start + c.new_len >= near.start
    })
}

/// Like [`input_edits`], but only changes whose new text touches `near`
/// (positions in `new`) are exact: the changes before it become one point
/// edit with their total growth, and the changes after it are left out.
/// Positions near the range (the visible rows) stay exact while the cost
/// does not depend on changes elsewhere (multi-cursor edits); tree-sitter
/// edits cost about the number of nodes their old range covers. Trees
/// edited this way are valid only near the range.
pub fn input_edits_near(
    old: &Snapshot,
    new: &Snapshot,
    changes: &ChangeSet,
    near: Range<u64>,
) -> Vec<InputEdit> {
    let list = changes.changes();
    // New start of each change: its old start plus the growth before it.
    let mut new_starts = Vec::with_capacity(list.len());
    let mut delta = 0i64;
    for c in list {
        new_starts.push((c.old.start as i64 + delta) as u64);
        delta += growth(c);
    }
    let touches = |i: usize| {
        let start = new_starts[i];
        start <= near.end && start + list[i].new_len >= near.start
    };
    let first = (0..list.len())
        .find(|&i| touches(i) || new_starts[i] > near.end)
        .unwrap_or(list.len());
    let last = (first..list.len())
        .take_while(|&i| touches(i))
        .last()
        .map_or(first, |i| i + 1);
    let mut edits = Vec::with_capacity(last - first + 1);
    for i in (first..last).rev() {
        let c = &list[i];
        edits.push(edit(old, new, c.old.clone(), new_starts[i], c.new_len));
    }
    if first > 0 {
        let before: i64 = list[..first].iter().map(growth).sum();
        // Old position of the start of `near`, before the exact part.
        let mut at = ((near.start as i64 - before).max(0) as u64).max(list[first - 1].old.end);
        if first < last {
            at = at.min(list[first].old.start);
        }
        let new_at = (at as i64 + before) as u64;
        let position = point(new, new_at);
        let (start, start_position) = if before >= 0 {
            (at, point(old, at))
        } else {
            (at - before.unsigned_abs(), position)
        };
        edits.push(InputEdit {
            start_byte: start as usize,
            old_end_byte: at as usize,
            new_end_byte: new_at as usize,
            start_position,
            old_end_position: point(old, at),
            new_end_position: position,
        });
    }
    edits
}
