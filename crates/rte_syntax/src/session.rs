//! Parsing on the syntax worker ([`Session`]) and the trees the UI
//! highlights ([`View`]).

use std::ops::{ControlFlow, Range};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Instant;

use rte_text::Snapshot;
use tree_sitter::{InputEdit, Language, Node, ParseOptions, ParseState, Parser, QueryCursor, Tree};

use crate::edits::point;
use crate::highlight::{Capture, Flatten, flatten, warm};
use crate::{Grammar, Highlighter, Lang, MARKDOWN_INLINE, Span, by_fence, by_id, highlighter};

/// Bytes of the window parsed first in a large file.
pub const WINDOW: u64 = 32 * 1024;
/// Larger files parse a window around the viewport before the whole file
/// (a full parse would take more than about one frame).
pub const WINDOW_THRESHOLD: u64 = 128 * 1024;

/// Result of one [`Session::work`] step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Work {
    /// Nothing left to do.
    Idle,
    /// Stopped because `cancel` returned true; call again later.
    Cancelled,
    /// The view changed.
    Updated,
    /// Only the whole-file parse runs (on its own thread); call again after
    /// the notification set with [`Session::set_notify`].
    Waiting,
}

/// Syntax state of one document on the worker thread.
pub struct Session {
    lang: &'static Lang,
    language: Language,
    injections: bool,
    parser: Parser,
    layer_parser: Parser,
    snapshot: Snapshot,
    interest: Range<u64>,
    host: Option<Host>,
    /// The host tree was parsed from `snapshot`.
    host_fresh: bool,
    layers: Vec<Layer>,
    /// The layers match the host tree and the interest range.
    layers_fresh: bool,
    /// Whole-file parse of a large file.
    full: Option<FullParse>,
    /// Called when the whole-file parse finishes.
    notify: Option<Arc<dyn Fn() + Send + Sync>>,
    /// The whole file parses on this thread (the parse thread failed).
    no_window: bool,
}

struct Host {
    tree: Tree,
    /// Parsed range of a window parse.
    window: Option<Range<u64>>,
}

/// A whole-file parse on its own thread, stopped when dropped.
struct FullParse {
    snapshot: Snapshot,
    /// Edits since `snapshot`, applied to the finished tree.
    edits: Vec<InputEdit>,
    tree: Receiver<Tree>,
    stop: Arc<AtomicBool>,
}

impl Drop for FullParse {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// An injected region (code fence, `<script>`, Markdown inline) and its tree.
#[derive(Clone)]
struct Layer {
    lang: &'static Lang,
    hl: Arc<Highlighter>,
    ranges: Vec<tree_sitter::Range>,
    tree: Tree,
    start: u64,
    end: u64,
    /// An edit touched the region since it was parsed.
    dirty: bool,
}

impl Session {
    pub fn new(
        lang: &'static Lang,
        snapshot: Snapshot,
        injections: bool,
    ) -> Result<Session, String> {
        // The highlight query compiles while the text parses.
        warm(lang.grammar);
        let language = lang.grammar.language();
        let mut parser = Parser::new();
        parser
            .set_language(&language)
            .map_err(|e| format!("{}: {e}", lang.id))?;
        Ok(Session {
            lang,
            language,
            injections,
            parser,
            layer_parser: Parser::new(),
            snapshot,
            interest: 0..0,
            host: None,
            host_fresh: false,
            layers: Vec::new(),
            layers_fresh: false,
            full: None,
            notify: None,
            no_window: false,
        })
    }

    /// `notify` runs (on another thread) when a whole-file parse finishes.
    pub fn set_notify(&mut self, notify: Arc<dyn Fn() + Send + Sync>) {
        self.notify = Some(notify);
    }

    pub fn lang(&self) -> &'static Lang {
        self.lang
    }

    /// The latest text.
    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    /// The trees match the latest text and interest range (a large file may
    /// still be parsing as a whole).
    pub fn is_current(&self) -> bool {
        self.host_fresh && self.layers_fresh
    }

