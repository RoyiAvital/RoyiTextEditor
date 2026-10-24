//! Code awareness from syntax trees: matching brackets, enclosing nodes for
//! Expand Selection, and symbols for Go to Symbol. Without a tree, brackets
//! fall back to a bounded text scan.

use std::ops::{ControlFlow, Range};

use rte_text::Snapshot;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, QueryCursor, QueryCursorOptions, QueryCursorState};

use crate::View;

/// Bytes scanned for a matching bracket without a syntax tree.
pub const BRACKET_SCAN_LIMIT: u64 = 64 * 1024;

fn byte_at(s: &Snapshot, pos: u64) -> Option<u8> {
    s.chunk_at(pos)
        .map(|(start, bytes)| bytes[(pos - start) as usize])
}

/// The bracket next to `pos` (before it first, then after it) and its
/// partner: `(bracket, partner)` offsets. The innermost syntax tree at the
/// bracket decides (injected code first); brackets inside strings and
/// comments of a parsed file are not matched.
pub fn matching_bracket(
    s: &Snapshot,
    view: Option<&View>,
    brackets: &[(char, char)],
    pos: u64,
) -> Option<(u64, u64)> {
    let candidates = [pos.checked_sub(1), Some(pos)];
    'candidates: for at in candidates.into_iter().flatten() {
        let Some(b) = byte_at(s, at) else {
            continue;
        };
        let Some(&(open, close)) = brackets
            .iter()
            .find(|&&(o, c)| o.is_ascii() && c.is_ascii() && (o as u8 == b || c as u8 == b))
        else {
            continue;
        };
        let opening = open as u8 == b;
        let mut buf = [0u8; 4];
        let token = char::from(b).encode_utf8(&mut buf) as &str;
        for layer in view.into_iter().flat_map(|v| v.layers_at(at)) {
            let injected = layer.end != u64::MAX;
            if injected && !in_included_ranges(&layer.tree, at) {
                continue;
            }
            let root = layer.tree.root_node();
            let Some(node) = root.descendant_for_byte_range(at as usize, at as usize + 1) else {
                continue;
            };
            if node.start_byte() != at as usize
                || node.end_byte() != at as usize + 1
                || node.kind() != token
            {
                // Inside a string, comment or other token of this language.
                continue 'candidates;
            }
            match tree_partner(node, open, close, opening) {
                Some(partner) => return Some((at, partner)),
                None => break,
            }
        }
        if let Some(partner) = scan(s, at, open as u8, close as u8, opening) {
            return Some((at, partner));
        }
    }
    None
}

fn in_included_ranges(tree: &tree_sitter::Tree, at: u64) -> bool {
    tree.included_ranges()
        .iter()
        .any(|r| (r.start_byte as u64..r.end_byte as u64).contains(&at))
}

/// Siblings visited when the partner is not the parent's first or last child.
const SIBLING_WALK_LIMIT: usize = 256;

/// The sibling token closing (or opening) `node` within its parent: usually
/// the parent's last (first) child; otherwise a bounded walk.
fn tree_partner(node: Node<'_>, open: char, close: char, opening: bool) -> Option<u64> {
    let parent = node.parent()?;
    let mut buf = [0u8; 4];
    let want = if opening { close } else { open }.encode_utf8(&mut buf) as &str;
    let is_partner = |n: &Node<'_>| n.kind() == want && n.end_byte() - n.start_byte() == 1;
    let edge = if opening {
        parent.child(parent.child_count().checked_sub(1)?)
    } else {
        parent.child(0)
    };
    if let Some(n) = edge.filter(|n| is_partner(n) && n.id() != node.id()) {
        return Some(n.start_byte() as u64);
    }
    let mut sibling = if opening {
        node.next_sibling()
    } else {
        node.prev_sibling()
    };
    for _ in 0..SIBLING_WALK_LIMIT {
        let n = sibling?;
        if is_partner(&n) {
            return Some(n.start_byte() as u64);
        }
        sibling = if opening {
            n.next_sibling()
        } else {
            n.prev_sibling()
        };
    }
    None
}

