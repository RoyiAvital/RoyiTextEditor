//! Pure edit commands: `(snapshot, selections, options) -> Command`.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::ops::Range;

use rte_text::{Bias, ChangeSet, Edit, EditError, LineEnding, Snapshot, Transaction};

use crate::columns::COLUMN_SCAN_LIMIT;
use crate::motion::{
    LineWalk, content_range, line_end, next_boundary, prev_boundary, word_left, word_right,
};
use crate::selection::{Selection, Selections};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditOptions {
    pub tab_width: u32,
    /// `Tab` inserts spaces up to the next tab stop instead of `\t`.
    pub insert_spaces: bool,
    /// Line break used for new lines and pasted text.
    pub line_ending: LineEnding,
    /// Typing replaces the next character (`Insert` key).
    pub overtype: bool,
}

impl Default for EditOptions {
    fn default() -> EditOptions {
        EditOptions {
            tab_width: 4,
            insert_spaces: true,
            line_ending: LineEnding::CrLf,
            overtype: false,
        }
    }
}

/// How a command coalesces in the undo history.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditKind {
    Typing,
    DeleteBack,
    DeleteForward,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    pub tx: Transaction,
    pub kind: EditKind,
    /// Selections become carets after the edited text.
    pub collapse: bool,
}

/// Applies `cmd`. Collapsing commands leave carets after the edited text;
/// other selections grow to include text inserted at their edges.
pub fn apply(
    s: &Snapshot,
    sels: &Selections,
    cmd: &Command,
) -> Result<(Snapshot, Selections, ChangeSet), EditError> {
    let (next, changes) = s.apply(&cmd.tx)?;
    let sels = sels.map(|sel| {
        if cmd.collapse {
            // A caret touched by an edit lands after that edit's new text.
            let edits = changes.changes();
            let i = edits.partition_point(|c| c.old.start <= sel.end());
            if i > 0 && edits[i - 1].old.end >= sel.start() {
                let c = &edits[i - 1];
                return Selection::caret(changes.map(c.old.start, Bias::Left) + c.new_len);
            }
            return Selection::caret(changes.map(sel.end(), Bias::Right));
        }
        let (anchor_bias, head_bias) = if sel.anchor <= sel.head {
            (Bias::Left, Bias::Right)
        } else {
            (Bias::Right, Bias::Left)
        };
        Selection::new(
            changes.map(sel.anchor, anchor_bias),
            changes.map(sel.head, head_bias),
        )
    });
    Ok((next, sels, changes))
}

/// Sorted edits; overlapping ranges and repeated insertion points merge.
pub(crate) fn build(mut edits: Vec<(Range<u64>, Vec<u8>)>) -> Transaction {
    edits.sort_by_key(|(r, _)| (r.start, r.end));
    let mut out: Vec<(Range<u64>, Vec<u8>)> = Vec::with_capacity(edits.len());
    for (range, text) in edits {
        if let Some((last, last_text)) = out.last_mut()
            && (range.start < last.end
                || (range.is_empty() && last.is_empty() && range.start == last.start))
        {
            last.end = last.end.max(range.end);
            last_text.extend_from_slice(&text);
            continue;
        }
        out.push((range, text));
    }
    Transaction::new(out.into_iter().map(|(r, t)| Edit::new(r, t)).collect())
}

fn deletion(ranges: Vec<Range<u64>>, kind: EditKind) -> Option<Command> {
    let edits: Vec<_> = ranges
        .into_iter()
        .filter(|r| !r.is_empty())
        .map(|r| (r, Vec::new()))
        .collect();
    (!edits.is_empty()).then(|| Command {
        tx: build(edits),
        kind,
        collapse: true,
    })
}

/// Typing or inserting `text` at every selection.
pub fn insert(s: &Snapshot, sels: &Selections, text: &[u8], opts: &EditOptions) -> Command {
    let edits = sels
        .as_slice()
        .iter()
        .map(|sel| {
            let mut range = sel.range();
            if opts.overtype && sel.is_empty() && range.start < line_end(s, range.start) {
                range.end = next_boundary(s, range.start);
            }
            (range, text.to_vec())
        })
        .collect();
    let typing = std::str::from_utf8(text)
        .is_ok_and(|t| t.chars().count() == 1 && !t.contains(['\n', '\r']));
    Command {
        tx: build(edits),
        kind: if typing {
            EditKind::Typing
        } else {
            EditKind::Other
        },
        collapse: true,
    }
}

