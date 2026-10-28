//! Whole-line commands (VS Code Basic Editing): move, copy, delete and
//! insert lines, indent lines. Move, Copy and Delete find the touched lines
//! of each selection as blocks (never line by line) and copy no bytes:
//! moved and copied text shares the snapshot's buffers, so their cost does
//! not depend on the size of the lines.

use std::ops::Range;

use rte_text::{Bias, Edit, Piece, Snapshot, Transaction};

use crate::code::{Coded, changes_of, indent_len};
use crate::commands::{Command, EditKind, EditOptions, build, covered_lines, indent_unit};
use crate::motion::{column_at, content_range, offset_in_line};
use crate::selection::{Selection, Selections};

/// Touched lines of the selections as sorted `(first, last)` blocks; blocks
/// that overlap or touch merge. A selection ending at a line start does not
/// touch that line.
pub fn line_blocks(s: &Snapshot, sels: &Selections) -> Vec<(u64, u64)> {
    let mut blocks: Vec<(u64, u64)> = Vec::new();
    for sel in sels.as_slice() {
        let first = s.line_of(sel.start());
        let mut last = s.line_of(sel.end());
        if last > first && s.line_start(last) == Some(sel.end()) {
            last -= 1;
        }
        match blocks.last_mut() {
            Some(block) if first <= block.1 + 1 => block.1 = block.1.max(last),
            _ => blocks.push((first, last)),
        }
    }
    blocks
}

fn line_start(s: &Snapshot, line: u64) -> u64 {
    s.line_start(line).unwrap_or(s.len())
}

/// Start of the line after `line` (the end for the last line).
fn next_start(s: &Snapshot, line: u64) -> u64 {
    s.line_start(line + 1).unwrap_or(s.len())
}

fn command(tx: Transaction) -> Command {
    Command {
        tx,
        kind: EditKind::Other,
        collapse: false,
    }
}

/// Index of the block of `sels`' primary selection.
fn primary_block(s: &Snapshot, sels: &Selections, blocks: &[(u64, u64)]) -> usize {
    let line = s.line_of(sels.primary().start());
    blocks
        .partition_point(|b| b.1 < line)
        .min(blocks.len().saturating_sub(1))
}

/// `Alt+Up` / `Alt+Down`: each block swaps with the line above / below; the
/// selections move with it. The two texts swap and the line break between
/// them stays, so a last line without a break keeps none. Blocks at the
/// first / last line stay. `None` when nothing moves.
pub fn move_lines(s: &Snapshot, sels: &Selections, up: bool) -> Option<Coded> {
    let last = s.line_count() - 1;
    let mut edits = Vec::new();
    // Per moved block: its span with the break after it, its content end,
    // how far the content moves, and the break that follows it afterwards.
    let mut moves: Vec<(Range<u64>, u64, i64, u64)> = Vec::new();
    for (a, b) in line_blocks(s, sels) {
        if (up && a == 0) || (!up && b == last) {
            continue;
        }
        let block = line_start(s, a)..content_range(s, b).end;
        let neighbor = content_range(s, if up { a - 1 } else { b + 1 });
        let (upper, lower) = if up {
            (neighbor.clone(), block.clone())
        } else {
            (block.clone(), neighbor.clone())
        };
        if !(upper.is_empty() && lower.is_empty()) {
            edits.push(Edit::copy(upper.clone(), lower.clone()));
            edits.push(Edit::copy(lower.clone(), upper.clone()));
        }
        let (delta, next_break) = if up {
            let delta = upper.start as i64 - lower.start as i64;
            (delta, lower.start - upper.end)
        } else {
            let after = next_start(s, b + 1) - neighbor.end;
            ((lower.end - upper.end) as i64, after)
        };
        moves.push((block.start..next_start(s, b), block.end, delta, next_break));
    }
    if moves.is_empty() {
        return None;
    }
    let map = |p: u64| {
        let i = moves.partition_point(|m| m.0.end < p);
        match moves.get(i) {
            Some((span, end, delta, next_break)) if span.start <= p => {
                let moved_end = (*end as i64 + delta) as u64;
                if p <= *end {
                    (p as i64 + delta) as u64
                } else {
                    // After the old break: after the break that now follows
                    // (which may be CRLF where the old one was LF).
                    moved_end + next_break
                }
            }
            _ => p,
        }
    };
    let cmd = command(Transaction::new(edits));
    let selections = sels.map(|sel| Selection::new(map(sel.anchor), map(sel.head)));
    Some(Coded { cmd, selections })
}

