//! Compiled queries per grammar and highlight spans of a byte range.

use std::collections::BinaryHeap;
use std::ops::{ControlFlow, Range};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use rte_text::Snapshot;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Node, Query, QueryCursor, QueryCursorOptions};

use crate::{Grammar, Style, style_of};

/// A grammar with its compiled highlight query.
pub struct Highlighter {
    pub grammar: Grammar,
    pub language: Language,
    query: Query,
    /// Style of each capture index; `Some(None)` draws plain text over
    /// outer captures (`@none`).
    styles: Box<[Option<Option<Style>>]>,
    tags: OnceLock<Option<Query>>,
}

impl std::fmt::Debug for Highlighter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Highlighter")
            .field("grammar", &self.grammar)
            .finish_non_exhaustive()
    }
}

/// Trimmed `highlights.scm` of a grammar.
pub fn highlights_source(grammar: Grammar) -> &'static str {
    macro_rules! q {
        ($dir:literal) => {
            include_str!(concat!("../queries/", $dir, "/highlights.scm"))
        };
    }
    match grammar {
        Grammar::Rust => q!("rust"),
        Grammar::C => q!("c"),
        Grammar::Cpp => q!("cpp"),
        Grammar::CSharp => q!("csharp"),
        Grammar::Python => q!("python"),
        Grammar::JavaScript => q!("javascript"),
        Grammar::TypeScript => q!("typescript"),
        Grammar::Tsx => q!("tsx"),
        Grammar::Json => q!("json"),
        Grammar::Toml => q!("toml"),
        Grammar::Yaml => q!("yaml"),
        Grammar::Markdown => q!("markdown"),
        Grammar::MarkdownInline => q!("markdown_inline"),
        Grammar::Html => q!("html"),
        Grammar::Css => q!("css"),
        Grammar::Xml => q!("xml"),
        Grammar::Bash => q!("bash"),
        Grammar::PowerShell => q!("powershell"),
        Grammar::Lua => q!("lua"),
        Grammar::Sql => q!("sql"),
        Grammar::Go => q!("go"),
        Grammar::Java => q!("java"),
        Grammar::Matlab => q!("matlab"),
        Grammar::Julia => q!("julia"),
    }
}

/// `tags.scm` of a grammar (definitions for Go to Symbol), if any: trimmed
/// from upstream, or written for this editor where upstream has none.
pub fn tags_source(grammar: Grammar) -> Option<&'static str> {
    macro_rules! q {
        ($dir:literal) => {
            Some(include_str!(concat!("../queries/", $dir, "/tags.scm")))
        };
    }
    match grammar {
        Grammar::Rust => q!("rust"),
        Grammar::C => q!("c"),
        Grammar::Cpp => q!("cpp"),
        Grammar::CSharp => q!("csharp"),
        Grammar::Python => q!("python"),
        Grammar::JavaScript => q!("javascript"),
        Grammar::TypeScript => q!("typescript"),
        Grammar::Tsx => q!("tsx"),
        Grammar::Json => q!("json"),
        Grammar::Toml => q!("toml"),
        Grammar::Yaml => q!("yaml"),
        Grammar::Markdown => q!("markdown"),
        Grammar::Css => q!("css"),
        Grammar::Bash => q!("bash"),
        Grammar::PowerShell => q!("powershell"),
        Grammar::Lua => q!("lua"),
        Grammar::Sql => q!("sql"),
        Grammar::Go => q!("go"),
        Grammar::Java => q!("java"),
        Grammar::Matlab => q!("matlab"),
        Grammar::Julia => q!("julia"),
        Grammar::MarkdownInline | Grammar::Html | Grammar::Xml => None,
    }
}

type Slot = OnceLock<Result<Arc<Highlighter>, String>>;
static HIGHLIGHTERS: [Slot; Grammar::ALL.len()] = [const { OnceLock::new() }; Grammar::ALL.len()];

/// The highlighter of `grammar`, compiling its query on first use.
pub fn highlighter(grammar: Grammar) -> Result<Arc<Highlighter>, String> {
    HIGHLIGHTERS[grammar as usize]
        .get_or_init(|| {
            let language = grammar.language();
            let query = Query::new(&language, highlights_source(grammar))
                .map_err(|e| format!("{} highlights: {e}", grammar.dir()))?;
            let styles = query
                .capture_names()
                .iter()
                .map(|n| match *n {
                    "none" => Some(None),
                    n => style_of(n).map(Some),
                })
                .collect();
            Ok(Arc::new(Highlighter {
                grammar,
                language,
                query,
                styles,
                tags: OnceLock::new(),
            }))
        })
        .clone()
}

/// Compiles the highlight query of `grammar` on another thread, once.
pub fn warm(grammar: Grammar) {
    static STARTED: [AtomicBool; Grammar::ALL.len()] =
        [const { AtomicBool::new(false) }; Grammar::ALL.len()];
    if HIGHLIGHTERS[grammar as usize].get().is_none()
        && !STARTED[grammar as usize].swap(true, Ordering::Relaxed)
    {
        let _ = std::thread::Builder::new()
            .name("syntax-query".to_owned())
            .spawn(move || highlighter(grammar));
    }
}

