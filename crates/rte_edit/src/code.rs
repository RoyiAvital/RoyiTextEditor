//! Language-aware edits that only need the text and a language's tokens:
//! comments, auto-closing pairs and indentation after `Enter`. Each returns
//! the command and the selections after it. The rules come from a function
//! of the position, so embedded languages (a `<script>` in HTML, a code
//! fence in Markdown) use their own.

use std::ops::Range;

use rte_text::{Bias, Change, ChangeSet, Snapshot};

use crate::columns::COLUMN_SCAN_LIMIT;
use crate::commands::{Command, EditKind, EditOptions, build, covered_lines, indent_unit};
use crate::motion::content_range;
use crate::selection::{Selection, Selections};

/// A command and the selections it leaves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Coded {
    pub cmd: Command,
    pub selections: Selections,
}

/// Comment tokens of a language.
#[derive(Clone, Copy, Debug, Default)]
pub struct CommentTokens<'a> {
    pub line: Option<&'a str>,
    pub block: Option<(&'a str, &'a str)>,
}

/// Characters after a caret that allow an automatic closing character.
const CLOSE_BEFORE: &[u8] = b";:.,=}])>` \t\r\n";
/// Whitespace scanned at each edge of a selection by Toggle Block Comment.
const EDGE_SCAN: u64 = 64 * 1024;

pub(crate) fn changes_of(cmd: &Command) -> ChangeSet {
    ChangeSet::from_changes(
        cmd.tx
            .edits()
            .iter()
            .map(|e| Change {
                old: e.range.clone(),
                new_len: e.text.len() as u64,
            })
            .collect(),
    )
}

fn byte_at(s: &Snapshot, pos: u64) -> Option<u8> {
    s.chunk_at(pos)
        .map(|(start, bytes)| bytes[(pos - start) as usize])
}

/// Leading spaces and tabs of a line content range.
pub(crate) fn indent_len(s: &Snapshot, content: &Range<u64>) -> u64 {
    let limit = content.end.min(content.start + COLUMN_SCAN_LIMIT as u64);
    let mut p = content.start;
    while p < limit && matches!(byte_at(s, p), Some(b' ' | b'\t')) {
        p += 1;
    }
    p - content.start
}

fn starts_with(s: &Snapshot, pos: u64, text: &[u8]) -> bool {
    s.to_vec(pos..pos + text.len() as u64) == text
}

/// What a line comment command does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommentMode {
    /// `Ctrl+/`: uncomment when every line is commented, else comment all.
    Toggle,
    /// `Ctrl+K Ctrl+C`: comment every line (also commented ones).
    Add,
    /// `Ctrl+K Ctrl+U`: uncomment the commented lines.
    Remove,
}

/// `Ctrl+/`: comments the touched lines, or uncomments them when every
/// non-blank one is commented. Lines of a language without a line comment
/// token are wrapped in its block tokens instead; lines without tokens stay.
pub fn toggle_line_comment<'a>(
    s: &Snapshot,
    sels: &Selections,
    tokens_at: &dyn Fn(u64) -> CommentTokens<'a>,
) -> Option<Coded> {
    line_comment(s, sels, CommentMode::Toggle, tokens_at)
}