/// `Shift+Alt+Up` / `Shift+Alt+Down`: duplicates each block above / below,
/// sharing its bytes; the selections end on the copy in the key's direction.
/// A last line without a break gets `opts.line_ending` between the copies.
pub fn copy_lines(s: &Snapshot, sels: &Selections, up: bool, opts: &EditOptions) -> Coded {
    let brk = opts.line_ending.as_bytes();
    let mut edits = Vec::new();
    // Per block: its span and where its copy (holding the selections) starts.
    let mut copies: Vec<(Range<u64>, u64)> = Vec::new();
    let mut inserted = 0;
    for (a, b) in line_blocks(s, sels) {
        let start = line_start(s, a);
        let end = next_start(s, b);
        let content_end = content_range(s, b).end;
        let has_break = end > content_end;
        let (at, pieces, copy_start) = match (up, has_break) {
            (true, true) => (start, vec![Piece::Copy(start..end)], start),
            (true, false) => (
                start,
                vec![Piece::Copy(start..end), Piece::Bytes(Box::from(brk))],
                start,
            ),
            (false, true) => (end, vec![Piece::Copy(start..end)], end),
            (false, false) => (
                end,
                vec![Piece::Bytes(Box::from(brk)), Piece::Copy(start..end)],
                end + brk.len() as u64,
            ),
        };
        let len: u64 = pieces.iter().map(|p| p.len() as u64).sum();
        edits.push(Edit::shared(at..at, pieces));
        copies.push((start..end, copy_start + inserted));
        inserted += len;
    }
    // Every selection lies in its block.
    let map = |p: u64| {
        let (span, copy_start) = &copies[copies.partition_point(|c| c.0.end < p)];
        copy_start + (p - span.start)
    };
    let cmd = command(Transaction::new(edits));
    let selections = sels.map(|sel| Selection::new(map(sel.anchor), map(sel.head)));
    Coded { cmd, selections }
}

/// `Ctrl+Shift+K`: deletes each block with its line break (a block ending
/// at the last line takes the break before it). One caret per block, at the
/// column of its first selection on the line that follows.
pub fn delete_lines(s: &Snapshot, sels: &Selections, opts: &EditOptions) -> Coded {
    let last = s.line_count() - 1;
    let blocks = line_blocks(s, sels);
    let mut ranges = Vec::new();
    let mut targets = Vec::new();
    for &(a, b) in &blocks {
        let start = line_start(s, a);
        let end = next_start(s, b);
        let head = sels
            .as_slice()
            .iter()
            .find(|sel| s.line_of(sel.start()) >= a)
            .map_or(start, |sel| sel.head);
        let col = column_at(s, head, opts.tab_width);
        let (range, target) = if b == last && a > 0 {
            (content_range(s, a - 1).end..end, Some(a - 1))
        } else if b == last {
            (start..end, None)
        } else {
            (start..end, Some(b + 1))
        };
        ranges.push((range, Vec::new()));
        targets.push(target.map_or(0, |line| {
            offset_in_line(s, content_range(s, line), col, opts.tab_width)
        }));
    }
    let cmd = command(build(ranges));
    let changes = changes_of(&cmd);
    let carets = targets
        .iter()
        .map(|&q| Selection::caret(changes.map(q, Bias::Left)))
        .collect();
    let primary = primary_block(s, sels, &blocks);
    Coded {
        cmd,
        selections: Selections::new(carets, primary),
    }
}

/// `Ctrl+Enter` / `Ctrl+Shift+Enter`: a new line below / above the line of
/// every caret, with that line's indentation; the carets move to it.
pub fn insert_line(s: &Snapshot, sels: &Selections, below: bool, opts: &EditOptions) -> Coded {
    let brk = opts.line_ending.as_bytes();
    let mut lines: Vec<u64> = sels
        .as_slice()
        .iter()
        .map(|sel| s.line_of(sel.head))
        .collect();
    lines.sort_unstable();
    lines.dedup();
    let mut edits = Vec::new();
    let mut targets = Vec::new();
    for &line in &lines {
        let content = content_range(s, line);
        let indent = s.to_vec(content.start..content.start + indent_len(s, &content));
        let (at, text, caret) = if below {
            let text = [brk, &indent].concat();
            let caret = text.len() as u64;
            (content.end, text, caret)
        } else {
            let caret = indent.len() as u64;
            (content.start, [&indent[..], brk].concat(), caret)
        };
        edits.push((at..at, text));
        targets.push((at, caret));
    }
    let cmd = command(build(edits));
    let changes = changes_of(&cmd);
    let carets = targets
        .iter()
        .map(|&(at, caret)| Selection::caret(changes.map(at, Bias::Left) + caret))
        .collect();
    let primary_line = s.line_of(sels.primary().head);
    let primary = lines.partition_point(|&l| l < primary_line);
    Coded {
        cmd,
        selections: Selections::new(carets, primary),
    }
}

/// `Ctrl+]`: one indentation unit at the start of every touched non-empty
/// line, also for a caret (unlike `Tab`). `None` when every line is empty.
pub fn indent_lines(s: &Snapshot, sels: &Selections, opts: &EditOptions) -> Option<Coded> {
    let unit = indent_unit(opts);
    let edits: Vec<_> = covered_lines(s, sels)
        .into_iter()
        .map(|line| content_range(s, line))
        .filter(|content| !content.is_empty())
        .map(|content| (content.start..content.start, unit.clone()))
        .collect();
    if edits.is_empty() {
        return None;
    }
    let cmd = command(build(edits));
    let changes = changes_of(&cmd);
    let selections = sels.map(|sel| {
        if sel.is_empty() {
            return Selection::caret(changes.map(sel.head, Bias::Right));
        }
        let (anchor, head) = if sel.anchor <= sel.head {
            (Bias::Left, Bias::Right)
        } else {
            (Bias::Right, Bias::Left)
        };
        Selection::new(changes.map(sel.anchor, anchor), changes.map(sel.head, head))
    });
    Some(Coded { cmd, selections })
}