    /// Current, and the whole file is parsed.
    pub fn is_complete(&self) -> bool {
        self.is_current() && self.full.is_none()
    }

    /// The host tree is a current parse of the whole file (not a window).
    pub fn host_is_whole(&self) -> bool {
        self.host_fresh
            && self.full.is_none()
            && self.host.as_ref().is_some_and(|h| h.window.is_none())
    }

    /// Bytes whose highlighting is final for the latest text: the whole file,
    /// or a window starting at the file start (a window starting later lacks
    /// the context before it: provisional), and only the interest range when
    /// injections apply.
    pub fn current_range(&self) -> Range<u64> {
        if !self.is_current() {
            return 0..0;
        }
        let mut range = match &self.host {
            Some(Host {
                window: Some(window),
                ..
            }) if window.start == 0 => window.clone(),
            Some(Host { window: None, .. }) => 0..u64::MAX,
            _ => return 0..0,
        };
        if self.has_injections() {
            range = range.start.max(self.interest.start)..range.end.min(self.interest.end);
        }
        range
    }

    pub fn interest(&self) -> &Range<u64> {
        &self.interest
    }

    fn has_injections(&self) -> bool {
        self.injections && matches!(self.lang.grammar, Grammar::Markdown | Grammar::Html)
    }

    /// Applies the edits that produced `snapshot`.
    pub fn edit(&mut self, edits: &[InputEdit], snapshot: Snapshot) {
        for edit in edits {
            if let Some(host) = &mut self.host {
                host.tree.edit(edit);
                if let Some(window) = &mut host.window {
                    map_range(window, edit);
                }
            }
            for layer in &mut self.layers {
                layer.tree.edit(edit);
                let mut range = layer.start..layer.end;
                layer.dirty |= map_range(&mut range, edit);
                (layer.start, layer.end) = (range.start, range.end);
            }
            if let Some(full) = &mut self.full {
                full.edits.push(*edit);
            }
        }
        self.snapshot = snapshot;
        self.host_fresh = false;
        self.layers_fresh = false;
    }

    /// Byte range the user sees (layers and windows are built around it).
    pub fn set_interest(&mut self, interest: Range<u64>) {
        if interest == self.interest {
            return;
        }
        self.interest = interest;
        if self.has_injections() {
            self.layers_fresh = false;
        }
        if let Some(Host {
            window: Some(window),
            ..
        }) = &self.host
            && !covers(window, &self.interest, self.snapshot.len())
        {
            self.host_fresh = false;
        }
    }

    /// Does the most urgent pending work: a finished whole-file tree (it
    /// replaces any window), the host tree, then the injected layers.
    pub fn work(&mut self, cancel: &dyn Fn() -> bool) -> Work {
        if let Some(work) = self.take_full_tree() {
            return work;
        }
        if !self.host_fresh {
            return self.parse_host(cancel);
        }
        if !self.layers_fresh {
            return self.parse_layers(cancel);
        }
        if self.full.is_some() {
            Work::Waiting
        } else {
            Work::Idle
        }
    }

    /// The trees for the UI (waits for the highlight query to compile).
    pub fn view(&self) -> View {
        let mut layers = Vec::with_capacity(1 + self.layers.len());
        if let (Some(host), Ok(hl)) = (&self.host, highlighter(self.lang.grammar)) {
            layers.push(ViewLayer {
                lang: self.lang,
                hl,
                tree: host.tree.clone(),
                start: 0,
                end: u64::MAX,
            });
            for layer in &self.layers {
                layers.push(ViewLayer {
                    lang: layer.lang,
                    hl: layer.hl.clone(),
                    tree: layer.tree.clone(),
                    start: layer.start,
                    end: layer.end,
                });
            }
        }
        View {
            lang: self.lang,
            layers,
            covered: if !self.has_injections() {
                0..u64::MAX
            } else if self.is_current() {
                self.interest.clone()
            } else {
                0..0
            },
        }
    }