/// Depth-counting scan of at most [`BRACKET_SCAN_LIMIT`] bytes.
fn scan(s: &Snapshot, at: u64, open: u8, close: u8, forward: bool) -> Option<u64> {
    let mut depth = 0u32;
    if forward {
        let end = s.len().min(at + 1 + BRACKET_SCAN_LIMIT);
        let mut pos = at + 1;
        for chunk in s.chunks(at + 1..end) {
            for &b in chunk {
                if b == open {
                    depth += 1;
                } else if b == close {
                    if depth == 0 {
                        return Some(pos);
                    }
                    depth -= 1;
                }
                pos += 1;
            }
        }
    } else {
        let start = at.saturating_sub(BRACKET_SCAN_LIMIT);
        let bytes = s.to_vec(start..at);
        for (i, &b) in bytes.iter().enumerate().rev() {
            if b == close {
                depth += 1;
            } else if b == open {
                if depth == 0 {
                    return Some(start + i as u64);
                }
                depth -= 1;
            }
        }
    }
    None
}

/// The smallest syntax node strictly larger than `range` that contains it:
/// in the innermost tree at `range` (injected code first), then outside it.
pub fn enclosing_node(view: &View, range: Range<u64>) -> Option<Range<u64>> {
    for layer in view.layers_at(range.start) {
        let injected = layer.end != u64::MAX;
        if injected
            && (!in_included_ranges(&layer.tree, range.start)
                || range.start < layer.start
                || range.end > layer.end)
        {
            continue;
        }
        let Some(mut node) = layer
            .tree
            .root_node()
            .descendant_for_byte_range(range.start as usize, range.end as usize)
        else {
            continue;
        };
        loop {
            let r = node.start_byte() as u64..node.end_byte() as u64;
            if r.start <= range.start && range.end <= r.end && r != range {
                // An injected tree's root may span gaps of its region.
                if !injected || (layer.start <= r.start && r.end <= layer.end) {
                    return Some(r);
                }
                break;
            }
            match node.parent() {
                Some(parent) => node = parent,
                None => break,
            }
        }
    }
    None
}

/// A definition found by the language's tags query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    /// `function`, `class`, `method`, ... (the `@definition.<kind>` suffix).
    pub kind: String,
    /// Offset of the name.
    pub start: u64,
}

/// Definitions in the host tree, in document order: at most `limit` (the
/// flag tells whether more exist); `None` when `stop` returned true.
pub fn symbols(
    view: &View,
    s: &Snapshot,
    limit: usize,
    stop: &dyn Fn() -> bool,
) -> Option<(Vec<Symbol>, bool)> {
    let (Some(tree), Some(hl)) = (view.tree(), view.highlighter()) else {
        return Some((Vec::new(), false));
    };
    let Some(query) = hl.tags() else {
        return Some((Vec::new(), false));
    };
    if stop() {
        return None;
    }
    let names = query.capture_names();
    let mut cursor = QueryCursor::new();
    let text = |n: Node<'_>| crate::highlight::predicate_text(s, n);
    // Polled while the query walks the tree, also when nothing matches.
    let mut stopped = false;
    let mut check = |_: &QueryCursorState| {
        if stop() {
            stopped = true;
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    };
    let options = QueryCursorOptions::new().progress_callback(&mut check);
    let mut matches = cursor.matches_with_options(query, tree.root_node(), text, options);
    let mut out: Vec<Symbol> = Vec::new();
    let mut truncated = false;
    while let Some(m) = matches.next() {
        let mut name = None;
        let mut kind = None;
        for capture in m.captures() {
            let capture_name = names[capture.index as usize];
            if capture_name == "name" {
                name = Some(capture.node);
            } else if let Some(k) = capture_name.strip_prefix("definition.") {
                kind = Some(k);
            }
        }
        if let (Some(node), Some(kind)) = (name, kind) {
            if out.len() == limit {
                truncated = true;
                break;
            }
            let range = node.start_byte() as u64..node.end_byte() as u64;
            let text =
                String::from_utf8_lossy(&s.to_vec(range.start..range.end.min(range.start + 256)))
                    .lines()
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_owned();
            if !text.is_empty() {
                out.push(Symbol {
                    name: text,
                    kind: kind.to_owned(),
                    start: range.start,
                });
            }
        }
    }
    drop(matches);
    if stopped {
        return None;
    }
    out.sort_by_key(|sym| sym.start);
    out.dedup_by(|a, b| a.start == b.start && a.name == b.name);
    Some((out, truncated))
}