impl Highlighter {
    /// Compiled `tags.scm`, if the grammar has one.
    pub fn tags(&self) -> Option<&Query> {
        self.tags
            .get_or_init(|| {
                let source = tags_source(self.grammar)?;
                Query::new(&self.language, source).ok()
            })
            .as_ref()
    }

    /// Adds the captures of `node` inside `range` to `out`; `false` when
    /// `stop` returned true first (the captures found so far are kept).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn captures(
        &self,
        cursor: &mut QueryCursor,
        node: Node<'_>,
        snapshot: &Snapshot,
        range: Range<u64>,
        layer: u8,
        stop: &dyn Fn() -> bool,
        out: &mut Vec<Capture>,
    ) -> bool {
        if stop() {
            return false;
        }
        let mut stopped = false;
        let mut check = |_: &tree_sitter::QueryCursorState| {
            if stop() {
                stopped = true;
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        };
        cursor.set_byte_range(range.start as usize..range.end as usize);
        let text = |n: Node<'_>| predicate_text(snapshot, n);
        let options = QueryCursorOptions::new().progress_callback(&mut check);
        let mut captures = cursor.captures_with_options(&self.query, node, text, options);
        let mut seen = 0u32;
        let mut interrupted = false;
        while let Some((m, index)) = captures.next() {
            seen += 1;
            if seen.is_multiple_of(64) && stop() {
                interrupted = true;
                break;
            }
            let capture = m.captures()[*index];
            let Some(style) = self.styles[capture.index as usize] else {
                continue;
            };
            let start = (capture.node.start_byte() as u64).max(range.start);
            let end = (capture.node.end_byte() as u64).min(range.end);
            if start < end {
                out.push(Capture {
                    start,
                    end,
                    style,
                    layer,
                    pattern: m.pattern_index as u32,
                    len: capture.node.end_byte() as u64 - capture.node.start_byte() as u64,
                });
            }
        }
        drop(captures);
        !stopped && !interrupted
    }
}

/// Bytes of a node's text that query predicates (`#eq?`, `#match?`) read.
pub const PREDICATE_TEXT_MAX: u64 = 1024;

/// Text of `n` for query predicates: at most [`PREDICATE_TEXT_MAX`] bytes.
/// A longer node ends in a NUL there, so predicates on the whole token
/// (`$`, `#eq?`, `#any-of?`) fail instead of matching its prefix.
pub(crate) fn predicate_text<'a>(
    s: &'a Snapshot,
    n: Node<'_>,
) -> impl Iterator<Item = &'a [u8]> + use<'a> {
    let (start, end) = (n.start_byte() as u64, n.end_byte() as u64);
    let cut = end > start + PREDICATE_TEXT_MAX;
    s.chunks(start..end.min(start + PREDICATE_TEXT_MAX))
        .chain(cut.then_some(&b"\0"[..]))
}

/// A styled byte range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: u64,
    pub end: u64,
    pub style: Style,
}

/// A capture clipped to the highlighted range.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Capture {
    pub start: u64,
    pub end: u64,
    /// `None`: plain text.
    pub style: Option<Style>,
    /// 0 for the host, 1 for injected layers (drawn above the host).
    pub layer: u8,
    pub pattern: u32,
    /// Unclipped node length.
    pub len: u64,
}

/// Reusable buffers of [`flatten`].
#[derive(Default)]
pub(crate) struct Flatten {
    /// (offset, 0 end / 1 start, rank).
    events: Vec<(u64, u8, u32)>,
    alive: Vec<bool>,
    heap: BinaryHeap<u32>,
}

/// Non-overlapping spans: injected layers above the host, inner nodes above
/// outer ones, and for the same node the first pattern. A sweep over the
/// capture boundaries: the cost depends on the number of captures, not on
/// their lengths.
pub(crate) fn flatten(captures: &mut [Capture], scratch: &mut Flatten, out: &mut Vec<Span>) {
    if captures.is_empty() {
        return;
    }
    // Rank: later captures win.
    captures.sort_unstable_by(|a, b| {
        a.layer
            .cmp(&b.layer)
            .then(b.len.cmp(&a.len))
            .then(b.pattern.cmp(&a.pattern))
    });
    let Flatten {
        events,
        alive,
        heap,
    } = scratch;
    events.clear();
    for (rank, c) in captures.iter().enumerate() {
        events.push((c.start, 1, rank as u32));
        events.push((c.end, 0, rank as u32));
    }
    events.sort_unstable();
    alive.clear();
    alive.resize(captures.len(), false);
    heap.clear();
    let mut i = 0;
    while i < events.len() {
        let at = events[i].0;
        while i < events.len() && events[i].0 == at {
            let (_, start, rank) = events[i];
            alive[rank as usize] = start == 1;
            if start == 1 {
                heap.push(rank);
            }
            i += 1;
        }
        while heap.peek().is_some_and(|&top| !alive[top as usize]) {
            heap.pop();
        }
        let (Some(&top), Some(&(next, ..))) = (heap.peek(), events.get(i)) else {
            continue;
        };
        let Some(style) = captures[top as usize].style else {
            continue;
        };
        match out.last_mut() {
            Some(last) if last.end == at && last.style == style => last.end = next,
            _ => out.push(Span {
                start: at,
                end: next,
                style,
            }),
        }
    }
}