    fn parse_host(&mut self, cancel: &dyn Fn() -> bool) -> Work {
        let len = self.snapshot.len();
        let has_full_tree = matches!(self.host, Some(Host { window: None, .. }));
        let mut window = None;
        let mut old = None;
        if len > WINDOW_THRESHOLD && !has_full_tree && !self.no_window {
            let (tree, w) = match &self.host {
                Some(Host {
                    tree,
                    window: Some(w),
                }) if covers(w, &self.interest, len) => (Some(tree), w.clone()),
                _ => (None, window_around(&self.snapshot, &self.interest)),
            };
            // A window covering half the file or more: parse the file once.
            if (w.end - w.start) * 2 < len {
                (old, window) = (tree, Some(w));
            }
        } else {
            old = self
                .host
                .as_ref()
                .filter(|h| h.window.is_none())
                .map(|h| &h.tree);
        }
        if window.is_none() {
            self.full = None;
        } else if self.full.is_none() {
            self.full = Some(self.start_full_parse());
        }
        let ranges = match &window {
            Some(w) => vec![ts_range(&self.snapshot, w.clone())],
            None => Vec::new(),
        };
        if self.parser.set_included_ranges(&ranges).is_err() {
            return Work::Idle;
        }
        match parse(&mut self.parser, &self.snapshot, old, cancel) {
            None => {
                self.parser.reset();
                Work::Cancelled
            }
            Some(tree) => {
                self.host = Some(Host { tree, window });
                self.host_fresh = true;
                self.layers_fresh = !self.has_injections();
                Work::Updated
            }
        }
    }

    /// Parses the injected regions near the interest range. Returns
    /// `Updated` once they are fresh (also when nothing changed: the caller
    /// reports the state before a long full parse starts).
    fn parse_layers(&mut self, cancel: &dyn Fn() -> bool) -> Work {
        let Some(host) = &self.host else {
            self.layers_fresh = true;
            return Work::Idle;
        };
        if !self.has_injections() {
            self.layers_fresh = true;
            return Work::Updated;
        }
        let Some(regions) = regions(
            self.lang.grammar,
            host.tree.root_node(),
            &self.snapshot,
            &self.interest,
            cancel,
        ) else {
            return Work::Cancelled;
        };
        let mut old = std::mem::take(&mut self.layers);
        let mut layers = Vec::with_capacity(regions.len());
        for (lang, ranges) in regions {
            let start = ranges[0].start_byte as u64;
            let end = ranges[ranges.len() - 1].end_byte as u64;
            let previous = old
                .iter()
                .position(|l| std::ptr::eq(l.lang, lang) && l.start == start)
                .map(|i| old.swap_remove(i));
            if let Some(layer) = &previous
                && !layer.dirty
                && layer.ranges == ranges
            {
                layers.push(previous.unwrap());
                continue;
            }
            let Ok(hl) = highlighter(lang.grammar) else {
                continue;
            };
            if self.layer_parser.set_language(&hl.language).is_err()
                || self.layer_parser.set_included_ranges(&ranges).is_err()
            {
                continue;
            }
            let old_tree = previous.as_ref().map(|l| &l.tree);
            let Some(tree) = parse(&mut self.layer_parser, &self.snapshot, old_tree, cancel) else {
                self.layer_parser.reset();
                // Keep what is done; the rest is parsed on the next call.
                layers.extend(previous);
                layers.append(&mut old);
                self.layers = layers;
                return Work::Cancelled;
            };
            layers.push(Layer {
                lang,
                hl,
                ranges,
                tree,
                start,
                end,
                dirty: false,
            });
        }
        self.layers = layers;
        self.layers_fresh = true;
        Work::Updated
    }

    /// Adopts the finished whole-file tree, if the parse thread posted it;
    /// `None` while there is none.
    fn take_full_tree(&mut self) -> Option<Work> {
        let mut tree = match self.full.as_ref()?.tree.try_recv() {
            Ok(tree) => tree,
            Err(TryRecvError::Empty) => return None,
            // The parse thread failed; parse the file on this thread instead.
            Err(TryRecvError::Disconnected) => {
                self.full = None;
                self.no_window = true;
                self.host_fresh = false;
                return Some(Work::Cancelled);
            }
        };
        let full = self.full.take().expect("checked above");
        for edit in &full.edits {
            tree.edit(edit);
        }
        self.host = Some(Host { tree, window: None });
        self.host_fresh = full.snapshot.same_tree(&self.snapshot);
        self.layers_fresh = !self.has_injections();
        // Clears the window ranges; empty ranges are always valid.
        let _ = self.parser.set_included_ranges(&[]);
        Some(Work::Updated)
    }