/// Line comment command of `mode` over the touched non-blank lines (see
/// [`toggle_line_comment`] for the tokens). `None` when nothing changes.
pub fn line_comment<'a>(
    s: &Snapshot,
    sels: &Selections,
    mode: CommentMode,
    tokens_at: &dyn Fn(u64) -> CommentTokens<'a>,
) -> Option<Coded> {
    // (content, text start, open, close) of each non-blank line.
    let lines: Vec<(Range<u64>, u64, &str, Option<&str>)> = covered_lines(s, sels)
        .into_iter()
        .map(|line| content_range(s, line))
        .filter(|c| indent_len(s, c) < c.end - c.start)
        .filter_map(|c| {
            let text_start = c.start + indent_len(s, &c);
            let tokens = tokens_at(text_start);
            let (open, close) = match (tokens.line, tokens.block) {
                (Some(line), _) => (line, None),
                (None, Some((open, close))) => (open, Some(close)),
                (None, None) => return None,
            };
            Some((c, text_start, open, close))
        })
        .collect();
    if lines.is_empty() {
        return None;
    }
    let is_commented = |(c, text_start, open, close): &(Range<u64>, u64, &str, Option<&str>)| {
        starts_with(s, *text_start, open.as_bytes())
            && close.is_none_or(|close| {
                let end = trimmed_end(s, c);
                end >= text_start + (open.len() + close.len()) as u64
                    && starts_with(s, end - close.len() as u64, close.as_bytes())
            })
    };
    let remove = match mode {
        CommentMode::Toggle => lines.iter().all(is_commented),
        CommentMode::Add => false,
        CommentMode::Remove => true,
    };
    let mut edits = Vec::new();
    if remove {
        for (c, start, open, close) in lines.iter().filter(|l| is_commented(l)) {
            let mut end = start + open.len() as u64;
            if byte_at(s, end) == Some(b' ') {
                end += 1;
            }
            edits.push((*start..end, Vec::new()));
            if let Some(close) = close {
                let close_end = trimmed_end(s, c);
                let mut close_start = close_end - close.len() as u64;
                if close_start > end && byte_at(s, close_start - 1) == Some(b' ') {
                    close_start -= 1;
                }
                edits.push((close_start..close_end, Vec::new()));
            }
        }
    } else {
        let indent = lines
            .iter()
            .map(|(c, text_start, ..)| text_start - c.start)
            .min()
            .unwrap_or(0);
        for (c, _, open, close) in &lines {
            let at = c.start + indent;
            edits.push((at..at, format!("{open} ").into_bytes()));
            if let Some(close) = close {
                let end = trimmed_end(s, c);
                edits.push((end..end, format!(" {close}").into_bytes()));
            }
        }
    }
    if edits.is_empty() {
        return None;
    }
    let cmd = Command {
        tx: build(edits),
        kind: EditKind::Other,
        collapse: false,
    };
    let changes = changes_of(&cmd);
    let selections = sels.map(|sel| {
        Selection::new(
            changes.map(sel.anchor, Bias::Right),
            changes.map(sel.head, Bias::Right),
        )
    });
    Some(Coded { cmd, selections })
}

/// End of a line's content without trailing spaces.
fn trimmed_end(s: &Snapshot, c: &Range<u64>) -> u64 {
    let mut p = c.end;
    while p > c.start {
        let Some((chunk_start, bytes)) = s.chunk_at(p - 1) else {
            break;
        };
        let from = chunk_start.max(c.start);
        let slice = &bytes[(from - chunk_start) as usize..(p - chunk_start) as usize];
        match slice.iter().rposition(|b| !matches!(b, b' ' | b'\t')) {
            Some(i) => return from + i as u64 + 1,
            None => p = from,
        }
    }
    p
}

/// `Shift+Alt+A`: wraps each selection in block comment tokens, or removes
/// them when the selection is already wrapped. An empty selection gets an
/// empty comment with the caret inside. Only the edges of a selection are
/// read. `None` when no selection has block tokens.
pub fn toggle_block_comment<'a>(
    s: &Snapshot,
    sels: &Selections,
    tokens_at: &dyn Fn(u64) -> Option<(&'a str, &'a str)>,
) -> Option<Coded> {
    let mut edits = Vec::new();
    // What happens to each selection.
    enum Plan {
        Keep(Selection),
        Empty(u64, usize),
        Wrap(Selection),
        Unwrap(Selection),
    }
    let mut plans = Vec::new();
    for sel in sels.as_slice() {
        let range = sel.range();
        let Some((open, close)) = tokens_at(range.start) else {
            plans.push(Plan::Keep(*sel));
            continue;
        };
        if range.is_empty() {
            let text = format!("{open}  {close}").into_bytes();
            edits.push((range.clone(), text));
            plans.push(Plan::Empty(range.start, open.len()));
            continue;
        }
        let a = skip_space_forward(s, range.clone());
        let b = skip_space_backward(s, a..range.end);
        let inner_len = (open.len() + close.len()) as u64;
        if b >= a + inner_len
            && starts_with(s, a, open.as_bytes())
            && starts_with(s, b - close.len() as u64, close.as_bytes())
        {
            let mut open_end = a + open.len() as u64;
            let mut close_start = b - close.len() as u64;
            if close_start > open_end && byte_at(s, open_end) == Some(b' ') {
                open_end += 1;
            }
            if close_start > open_end && byte_at(s, close_start - 1) == Some(b' ') {
                close_start -= 1;
            }
            edits.push((a..open_end, Vec::new()));
            edits.push((close_start..b, Vec::new()));
            plans.push(Plan::Unwrap(*sel));
        } else {
            edits.push((range.start..range.start, format!("{open} ").into_bytes()));
            edits.push((range.end..range.end, format!(" {close}").into_bytes()));
            plans.push(Plan::Wrap(*sel));
        }
    }
    if edits.is_empty() {
        return None;
    }
    let cmd = Command {
        tx: build(edits),
        kind: EditKind::Other,
        collapse: false,
    };
    let changes = changes_of(&cmd);
    let ranges = plans
        .into_iter()
        .map(|plan| match plan {
            Plan::Keep(sel) => Selection::new(
                changes.map(sel.anchor, Bias::Right),
                changes.map(sel.head, Bias::Right),
            ),
            Plan::Empty(at, open) => {
                Selection::caret(changes.map(at, Bias::Left) + open as u64 + 1)
            }
            Plan::Wrap(sel) | Plan::Unwrap(sel) => inner(&changes, sel),
        })
        .collect();
    Some(Coded {
        cmd,
        selections: Selections::new(ranges, sels.primary_index()),
    })
}