/// Leading spaces and tabs of the line holding `pos`, up to `pos` (at most
/// [`COLUMN_SCAN_LIMIT`] bytes). Reads only the indentation.
pub(crate) fn indent_before(s: &Snapshot, pos: u64) -> Vec<u8> {
    let start = content_range(s, s.line_of(pos)).start;
    let upto = pos.min(start + COLUMN_SCAN_LIMIT as u64);
    let mut indent = Vec::new();
    for chunk in s.chunks(start..upto) {
        let n = chunk
            .iter()
            .take_while(|&&b| b == b' ' || b == b'\t')
            .count();
        indent.extend_from_slice(&chunk[..n]);
        if n < chunk.len() {
            break;
        }
    }
    indent
}

/// The last byte before `pos` on its line that is not a space or tab;
/// `None` when there is none within [`COLUMN_SCAN_LIMIT`] bytes.
pub(crate) fn last_non_blank_before(s: &Snapshot, pos: u64) -> Option<u8> {
    let start = content_range(s, s.line_of(pos))
        .start
        .max(pos.saturating_sub(COLUMN_SCAN_LIMIT as u64));
    let mut end = pos;
    while end > start {
        let from = end.saturating_sub(256).max(start);
        let found = s
            .to_vec(from..end)
            .into_iter()
            .rev()
            .find(|b| !matches!(b, b' ' | b'\t'));
        if found.is_some() {
            return found;
        }
        end = from;
    }
    None
}

/// `Enter`: a line break plus the indentation before the caret.
pub fn newline(s: &Snapshot, sels: &Selections, opts: &EditOptions) -> Command {
    let edits = sels
        .as_slice()
        .iter()
        .map(|sel| {
            let mut text = opts.line_ending.as_bytes().to_vec();
            text.extend_from_slice(&indent_before(s, sel.start()));
            (sel.range(), text)
        })
        .collect();
    Command {
        tx: build(edits),
        kind: EditKind::Other,
        collapse: true,
    }
}

pub fn backspace(s: &Snapshot, sels: &Selections) -> Option<Command> {
    let ranges = sels
        .as_slice()
        .iter()
        .map(|sel| {
            if sel.is_empty() {
                prev_boundary(s, sel.head)..sel.head
            } else {
                sel.range()
            }
        })
        .collect();
    deletion(ranges, EditKind::DeleteBack)
}

pub fn delete_forward(s: &Snapshot, sels: &Selections) -> Option<Command> {
    let ranges = sels
        .as_slice()
        .iter()
        .map(|sel| {
            if sel.is_empty() {
                sel.head..next_boundary(s, sel.head)
            } else {
                sel.range()
            }
        })
        .collect();
    deletion(ranges, EditKind::DeleteForward)
}

pub fn delete_word_left(s: &Snapshot, sels: &Selections) -> Option<Command> {
    let ranges = sels
        .as_slice()
        .iter()
        .map(|sel| {
            if sel.is_empty() {
                word_left(s, sel.head)..sel.head
            } else {
                sel.range()
            }
        })
        .collect();
    deletion(ranges, EditKind::Other)
}

pub fn delete_word_right(s: &Snapshot, sels: &Selections) -> Option<Command> {
    let ranges = sels
        .as_slice()
        .iter()
        .map(|sel| {
            if sel.is_empty() {
                sel.head..word_right(s, sel.head)
            } else {
                sel.range()
            }
        })
        .collect();
    deletion(ranges, EditKind::Other)
}

/// Lines touched by the selections; a selection ending at a line start does
/// not include that line.
pub(crate) fn covered_lines(s: &Snapshot, sels: &Selections) -> BTreeSet<u64> {
    let mut lines = BTreeSet::new();
    for sel in sels.as_slice() {
        let first = s.line_of(sel.start());
        let mut last = s.line_of(sel.end());
        if last > first && s.line_start(last) == Some(sel.end()) {
            last -= 1;
        }
        lines.extend(first..=last);
    }
    lines
}

pub(crate) fn indent_unit(opts: &EditOptions) -> Vec<u8> {
    if opts.insert_spaces {
        vec![b' '; opts.tab_width.max(1) as usize]
    } else {
        b"\t".to_vec()
    }
}