    /// Parses the current text as a whole on its own thread.
    fn start_full_parse(&self) -> FullParse {
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let (language, snapshot, flag) =
            (self.language.clone(), self.snapshot.clone(), stop.clone());
        let notify = self.notify.clone();
        // A failed spawn drops `tx`: the parse falls back to this thread.
        let _ = thread::Builder::new()
            .name("syntax-full".to_owned())
            .spawn(move || {
                let mut parser = Parser::new();
                if parser.set_language(&language).is_err() {
                    return;
                }
                let stopped = || flag.load(Ordering::Relaxed);
                if let Some(tree) = parse(&mut parser, &snapshot, None, &stopped)
                    && tx.send(tree).is_ok()
                    && let Some(notify) = notify
                {
                    notify();
                }
            });
        FullParse {
            snapshot: self.snapshot.clone(),
            edits: Vec::new(),
            tree: rx,
            stop,
        }
    }
}

/// Parses `snapshot` through its chunks; `None` when cancelled.
fn parse(
    parser: &mut Parser,
    snapshot: &Snapshot,
    old: Option<&Tree>,
    cancel: &dyn Fn() -> bool,
) -> Option<Tree> {
    let mut read = |offset: usize, _| match snapshot.chunk_at(offset as u64) {
        Some((start, bytes)) => &bytes[offset - start as usize..],
        None => &[][..],
    };
    let mut progress = |_: &ParseState| {
        if cancel() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    };
    let options = ParseOptions::new().progress_callback(&mut progress);
    parser.parse_with_options(&mut read, old, Some(options))
}

/// Maps `range` through `edit`; true when the edit touches it.
fn map_range(range: &mut Range<u64>, edit: &InputEdit) -> bool {
    let start = edit.start_byte as u64;
    let old_end = edit.old_end_byte as u64;
    let new_end = edit.new_end_byte as u64;
    let shift = |pos: u64| pos + new_end - old_end;
    if old_end < range.start {
        *range = shift(range.start)..shift(range.end);
        false
    } else if start > range.end {
        false
    } else {
        range.start = range.start.min(start);
        range.end = if range.end >= old_end {
            shift(range.end)
        } else {
            new_end
        };
        true
    }
}

fn covers(window: &Range<u64>, interest: &Range<u64>, len: u64) -> bool {
    let end = interest.end.min(len);
    window.start <= interest.start.min(end) && end <= window.end
}

/// About [`WINDOW`] bytes (at most four windows for a large interest range)
/// starting a quarter window before `interest`: whole lines, unless a line
/// boundary is more than a quarter window away (minified files).
fn window_around(snapshot: &Snapshot, interest: &Range<u64>) -> Range<u64> {
    let len = snapshot.len();
    let from = interest.start.min(len).saturating_sub(WINDOW / 4);
    let line_start = snapshot.line_start(snapshot.line_of(from)).unwrap_or(0);
    let start = if from - line_start <= WINDOW / 4 {
        line_start
    } else {
        char_start(snapshot, from)
    };
    let to = (start + WINDOW)
        .max(interest.end.min(start + 4 * WINDOW))
        .min(len);
    let next_line = snapshot.line_start(snapshot.line_of(to) + 1).unwrap_or(len);
    let end = if next_line - to <= WINDOW / 4 {
        next_line
    } else {
        char_start(snapshot, to)
    };
    start..end
}

/// `pos`, moved back to the start of a UTF-8 character.
fn char_start(snapshot: &Snapshot, mut pos: u64) -> u64 {
    for _ in 0..3 {
        let continuation = snapshot
            .chunk_at(pos)
            .is_some_and(|(start, bytes)| bytes[(pos - start) as usize] & 0xC0 == 0x80);
        if !continuation || pos == 0 {
            break;
        }
        pos -= 1;
    }
    pos
}

