//! Caret movement over a snapshot. Every movement scans at most
//! [`MOTION_SCAN_LIMIT`] bytes; longer runs (words, whitespace, combining
//! marks) are crossed by repeating the movement.

use std::ops::Range;

use rte_text::Snapshot;

use crate::columns::{COLUMN_SCAN_LIMIT, cell_width, column_of, offset_at_column};
use crate::cursor::Bytes;

/// Bytes one movement may scan.
pub const MOTION_SCAN_LIMIT: u64 = 64 * 1024;

fn is_mark(ch: Option<char>) -> bool {
    ch.is_some_and(|c| cell_width(c) == 0)
}

fn next_in(b: &mut Bytes<'_>, pos: u64) -> Option<u64> {
    let (ch, len) = b.char_at(pos)?;
    if ch == Some('\r') && b.get(pos + 1) == Some(b'\n') {
        return Some(pos + 2);
    }
    let limit = pos + MOTION_SCAN_LIMIT;
    let mut next = pos + len;
    while next < limit
        && let Some((c, len)) = b.char_at(next)
        && is_mark(c)
    {
        next += len;
    }
    Some(next)
}

fn prev_in(b: &mut Bytes<'_>, pos: u64) -> Option<u64> {
    if pos == 0 {
        return None;
    }
    if pos >= 2 && b.get(pos - 1) == Some(b'\n') && b.get(pos - 2) == Some(b'\r') {
        return Some(pos - 2);
    }
    let limit = pos.saturating_sub(MOTION_SCAN_LIMIT);
    let mut prev = pos;
    while prev > limit
        && let Some((ch, len)) = b.char_before(prev)
    {
        prev -= len;
        if !is_mark(ch) {
            break;
        }
    }
    Some(prev)
}

/// Next caret position after `pos`: one character with its marks, or a CRLF.
pub fn next_boundary(s: &Snapshot, pos: u64) -> u64 {
    next_in(&mut Bytes::new(s), pos).unwrap_or(s.len())
}

/// Previous caret position before `pos` (inverse of [`next_boundary`]).
pub fn prev_boundary(s: &Snapshot, pos: u64) -> u64 {
    prev_in(&mut Bytes::new(s), pos.min(s.len())).unwrap_or(0)
}

/// Nearest caret position at or before `pos`: never inside a UTF-8
/// sequence, between CR and LF, or before a zero-width mark of a character
/// on the same line. Used for positions estimated from columns.
pub fn snap_to_boundary(s: &Snapshot, pos: u64) -> u64 {
    let mut b = Bytes::new(s);
    let mut p = pos.min(s.len());
    for k in 1..=p.min(3) {
        let Some(byte) = b.get(p - k) else { break };
        if byte & 0xC0 == 0x80 {
            continue;
        }
        if let Some((Some(_), len)) = b.char_at(p - k)
            && len > k
        {
            p -= k;
        }
        break;
    }
    if p > 0 && b.get(p - 1) == Some(b'\r') && b.get(p) == Some(b'\n') {
        p -= 1;
    }
    let limit = p.saturating_sub(MOTION_SCAN_LIMIT);
    while p > limit
        && is_mark(b.char_at(p).and_then(|(ch, _)| ch))
        && let Some((ch, len)) = b.char_before(p)
        && !matches!(ch, Some('\n' | '\r'))
    {
        p -= len;
    }
    p
}

/// Content of `line` without its line break (`\n` or `\r\n`).
pub fn content_range(s: &Snapshot, line: u64) -> Range<u64> {
    let range = s.line_range(line).unwrap_or(s.len()..s.len());
    if range.end < s.len()
        && range.end > range.start
        && Bytes::new(s).get(range.end - 1) == Some(b'\r')
    {
        range.start..range.end - 1
    } else {
        range
    }
}

/// End of the content of the line containing `pos`.
pub fn line_end(s: &Snapshot, pos: u64) -> u64 {
    content_range(s, s.line_of(pos)).end
}

/// Smart Home: the first non-blank character, or the line start when already there.
pub fn line_home(s: &Snapshot, pos: u64) -> u64 {
    let range = content_range(s, s.line_of(pos));
    let limit = range.end.min(range.start + COLUMN_SCAN_LIMIT as u64);
    let mut b = Bytes::new(s);
    let mut first = range.start;
    while first < limit && matches!(b.get(first), Some(b' ' | b'\t')) {
        first += 1;
    }
    if pos == first { range.start } else { first }
}

/// Display column of `pos` within its line (exact within the first
/// [`COLUMN_SCAN_LIMIT`] bytes; one column per byte beyond).
pub fn column_at(s: &Snapshot, pos: u64, tab_width: u32) -> u64 {
    let range = content_range(s, s.line_of(pos));
    let pos = pos.clamp(range.start, range.end);
    let scanned = s.to_vec(range.start..pos.min(range.start + COLUMN_SCAN_LIMIT as u64));
    column_of(&scanned, (pos - range.start) as usize, tab_width)
}

