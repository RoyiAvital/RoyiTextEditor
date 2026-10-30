//! Markdown text that is not prose (code, link destinations, HTML, front
//! matter), for spelling. Answered from a verified full parse only.

use std::ops::Range;

use rte_text::Snapshot;
use tree_sitter::{Node, Parser};

use crate::{Grammar, View};

/// Block nodes skipped whole.
const BLOCKS: &[&str] = &[
    "fenced_code_block",
    "indented_code_block",
    "html_block",
    "minus_metadata",
    "plus_metadata",
    "link_reference_definition",
];
/// Inline nodes skipped whole.
const INLINES: &[&str] = &[
    "code_span",
    "link_destination",
    "uri_autolink",
    "email_autolink",
    "html_tag",
    "latex_block",
];
/// Longer inline runs are not parsed; they are skipped whole.
const INLINE_MAX: u64 = 1024 * 1024;

/// Parses the inline elements of Markdown blocks on demand.
pub struct MarkdownMask {
    parser: Parser,
}

impl MarkdownMask {
    pub fn new() -> Result<MarkdownMask, String> {
        let mut parser = Parser::new();
        parser
            .set_language(&Grammar::MarkdownInline.language())
            .map_err(|e| e.to_string())?;
        Ok(MarkdownMask { parser })
    }

    /// Sorted, disjoint ranges of `range` that are not prose, from the
    /// Markdown host tree of `view` (which must be a parse of `snapshot`);
    /// `None` when `view` is not Markdown.
    pub fn exclusions(
        &mut self,
        view: &View,
        snapshot: &Snapshot,
        range: Range<u64>,
    ) -> Option<Vec<Range<u64>>> {
        if view.lang().grammar != Grammar::Markdown {
            return None;
        }
        let tree = view.tree()?;
        let mut out = Vec::new();
        self.walk(tree.root_node(), snapshot, &range, &mut out);
        out.sort_by_key(|r| r.start);
        let mut merged: Vec<Range<u64>> = Vec::with_capacity(out.len());
        for r in out {
            match merged.last_mut() {
                Some(last) if last.end >= r.start => last.end = last.end.max(r.end),
                _ => merged.push(r),
            }
        }
        Some(merged)
    }

    fn walk(
        &mut self,
        node: Node<'_>,
        snapshot: &Snapshot,
        range: &Range<u64>,
        out: &mut Vec<Range<u64>>,
    ) {
        let (start, end) = (node.start_byte() as u64, node.end_byte() as u64);
        if end <= range.start || start >= range.end {
            return;
        }
        let kind = node.kind();
        if BLOCKS.contains(&kind) {
            out.push(start..end);
            return;
        }
        if kind == "inline" {
            self.inline(start..end, snapshot, out);
            return;
        }
        // Only the children that intersect `range`: seek to the first, stop after the last.
        let mut cursor = node.walk();
        if cursor
            .goto_first_child_for_byte(range.start as usize)
            .is_none()
        {
            return;
        }
        loop {
            let child = cursor.node();
            if child.start_byte() as u64 >= range.end {
                break;
            }
            self.walk(child, snapshot, range, out);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }

    fn inline(&mut self, span: Range<u64>, snapshot: &Snapshot, out: &mut Vec<Range<u64>>) {
        if span.end - span.start > INLINE_MAX {
            out.push(span);
            return;
        }
        let bytes = snapshot.to_vec(span.clone());
        let Some(tree) = self.parser.parse(&bytes, None) else {
            out.push(span);
            return;
        };
        let mut stack = vec![tree.root_node()];
        while let Some(node) = stack.pop() {
            if INLINES.contains(&node.kind()) {
                out.push(
                    span.start + node.start_byte() as u64..span.start + node.end_byte() as u64,
                );
                continue;
            }
            let mut cursor = node.walk();
            stack.extend(node.children(&mut cursor));
        }
    }
}