fn ts_range(snapshot: &Snapshot, range: Range<u64>) -> tree_sitter::Range {
    tree_sitter::Range {
        start_byte: range.start as usize,
        end_byte: range.end as usize,
        start_point: point(snapshot, range.start),
        end_point: point(snapshot, range.end),
    }
}

/// Ranges of `node` without its named children (block continuation markers
/// of Markdown; anonymous children are part of the text).
fn ranges_between_children(node: Node<'_>) -> Vec<tree_sitter::Range> {
    let mut ranges = Vec::new();
    let mut start = (node.start_byte(), node.start_position());
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.start_byte() > start.0 {
            ranges.push(tree_sitter::Range {
                start_byte: start.0,
                end_byte: child.start_byte(),
                start_point: start.1,
                end_point: child.start_position(),
            });
        }
        start = (child.end_byte(), child.end_position());
    }
    if node.end_byte() > start.0 {
        ranges.push(tree_sitter::Range {
            start_byte: start.0,
            end_byte: node.end_byte(),
            start_point: start.1,
            end_point: node.end_position(),
        });
    }
    ranges
}

/// Lines of `node` without its first and last (front matter delimiters).
fn inner_lines(snapshot: &Snapshot, node: Node<'_>) -> Option<tree_sitter::Range> {
    let first = node.start_position().row as u64 + 1;
    let last = node.end_position().row as u64;
    // The node ends after the closing line's break or at its end.
    let last = if node.end_position().column == 0 {
        last.checked_sub(1)?
    } else {
        last
    };
    let start = snapshot.line_start(first)?;
    let end = snapshot.line_start(last)?;
    (start < end).then(|| ts_range(snapshot, start..end))
}

fn node_text(snapshot: &Snapshot, node: Node<'_>) -> String {
    let bytes = snapshot.to_vec(node.start_byte() as u64..node.end_byte() as u64);
    String::from_utf8_lossy(&bytes).into_owned()
}

type Region = (&'static Lang, Vec<tree_sitter::Range>);

/// Injected regions of a Markdown or HTML host intersecting `interest`;
/// `None` when cancelled. Children before the interest range are skipped by
/// byte seek and the walk stops after it.
fn regions(
    host: Grammar,
    root: Node<'_>,
    snapshot: &Snapshot,
    interest: &Range<u64>,
    cancel: &dyn Fn() -> bool,
) -> Option<Vec<Region>> {
    let mut out = Vec::new();
    if !matches!(host, Grammar::Markdown | Grammar::Html) {
        return Some(out);
    }
    let mut cursor = root.walk();
    let mut visited = 0u32;
    'walk: loop {
        let node = cursor.node();
        visited += 1;
        if visited.is_multiple_of(256) && cancel() {
            return None;
        }
        let (region, leaf) = match (host, node.kind()) {
            (Grammar::Markdown, "inline") => (
                Some((&MARKDOWN_INLINE, ranges_between_children(node))),
                true,
            ),
            (Grammar::Markdown, "fenced_code_block") => (fence(snapshot, node), true),
            (Grammar::Markdown, "minus_metadata") => (
                inner_lines(snapshot, node).map(|r| (by_id("yaml").unwrap(), vec![r])),
                true,
            ),
            (Grammar::Markdown, "plus_metadata") => (
                inner_lines(snapshot, node).map(|r| (by_id("toml").unwrap(), vec![r])),
                true,
            ),
            (Grammar::Markdown, "html_block") => (
                Some((by_id("html").unwrap(), ranges_between_children(node))),
                true,
            ),
            (Grammar::Html, "script_element") => (script(snapshot, node), true),
            (Grammar::Html, "style_element") => (
                raw_text(node).map(|r| (by_id("css").unwrap(), vec![r])),
                true,
            ),
            _ => (None, false),
        };
        if let Some((lang, ranges)) = region
            && !ranges.is_empty()
        {
            out.push((lang, ranges));
        }
        if !leaf
            && cursor
                .goto_first_child_for_byte(interest.start as usize)
                .is_some()
        {
            if cursor.node().start_byte() as u64 <= interest.end {
                continue;
            }
            cursor.goto_parent();
        }
        // The next sibling inside the interest range, climbing as needed.
        loop {
            if cursor.goto_next_sibling() && cursor.node().start_byte() as u64 <= interest.end {
                continue 'walk;
            }
            if !cursor.goto_parent() {
                return Some(out);
            }
        }
    }
}