/// First non-whitespace offset of `range`, scanning at most [`EDGE_SCAN`]
/// bytes (beyond that the scan position counts as the text start).
fn skip_space_forward(s: &Snapshot, range: Range<u64>) -> u64 {
    let limit = range.end.min(range.start + EDGE_SCAN);
    let mut p = range.start;
    for chunk in s.chunks(range.start..limit) {
        match chunk.iter().position(|b| !b.is_ascii_whitespace()) {
            Some(i) => return p + i as u64,
            None => p += chunk.len() as u64,
        }
    }
    p
}

/// End of `range` without trailing whitespace, scanning at most
/// [`EDGE_SCAN`] bytes backwards.
fn skip_space_backward(s: &Snapshot, range: Range<u64>) -> u64 {
    let stop = range.start.max(range.end.saturating_sub(EDGE_SCAN));
    let mut p = range.end;
    while p > stop {
        let Some((chunk_start, bytes)) = s.chunk_at(p - 1) else {
            break;
        };
        let from = chunk_start.max(stop);
        let slice = &bytes[(from - chunk_start) as usize..(p - chunk_start) as usize];
        match slice.iter().rposition(|b| !b.is_ascii_whitespace()) {
            Some(i) => return from + i as u64 + 1,
            None => p = from,
        }
    }
    p
}

/// `sel` mapped so it stays on the text between inserted edges.
fn inner(changes: &ChangeSet, sel: Selection) -> Selection {
    let (start, end) = (
        changes.map(sel.start(), Bias::Right),
        changes.map(sel.end(), Bias::Left),
    );
    if sel.anchor <= sel.head {
        Selection::new(start, end)
    } else {
        Selection::new(end, start)
    }
}

/// Typing `ch` with the auto-closing pairs at each selection: surrounds
/// selections, inserts the closing character after carets, or steps over a
/// closing character already there. `None` when the character is typed
/// normally (at some selection the rules do not apply).
pub fn auto_close<'a>(
    s: &Snapshot,
    sels: &Selections,
    ch: char,
    pairs_at: &dyn Fn(u64) -> &'a [(char, char)],
) -> Option<Coded> {
    let sel_list = sels.as_slice();
    let mut buf = [0u8; 4];
    let typed = ch.encode_utf8(&mut buf).as_bytes().to_vec();
    let next_is = |pos: u64| s.to_vec(pos..pos + typed.len() as u64) == typed;
    // Step over a closing character (or quote) the caret is in front of.
    let closes = |pos: u64| pairs_at(pos).iter().any(|&(_, close)| close == ch);
    if sel_list
        .iter()
        .all(|sel| sel.is_empty() && closes(sel.head) && next_is(sel.head))
    {
        let selections = sels.map(|sel| Selection::caret(sel.head + typed.len() as u64));
        return Some(Coded {
            cmd: Command {
                tx: build(Vec::new()),
                kind: EditKind::Typing,
                collapse: true,
            },
            selections,
        });
    }
    let pair = |pos: u64| pairs_at(pos).iter().copied().find(|&(open, _)| open == ch);
    let mut plans = Vec::with_capacity(sel_list.len());
    for sel in sel_list {
        let (open, close) = pair(sel.start())?;
        let mut close_buf = [0u8; 4];
        plans.push((
            open == close,
            close.encode_utf8(&mut close_buf).as_bytes().to_vec(),
        ));
    }
    if sel_list.iter().all(|sel| !sel.is_empty()) {
        let edits = sel_list
            .iter()
            .zip(&plans)
            .flat_map(|(sel, (_, close))| {
                [
                    (sel.start()..sel.start(), typed.clone()),
                    (sel.end()..sel.end(), close.clone()),
                ]
            })
            .collect();
        let cmd = Command {
            tx: build(edits),
            kind: EditKind::Other,
            collapse: false,
        };
        let changes = changes_of(&cmd);
        let selections = sels.map(|sel| inner(&changes, *sel));
        return Some(Coded { cmd, selections });
    }
    let allowed = sel_list.iter().zip(&plans).all(|(sel, (quote, _))| {
        if !sel.is_empty() {
            return false;
        }
        let after_ok = byte_at(s, sel.head).is_none_or(|b| CLOSE_BEFORE.contains(&b));
        // No closing quote after a word character (`don't`) or another quote.
        let before = sel.head.checked_sub(1).and_then(|p| byte_at(s, p));
        let before_ok = !quote
            || before.is_none_or(|b| !(b.is_ascii_alphanumeric() || b == b'_' || b == typed[0]));
        after_ok && before_ok
    });
    if !allowed {
        return None;
    }
    let edits = sel_list
        .iter()
        .zip(&plans)
        .map(|(sel, (_, close))| {
            let mut text = typed.clone();
            text.extend_from_slice(close);
            (sel.range(), text)
        })
        .collect();
    let cmd = Command {
        tx: build(edits),
        kind: EditKind::Typing,
        collapse: true,
    };
    let changes = changes_of(&cmd);
    let selections =
        sels.map(|sel| Selection::caret(changes.map(sel.head, Bias::Left) + typed.len() as u64));
    Some(Coded { cmd, selections })
}