/// `Tab`: indents the lines of multi-line selections, otherwise inserts a
/// tab or spaces up to the next tab stop.
pub fn tab(s: &Snapshot, sels: &Selections, opts: &EditOptions) -> Command {
    let multi_line = sels
        .as_slice()
        .iter()
        .any(|sel| s.line_of(sel.start()) != s.line_of(sel.end()));
    if multi_line {
        let unit = indent_unit(opts);
        let edits = covered_lines(s, sels)
            .into_iter()
            .map(|line| content_range(s, line))
            .filter(|content| !content.is_empty())
            .map(|content| (content.start..content.start, unit.clone()))
            .collect();
        return Command {
            tx: build(edits),
            kind: EditKind::Other,
            collapse: false,
        };
    }
    let tab = u64::from(opts.tab_width.max(1));
    let mut walk = LineWalk::default();
    let edits = sels
        .as_slice()
        .iter()
        .map(|sel| {
            let text = if opts.insert_spaces {
                let col = walk.column(s, sel.start(), opts.tab_width);
                vec![b' '; (tab - col % tab) as usize]
            } else {
                b"\t".to_vec()
            };
            (sel.range(), text)
        })
        .collect();
    Command {
        tx: build(edits),
        kind: EditKind::Other,
        collapse: true,
    }
}

/// `Shift+Tab`: removes one level of indentation (a tab or up to
/// `tab_width` spaces) from every touched line.
pub fn outdent(s: &Snapshot, sels: &Selections, opts: &EditOptions) -> Option<Command> {
    let width = opts.tab_width.max(1) as u64;
    let edits: Vec<_> = covered_lines(s, sels)
        .into_iter()
        .filter_map(|line| {
            let content = content_range(s, line);
            let prefix = s.to_vec(content.start..content.end.min(content.start + width));
            let n = if prefix.first() == Some(&b'\t') {
                1
            } else {
                prefix.iter().take_while(|&&b| b == b' ').count() as u64
            };
            (n > 0).then(|| (content.start..content.start + n, Vec::new()))
        })
        .collect();
    (!edits.is_empty()).then(|| Command {
        tx: build(edits),
        kind: EditKind::Other,
        collapse: false,
    })
}

/// Clipboard content: one piece per selection, or whole lines (`line_mode`)
/// copied from empty selections.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clip {
    pub pieces: Vec<Vec<u8>>,
    pub line_mode: bool,
}

impl Clip {
    /// Plain-text form: line-mode pieces end with their break; other pieces
    /// are joined with `line_ending`.
    pub fn text(&self, line_ending: LineEnding) -> Vec<u8> {
        if self.line_mode {
            self.pieces.concat()
        } else {
            self.pieces.join(line_ending.as_bytes())
        }
    }
}

fn empty_selection_lines(s: &Snapshot, sels: &Selections) -> Vec<u64> {
    let mut lines: Vec<u64> = sels
        .as_slice()
        .iter()
        .map(|sel| s.line_of(sel.head))
        .collect();
    lines.dedup();
    lines
}

/// `Ctrl+C`: the selected text, or the caret lines when nothing is selected.
pub fn copy(s: &Snapshot, sels: &Selections, opts: &EditOptions) -> Clip {
    if sels.as_slice().iter().all(Selection::is_empty) {
        let pieces = empty_selection_lines(s, sels)
            .into_iter()
            .map(|line| {
                let range = s.line_range(line).expect("caret line exists");
                if range.end < s.len() {
                    s.to_vec(range.start..range.end + 1)
                } else {
                    let mut piece = s.to_vec(range);
                    piece.extend_from_slice(opts.line_ending.as_bytes());
                    piece
                }
            })
            .collect();
        return Clip {
            pieces,
            line_mode: true,
        };
    }
    Clip {
        pieces: sels
            .as_slice()
            .iter()
            .filter(|sel| !sel.is_empty())
            .map(|sel| s.to_vec(sel.range()))
            .collect(),
        line_mode: false,
    }
}