fn fence(snapshot: &Snapshot, node: Node<'_>) -> Option<Region> {
    let mut cursor = node.walk();
    let mut lang = None;
    let mut content = None;
    for child in node.children(&mut cursor) {
        match child.kind() {
            "info_string" => {
                let mut c = child.walk();
                let word = child
                    .children(&mut c)
                    .find(|n| n.kind() == "language")
                    .map(|n| node_text(snapshot, n))?;
                lang = by_fence(&word);
            }
            "code_fence_content" => content = Some(child),
            _ => {}
        }
    }
    let lang = lang?;
    // One level of injection: no Markdown inside Markdown.
    if lang.grammar == Grammar::Markdown {
        return None;
    }
    Some((lang, ranges_between_children(content?)))
}

fn raw_text(node: Node<'_>) -> Option<tree_sitter::Range> {
    let mut cursor = node.walk();
    let raw = node
        .children(&mut cursor)
        .find(|n| n.kind() == "raw_text")?;
    Some(raw.range())
}

/// `<script>` content: JavaScript unless its `type` names something else.
fn script(snapshot: &Snapshot, node: Node<'_>) -> Option<Region> {
    let mut cursor = node.walk();
    let start_tag = node
        .children(&mut cursor)
        .find(|n| n.kind() == "start_tag")?;
    let mut lang = by_id("javascript");
    let mut c = start_tag.walk();
    for attribute in start_tag.children(&mut c) {
        if attribute.kind() != "attribute" {
            continue;
        }
        let mut a = attribute.walk();
        let parts: Vec<Node<'_>> = attribute.named_children(&mut a).collect();
        let is_type = parts
            .first()
            .is_some_and(|n| node_text(snapshot, *n).eq_ignore_ascii_case("type"));
        if is_type {
            let value = parts
                .get(1)
                .map(|n| {
                    node_text(snapshot, *n)
                        .trim_matches(['"', '\''])
                        .to_ascii_lowercase()
                })
                .unwrap_or_default();
            lang = match value.as_str() {
                "" | "module" | "text/javascript" | "application/javascript" => by_id("javascript"),
                v if v.contains("json") => by_id("json"),
                v if v.contains("typescript") => by_id("typescript"),
                _ => None,
            };
        }
    }
    Some((lang?, vec![raw_text(node)?]))
}

/// Syntax trees for highlighting on the UI thread. Cloning copies tree
/// handles, not trees.
#[derive(Clone)]
pub struct View {
    lang: &'static Lang,
    /// The host first, then injected layers.
    pub(crate) layers: Vec<ViewLayer>,
    /// Bytes where `layers` hold every injected region of the parsed text;
    /// empty after an edit.
    covered: Range<u64>,
}

#[derive(Clone)]
pub(crate) struct ViewLayer {
    pub(crate) lang: &'static Lang,
    pub(crate) hl: Arc<Highlighter>,
    pub(crate) tree: Tree,
    pub(crate) start: u64,
    pub(crate) end: u64,
}