/// `Backspace` between an empty pair (`(|)`) deletes both characters.
pub fn delete_pair<'a>(
    s: &Snapshot,
    sels: &Selections,
    pairs_at: &dyn Fn(u64) -> &'a [(char, char)],
) -> Option<Coded> {
    let mut edits = Vec::new();
    for sel in sels.as_slice() {
        if !sel.is_empty() || sel.head == 0 {
            return None;
        }
        let around = s.to_vec(sel.head - 1..sel.head + 1);
        let pair = pairs_at(sel.head).iter().any(|&(open, close)| {
            open.is_ascii() && close.is_ascii() && around == [open as u8, close as u8]
        });
        if !pair {
            return None;
        }
        edits.push((sel.head - 1..sel.head + 1, Vec::new()));
    }
    let cmd = Command {
        tx: build(edits),
        kind: EditKind::DeleteBack,
        collapse: true,
    };
    let changes = changes_of(&cmd);
    let selections = sels.map(|sel| Selection::caret(changes.map(sel.head, Bias::Left)));
    Some(Coded { cmd, selections })
}

/// `Enter` after an opening bracket (or a `:` ending a line where the
/// rules allow it) indents the new line one level; between a pair (`{|}`)
/// the closing bracket moves to its own line. `rules_at` gives the brackets
/// and the colon rule at a position. `None` when plain `Enter` applies.
pub fn newline_indent<'a>(
    s: &Snapshot,
    sels: &Selections,
    opts: &EditOptions,
    rules_at: &dyn Fn(u64) -> (&'a [(char, char)], bool),
) -> Option<Coded> {
    let unit = indent_unit(opts);
    let eol = opts.line_ending.as_bytes();
    let mut edits = Vec::new();
    let mut carets = Vec::new();
    let mut any = false;
    for sel in sels.as_slice() {
        let (brackets, colon) = rules_at(sel.start());
        let content = content_range(s, s.line_of(sel.start()));
        let upto = sel.start().min(content.start + COLUMN_SCAN_LIMIT as u64);
        let prefix = s.to_vec(content.start..upto);
        let indent: Vec<u8> = prefix
            .iter()
            .copied()
            .take_while(|&b| b == b' ' || b == b'\t')
            .collect();
        let before = prefix
            .iter()
            .rev()
            .find(|b| !matches!(b, b' ' | b'\t'))
            .copied();
        // The text after the selection, on the line where it ends.
        let end_line = content_range(s, s.line_of(sel.end()));
        let after = s
            .to_vec(sel.end()..end_line.end.min(sel.end() + 256))
            .into_iter()
            .find(|b| !matches!(b, b' ' | b'\t'));
        let opener = before.and_then(|b| {
            brackets
                .iter()
                .find(|&&(open, _)| open.is_ascii() && open as u8 == b)
        });
        let deeper = opener.is_some() || (colon && before == Some(b':') && after.is_none());
        let split = opener.is_some_and(|&(_, close)| after == Some(close as u8));
        let mut text = eol.to_vec();
        text.extend_from_slice(&indent);
        if deeper {
            any = true;
            text.extend_from_slice(&unit);
        }
        let caret = text.len() as u64;
        if split {
            text.extend_from_slice(eol);
            text.extend_from_slice(&indent);
        }
        edits.push((sel.range(), text));
        carets.push((sel.start(), caret));
    }
    if !any {
        return None;
    }
    let cmd = Command {
        tx: build(edits),
        kind: EditKind::Other,
        collapse: true,
    };
    let changes = changes_of(&cmd);
    let ranges = carets
        .into_iter()
        .map(|(at, caret)| Selection::caret(changes.map(at, Bias::Left) + caret))
        .collect();
    Some(Coded {
        cmd,
        selections: Selections::new(ranges, sels.primary_index()),
    })
}