/// `Ctrl+X`: [`copy`] plus deleting what was copied (whole lines with their
/// break in line mode; the last line takes the preceding break).
pub fn cut(s: &Snapshot, sels: &Selections, opts: &EditOptions) -> (Clip, Option<Command>) {
    let clip = copy(s, sels, opts);
    let ranges = if clip.line_mode {
        empty_selection_lines(s, sels)
            .into_iter()
            .map(|line| {
                let range = s.line_range(line).expect("caret line exists");
                if range.end < s.len() {
                    range.start..range.end + 1
                } else if line > 0 {
                    content_range(s, line - 1).end..range.end
                } else {
                    range
                }
            })
            .collect()
    } else {
        sels.as_slice().iter().map(Selection::range).collect()
    };
    (clip, deletion(ranges, EditKind::Other))
}

/// Converts `\r\n` and `\n` to `line_ending`; lone `\r` is kept. Text that
/// already uses `line_ending` is returned as is, without copying.
fn normalize_breaks(text: Cow<'_, [u8]>, line_ending: LineEnding) -> Cow<'_, [u8]> {
    let lone_lf = |i: usize| text[i] == b'\n' && (i == 0 || text[i - 1] != b'\r');
    let (changes, len) = match line_ending {
        LineEnding::Lf => {
            let crlf = text.windows(2).filter(|w| *w == b"\r\n").count();
            (crlf, text.len() - crlf)
        }
        LineEnding::CrLf => {
            let lf = (0..text.len()).filter(|&i| lone_lf(i)).count();
            (lf, text.len() + lf)
        }
    };
    if changes == 0 {
        return text;
    }
    let mut out = Vec::with_capacity(len);
    let mut i = 0;
    while i < text.len() {
        match text[i] {
            b'\r' if text.get(i + 1) == Some(&b'\n') => {
                out.extend_from_slice(line_ending.as_bytes());
                i += 2;
            }
            b'\n' => {
                out.extend_from_slice(line_ending.as_bytes());
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    Cow::Owned(out)
}

/// The lines of `text` (one trailing break removed) when there are exactly
/// `n` of them: VS Code spreads such a paste over `n` cursors.
fn spread_lines(text: &[u8], n: usize) -> Option<Vec<Vec<u8>>> {
    let text = text.strip_suffix(b"\n").unwrap_or(text);
    let text = text.strip_suffix(b"\r").unwrap_or(text);
    let mut lines = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < text.len() {
        let width = match (text[i], text.get(i + 1)) {
            (b'\r', Some(b'\n')) => 2,
            (b'\r' | b'\n', _) => 1,
            _ => {
                i += 1;
                continue;
            }
        };
        if lines.len() + 1 >= n {
            return None;
        }
        lines.push(text[start..i].to_vec());
        i += width;
        start = i;
    }
    lines.push(text[start..].to_vec());
    (lines.len() == n).then_some(lines)
}

/// `Ctrl+V`. One piece per selection when the counts match (several
/// selections), or one line per selection when plain text has as many lines;
/// otherwise the whole text at each. Line-mode text pastes above the caret
/// line when the selection is empty.
pub fn paste(s: &Snapshot, sels: &Selections, clip: &Clip, opts: &EditOptions) -> Option<Command> {
    let eol = opts.line_ending;
    let spread = match clip.pieces.as_slice() {
        [piece] if sels.len() > 1 && !clip.line_mode => spread_lines(piece, sels.len()),
        _ => None,
    };
    let texts: Vec<Vec<u8>> = if let Some(lines) = spread {
        lines
    } else if sels.len() > 1 && clip.pieces.len() == sels.len() {
        clip.pieces
            .iter()
            .map(|p| normalize_breaks(Cow::Borrowed(p), eol).into_owned())
            .collect()
    } else {
        let joined = match clip.pieces.as_slice() {
            [piece] => Cow::Borrowed(piece.as_slice()),
            _ => Cow::Owned(clip.text(eol)),
        };
        let whole = normalize_breaks(joined, eol).into_owned();
        // The last selection takes the buffer itself.
        let mut texts = Vec::with_capacity(sels.len());
        for _ in 1..sels.len() {
            texts.push(whole.clone());
        }
        texts.push(whole);
        texts
    };
    if texts.iter().all(Vec::is_empty) {
        return None;
    }
    let edits = sels
        .as_slice()
        .iter()
        .zip(texts)
        .map(|(sel, text)| {
            if clip.line_mode && sel.is_empty() {
                let at = content_range(s, s.line_of(sel.head)).start;
                (at..at, text)
            } else {
                (sel.range(), text)
            }
        })
        .collect();
    Some(Command {
        tx: build(edits),
        kind: EditKind::Other,
        collapse: true,
    })
}