/// Caret position nearest to display column `col` in the line `range`,
/// reading only as many bytes as the column needs.
pub fn offset_in_line(s: &Snapshot, range: Range<u64>, col: u64, tab_width: u32) -> u64 {
    let full = (range.end - range.start).min(COLUMN_SCAN_LIMIT as u64);
    let mut window = col.saturating_mul(4).saturating_add(256).min(full);
    loop {
        let bytes = s.to_vec(range.start..range.start + window);
        let offset = offset_at_column(&bytes, col, tab_width) as u64;
        if offset < window || window == full {
            return snap_to_boundary(s, range.start.saturating_add(offset).min(range.end));
        }
        window = window.saturating_mul(4).min(full);
    }
}

/// Moves `pos` by `lines` keeping display column `goal` (or the current
/// column). Moving above the first line goes to 0, below the last to the end.
/// Returns the new position and the goal column.
pub fn vertical(
    s: &Snapshot,
    pos: u64,
    goal: Option<u64>,
    lines: i64,
    tab_width: u32,
) -> (u64, u64) {
    let goal = goal.unwrap_or_else(|| column_at(s, pos, tab_width));
    let line = s.line_of(pos);
    let last = s.line_count() - 1;
    let target = line as i64 + lines;
    if target < 0 {
        return (0, goal);
    }
    if target as u64 > last {
        return (s.len(), goal);
    }
    let range = content_range(s, target as u64);
    (offset_in_line(s, range, goal, tab_width), goal)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    Space,
    Word,
    Punct,
    Break,
}

fn class(ch: Option<char>) -> Class {
    match ch {
        Some('\n' | '\r') => Class::Break,
        Some(c) if c.is_whitespace() => Class::Space,
        Some(c) if c.is_alphanumeric() || c == '_' => Class::Word,
        _ => Class::Punct,
    }
}

/// Class of the character at `pos`.
fn class_at(b: &mut Bytes<'_>, pos: u64) -> Option<Class> {
    b.char_at(pos).map(|(ch, _)| class(ch))
}

/// Class of the character (with its marks) ending at `pos`.
fn class_before(b: &mut Bytes<'_>, pos: u64) -> Option<Class> {
    let start = prev_in(b, pos)?;
    class_at(b, start)
}

/// `Ctrl+Right`: end of the next word; a line end moves past the line break.
pub fn word_right(s: &Snapshot, pos: u64) -> u64 {
    let b = &mut Bytes::new(s);
    let Some(first) = class_at(b, pos) else {
        return s.len();
    };
    if first == Class::Break {
        return next_in(b, pos).unwrap_or(s.len());
    }
    let limit = pos + MOTION_SCAN_LIMIT;
    let mut p = pos;
    while p < limit && class_at(b, p) == Some(Class::Space) {
        p = next_in(b, p).unwrap_or(s.len());
    }
    let kind = match class_at(b, p) {
        None | Some(Class::Break) => return p,
        Some(kind) => kind,
    };
    while p < limit && class_at(b, p) == Some(kind) {
        p = next_in(b, p).unwrap_or(s.len());
    }
    p
}

/// `Ctrl+Left`: start of the previous word; a line start moves before the line break.
pub fn word_left(s: &Snapshot, pos: u64) -> u64 {
    let b = &mut Bytes::new(s);
    let pos = pos.min(s.len());
    let Some(first) = class_before(b, pos) else {
        return 0;
    };
    if first == Class::Break {
        return prev_in(b, pos).unwrap_or(0);
    }
    let limit = pos.saturating_sub(MOTION_SCAN_LIMIT);
    let mut p = pos;
    while p > limit && class_before(b, p) == Some(Class::Space) {
        p = prev_in(b, p).unwrap_or(0);
    }
    let kind = match class_before(b, p) {
        None | Some(Class::Break) => return p,
        Some(kind) => kind,
    };
    while p > limit && class_before(b, p) == Some(kind) {
        p = prev_in(b, p).unwrap_or(0);
    }
    p
}

/// Run of same-class characters around `pos` (double-click selection),
/// at most [`MOTION_SCAN_LIMIT`] bytes on each side.
pub fn word_at(s: &Snapshot, pos: u64) -> Range<u64> {
    let b = &mut Bytes::new(s);
    let pos = pos.min(s.len());
    let kind = match (class_at(b, pos), class_before(b, pos)) {
        (Some(kind), _) if kind != Class::Break => kind,
        (_, Some(kind)) if kind != Class::Break => kind,
        _ => return pos..pos,
    };
    let floor = pos.saturating_sub(MOTION_SCAN_LIMIT);
    let mut start = pos;
    while start > floor && class_before(b, start) == Some(kind) {
        start = prev_in(b, start).unwrap_or(0);
    }
    let ceiling = pos + MOTION_SCAN_LIMIT;
    let mut end = pos;
    while end < ceiling && class_at(b, end) == Some(kind) {
        end = next_in(b, end).unwrap_or(s.len());
    }
    start..end
}
