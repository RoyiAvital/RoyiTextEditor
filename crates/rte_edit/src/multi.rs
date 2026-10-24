//! Multi-cursor and column-selection commands (VS Code behavior). They only
//! change selections; edits then apply at every selection.

use rte_text::Snapshot;

use crate::motion::{column_at, content_range, offset_in_line};
use crate::selection::{Selection, Selections};

/// `Ctrl+Alt+Up` / `Ctrl+Alt+Down`: adds a caret `lines` away from each
/// selection's head, at its goal column. Heads on the first or last line add
/// nothing. The cursor added for the primary becomes the primary.
pub fn add_cursors_vertical(s: &Snapshot, sels: &Selections, lines: i64, tab: u32) -> Selections {
    let last = s.line_count() - 1;
    let mut out: Vec<Selection> = sels.as_slice().to_vec();
    let mut primary = sels.primary_index();
    for (i, sel) in sels.as_slice().iter().enumerate() {
        let target = s.line_of(sel.head) as i64 + lines;
        if target < 0 || target as u64 > last {
            continue;
        }
        let goal = sel.goal.unwrap_or_else(|| column_at(s, sel.head, tab));
        let pos = offset_in_line(s, content_range(s, target as u64), goal, tab);
        if i == sels.primary_index() {
            primary = out.len();
        }
        out.push(Selection {
            goal: Some(goal),
            ..Selection::caret(pos)
        });
    }
    Selections::new(out, primary)
}

/// `Shift+Alt+I`: a caret at the end of every line each selection touches;
/// the last line gets one at the selection end (none when that end is the
/// line start). `None`, without building them, when there would be more than
/// `limit` carets.
pub fn cursors_at_line_ends(s: &Snapshot, sels: &Selections, limit: usize) -> Option<Selections> {
    let mut out: Vec<Selection> = Vec::new();
    let push = |out: &mut Vec<Selection>, pos: u64| {
        if out.last().is_none_or(|c| c.head != pos) {
            out.push(Selection::caret(pos));
        }
        out.len() <= limit
    };
    for sel in sels.as_slice() {
        let (first, last) = (s.line_of(sel.start()), s.line_of(sel.end()));
        if first == last {
            if !push(&mut out, sel.end()) {
                return None;
            }
            continue;
        }
        if (last - first) as usize > limit {
            return None;
        }
        for line in first..last {
            if !push(&mut out, content_range(s, line).end) {
                return None;
            }
        }
        if sel.end() > s.line_start(last).unwrap_or(0) && !push(&mut out, sel.end()) {
            return None;
        }
    }
    let primary = out.len().saturating_sub(1);
    Some(Selections::new(out, primary))
}

/// `Ctrl+L`: selects whole lines (with their line break); a selection that
/// already covers whole lines grows by the next line.
pub fn select_lines(s: &Snapshot, sels: &Selections) -> Selections {
    let line_start = |line: u64| s.line_start(line).unwrap_or(s.len());
    sels.map(|sel| {
        let start = line_start(s.line_of(sel.start()));
        Selection::new(start, line_start(s.line_of(sel.end()) + 1))
    })
}

/// A column selection between two (line, column) cells: one selection per
/// line from the anchor column to the head column. Lines shorter than both
/// columns get a caret at their end. The head line is primary. `None`,
/// without building them, when it spans more than `limit` lines.
pub fn box_select(
    s: &Snapshot,
    anchor: (u64, u64),
    head: (u64, u64),
    tab: u32,
    limit: usize,
) -> Option<Selections> {
    let last = s.line_count() - 1;
    let (a_line, h_line) = (anchor.0.min(last), head.0.min(last));
    if a_line.abs_diff(h_line) >= limit as u64 {
        return None;
    }
    let lines: Box<dyn Iterator<Item = u64>> = if a_line <= h_line {
        Box::new(a_line..=h_line)
    } else {
        Box::new((h_line..=a_line).rev())
    };
    let out: Vec<Selection> = lines
        .map(|line| {
            let range = content_range(s, line);
            let a = offset_in_line(s, range.clone(), anchor.1, tab);
            let h = offset_in_line(s, range, head.1, tab);
            Selection {
                goal: Some(head.1),
                ..Selection::new(a, h)
            }
        })
        .collect();
    let primary = out.len() - 1;
    Some(Selections::new(out, primary))
}

/// Line and display column of `pos` (column-selection cells).
pub fn cell_of(s: &Snapshot, pos: u64, tab: u32) -> (u64, u64) {
    (s.line_of(pos), column_at(s, pos, tab))
}