impl View {
    pub fn lang(&self) -> &'static Lang {
        self.lang
    }

    /// Layers whose region contains `pos`, innermost first, then the host.
    /// Injected regions do not overlap, so at most one precedes the host.
    pub(crate) fn layers_at(&self, pos: u64) -> impl Iterator<Item = &ViewLayer> {
        let inner = self
            .layers
            .iter()
            .skip(1)
            .filter(|l| l.start <= pos && pos < l.end)
            .min_by_key(|l| l.end - l.start);
        inner.into_iter().chain(self.layers.first())
    }

    /// Language at `pos` of `snapshot` (the text these trees were edited
    /// to): an injected language (Markdown inline elements count as
    /// Markdown), otherwise the host's. Outside the parsed layers, or after
    /// an edit, the host tree's fences and `<script>`/`<style>` elements
    /// decide.
    pub fn lang_at(&self, pos: u64, snapshot: &Snapshot) -> &'static Lang {
        if !matches!(self.lang.grammar, Grammar::Markdown | Grammar::Html) {
            return self.lang;
        }
        let injected = if self.covered.start <= pos && pos < self.covered.end {
            self.layers_at(pos).next().map(|l| l.lang)
        } else {
            self.tree().and_then(|tree| {
                let found = regions(
                    self.lang.grammar,
                    tree.root_node(),
                    snapshot,
                    &(pos..pos + 1),
                    &|| false,
                )?;
                found
                    .into_iter()
                    .find(|(_, ranges)| {
                        ranges
                            .iter()
                            .any(|r| r.start_byte as u64 <= pos && pos < r.end_byte as u64)
                    })
                    .map(|(lang, _)| lang)
            })
        };
        injected
            .filter(|l| !std::ptr::eq(*l, &MARKDOWN_INLINE))
            .unwrap_or(self.lang)
    }

    /// Host syntax tree, if parsed.
    pub fn tree(&self) -> Option<&Tree> {
        self.layers.first().map(|l| &l.tree)
    }

    /// Host highlighter (its language and tags query).
    pub fn highlighter(&self) -> Option<&Arc<Highlighter>> {
        self.layers.first().map(|l| &l.hl)
    }

    /// Keeps tree positions valid for the text after `edits` (from
    /// [`crate::input_edits`]) until a new parse arrives.
    pub fn edit(&mut self, edits: &[InputEdit]) {
        if !edits.is_empty() {
            self.covered = 0..0;
        }
        for layer in &mut self.layers {
            for edit in edits {
                layer.tree.edit(edit);
                if layer.end != u64::MAX {
                    let mut range = layer.start..layer.end;
                    map_range(&mut range, edit);
                    (layer.start, layer.end) = (range.start, range.end);
                }
            }
        }
    }
}

/// Reusable buffers for highlighting.
#[derive(Default)]
pub struct Painter {
    cursor: Option<QueryCursor>,
    captures: Vec<Capture>,
    scratch: Flatten,
}

impl Painter {
    /// Appends the spans of `range` to `out`, sorted and non-overlapping.
    /// Returns false when `deadline` passed (the spans are incomplete).
    pub fn paint(
        &mut self,
        view: &View,
        snapshot: &Snapshot,
        range: Range<u64>,
        deadline: Option<Instant>,
        out: &mut Vec<Span>,
    ) -> bool {
        let stop = || deadline.is_some_and(|d| Instant::now() >= d);
        self.paint_ranges(view, snapshot, &[range], &stop, out)
    }

    /// Appends the spans of the sorted, disjoint `ranges` to `out`. Returns
    /// false when `stop` returned true first (the spans are incomplete).
    pub fn paint_ranges(
        &mut self,
        view: &View,
        snapshot: &Snapshot,
        ranges: &[Range<u64>],
        stop: &dyn Fn() -> bool,
        out: &mut Vec<Span>,
    ) -> bool {
        if stop() {
            return false;
        }
        let cursor = self.cursor.get_or_insert_with(QueryCursor::new);
        self.captures.clear();
        let mut complete = true;
        'ranges: for range in ranges {
            for (index, layer) in view.layers.iter().enumerate() {
                let start = range.start.max(layer.start);
                let end = range.end.min(layer.end);
                if start >= end {
                    continue;
                }
                let depth = u8::from(index > 0);
                if !layer.hl.captures(
                    cursor,
                    layer.tree.root_node(),
                    snapshot,
                    start..end,
                    depth,
                    stop,
                    &mut self.captures,
                ) {
                    complete = false;
                    break 'ranges;
                }
            }
        }
        flatten(&mut self.captures, &mut self.scratch, out);
        complete
    }
}
