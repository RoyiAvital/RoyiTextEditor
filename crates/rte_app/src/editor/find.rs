//! The search box: options, requests to the search thread and their
//! replies, keys, mouse and drawing. Literal searches with Match Case are
//! cheap enough to scan a bounded range on the UI thread; everything else
//! runs on the search thread, which is started on first use.

use std::ops::Range;

use rte_search::{Matcher, Mode as SearchMode, Query, Replacement, Searcher};
use rte_text::{Edit, Transaction};
use rte_view::{FindField, FindFrame, FindGeometry, FindHit, Severity};

use super::*;
use crate::palette::Query as TextField;
use crate::search::{Job, Lane, MAX_VIEW_MATCHES, Outcome, Reply, SearchThread, Work};

/// Bytes a literal search scans on the UI before the search thread takes over.
const UI_SCAN: u64 = 1024 * 1024;
/// Bytes of visible text searched for highlights.
const VIEWPORT_MAX: u64 = 1024 * 1024;
/// Longest selection used to seed the search box.
const SEED_MAX: u64 = 1024;
/// Highlight ranges kept allocated between searches.
const KEEP_MATCHES: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Field {
    Find,
    Replace,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Count {
    Unknown,
    Partial(u64),
    Done(u64),
}

/// State of the search box; created on first use and kept after closing.
pub(super) struct FindBox {
    pub(super) open: bool,
    pub(super) focus: Option<Field>,
    pub(super) replace_open: bool,
    pub(super) find: TextField,
    pub(super) replace: TextField,
    pub(super) mode: SearchMode,
    pub(super) match_case: bool,
    pub(super) whole_word: bool,
    pub(super) preserve: bool,
    pub(super) scope: Option<Range<u64>>,
    /// Bumped by every change of query, options, scope or text.
    pub(super) generation: u64,
    /// Text of the current generation.
    text: Snapshot,
    matches: Vec<Range<u64>>,
    /// Generation and visible rows the matches belong to.
    matches_for: Option<(u64, Vec<Range<u64>>)>,
    viewport_sent: Option<(u64, Vec<Range<u64>>)>,
    count: Count,
    count_sent: Option<u64>,
    /// Index of the match at a range.
    index: Option<(Range<u64>, u64)>,
    index_sent: Option<(u64, u64)>,
    pub(super) error: Option<String>,
    warning: Option<String>,
    /// UI matcher of the current generation (literal queries only).
    literal: Option<(u64, Option<Matcher>)>,
    /// A navigation in flight: generation, selections it applies to.
    nav: Option<(u64, Selections)>,
    /// The match navigation selected last (an empty one is stepped over).
    current: Option<Range<u64>>,
}

impl FindBox {
    fn new() -> FindBox {
        FindBox {
            open: false,
            focus: None,
            replace_open: false,
            find: TextField::default(),
            replace: TextField::default(),
            mode: SearchMode::Text,
            match_case: false,
            whole_word: false,
            preserve: false,
            scope: None,
            generation: 1,
            text: Snapshot::empty(),
            matches: Vec::new(),
            matches_for: None,
            viewport_sent: None,
            count: Count::Unknown,
            count_sent: None,
            index: None,
            index_sent: None,
            error: None,
            warning: None,
            literal: None,
            nav: None,
            current: None,
        }
    }

    pub(super) fn query(&self) -> Query {
        Query {
            text: self.find.as_str().to_owned(),
            mode: self.mode,
            match_case: self.match_case,
            whole_word: self.whole_word,
        }
    }

    fn replacement(&self) -> Replacement {
        if self.mode == SearchMode::Regex {
            Replacement::parse(self.replace.as_str())
        } else {
            Replacement::literal(self.replace.as_str())
        }
    }

    /// What authorizes an edit or selection computed for the box.
    fn auth(&self) -> FindAuth {
        FindAuth {
            query: self.query(),
            replace: self.replace.as_str().to_owned(),
            preserve: self.preserve,
            scope: self.scope.clone(),
        }
    }

    #[cfg(test)]
    pub(super) fn match_capacity(&self) -> usize {
        self.matches.capacity()
    }

    /// Drops the highlights; a large buffer is released.
    fn clear_matches(&mut self) {
        if self.matches.capacity() > KEEP_MATCHES {
            self.matches = Vec::new();
        } else {
            self.matches.clear();
        }
        self.matches_for = None;
        self.viewport_sent = None;
    }

    /// Starts a new generation: results so far no longer apply.
    fn invalidate(&mut self, text: &Snapshot) {
        self.generation += 1;
        self.text = text.clone();
        self.clear_matches();
        self.count = Count::Unknown;
        self.count_sent = None;
        self.index = None;
        self.index_sent = None;
        self.error = None;
        self.warning = None;
        self.literal = None;
        self.nav = None;
        self.current = None;
    }

    fn scope(&self, len: u64) -> Range<u64> {
        self.scope
            .as_ref()
            .map_or(0..len, |r| r.start.min(len)..r.end.min(len))
    }

    pub(super) fn counter(&self, current: Range<u64>) -> (String, bool) {
        if self.find.as_str().is_empty() {
            return (String::new(), false);
        }
        if self.error.is_some() || self.count == Count::Done(0) {
            return ("No results".to_owned(), true);
        }
        let index = self
            .index
            .as_ref()
            .filter(|(r, _)| *r == current)
            .map_or_else(|| "?".to_owned(), |(_, i)| thousands(i + 1));
        let total = match self.count {
            Count::Unknown => "?".to_owned(),
            Count::Partial(n) => format!("{}+", thousands(n)),
            Count::Done(n) => thousands(n),
        };
        (format!("{index} of {total}"), false)
    }
}

fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// Matches to draw and the find-in-selection range of an open box.
pub(super) fn highlights<'a>(
    find: Option<&'a FindBox>,
    snapshot: &Snapshot,
) -> (&'a [Range<u64>], Option<Range<u64>>) {
    match find {
        Some(f) if f.open => {
            let valid = f.text.same_tree(snapshot)
                && f.matches_for
                    .as_ref()
                    .is_some_and(|(g, _)| *g == f.generation);
            let matches: &[Range<u64>] = if valid { &f.matches } else { &[] };
            (matches, f.scope.as_ref().map(|_| f.scope(snapshot.len())))
        }
        _ => (&[], None),
    }
}

impl Editor {
    pub(super) fn search_thread(&mut self) -> &SearchThread {
        let wake = self.wake.clone();
        self.search
            .get_or_insert_with(|| SearchThread::spawn(move || wake()))
    }

    fn find_box(&mut self) -> &mut FindBox {
        self.find.get_or_insert_with(|| Box::new(FindBox::new()))
    }

    pub(super) fn find_open(&self) -> bool {
        self.find.as_ref().is_some_and(|f| f.open)
    }

    pub(super) fn find_focused(&self) -> bool {
        self.find
            .as_ref()
            .is_some_and(|f| f.open && f.focus.is_some())
    }

    /// Text selected by the primary selection when it can seed a search:
    /// one line, at most [`SEED_MAX`] bytes, valid UTF-8. An empty
    /// selection gives the word at the caret.
    pub(super) fn seed_text(&self) -> Option<(String, bool)> {
        let s = &self.doc.snapshot;
        let primary = self.doc.selections.primary();
        let (range, word) = if primary.is_empty() {
            let w = edit::word_at(s, primary.head);
            (w, true)
        } else {
            (primary.range(), false)
        };
        if range.is_empty() || range.end - range.start > SEED_MAX {
            return None;
        }
        let text = String::from_utf8(s.to_vec(range)).ok()?;
        if text.contains('\n') || (word && !text.chars().any(|c| c.is_alphanumeric() || c == '_')) {
            return None;
        }
        Some((text, word))
    }

    /// `Ctrl+F` / `Ctrl+H`: opens and focuses the box, seeded from the selection.
    pub(super) fn open_find(&mut self, replace: bool) {
        let seed = self.seed_text();
        let f = self.find_box();
        f.open = true;
        f.replace_open |= replace;
        f.focus = Some(if replace { Field::Replace } else { Field::Find });
        if let Some((text, _)) = seed {
            let text = if f.mode == SearchMode::Regex {
                escape_regex(&text)
            } else {
                text
            };
            f.find.set(&text);
        } else {
            f.find.select_all();
        }
        if replace {
            f.replace.select_all();
        }
        self.query_changed(true);
    }

    pub(super) fn close_find(&mut self) {
        let Some(f) = &mut self.find else {
            return;
        };
        f.open = false;
        f.focus = None;
        f.clear_matches();
        let generation = f.generation + 1;
        f.generation = generation;
        if let Some(t) = &self.search {
            t.cancel(Lane::Find, generation);
        }
    }

    /// The query or options changed: new generation; `incremental` moves
    /// to the nearest match from the selection start.
    pub(super) fn query_changed(&mut self, incremental: bool) {
        let snapshot = self.doc.snapshot.clone();
        let Some(f) = &mut self.find else {
            return;
        };
        f.invalidate(&snapshot);
        if incremental && f.open && !f.find.as_str().is_empty() {
            let from = self.doc.selections.primary().start();
            self.find_from(from, false);
        }
    }

    /// Scope of the search box for the current text.
    fn find_scope(&self) -> Range<u64> {
        let len = self.doc.snapshot.len();
        self.find.as_ref().map_or(0..len, |f| f.scope(len))
    }

    /// UI matcher for literal queries of the current generation.
    fn literal_matcher(&mut self) -> Option<&Matcher> {
        let f = self.find.as_mut()?;
        let literal = f.mode == SearchMode::Text && f.match_case && !f.find.as_str().is_empty();
        if !literal {
            return None;
        }
        if f.literal.as_ref().is_none_or(|(g, _)| *g != f.generation) {
            let m = Matcher::new(&f.query()).ok().flatten();
            f.literal = Some((f.generation, m));
        }
        f.literal.as_ref().and_then(|(_, m)| m.as_ref())
    }

    /// `F3` / `Shift+F3` and incremental search: selects the next (previous)
    /// match from `from`, wrapping in the scope.
    pub(super) fn find_from(&mut self, from: u64, backwards: bool) {
        let scope = self.find_scope();
        let snapshot = self.doc.snapshot.clone();
        let never = || false;
        // Backwards, a bounded window is exact only when matches cannot overlap.
        if let Some(m) = self
            .literal_matcher()
            .filter(|m| !backwards || m.never_overlaps())
        {
            let mut s = Searcher::new(m, &snapshot, &never);
            // Each pass is bounded; `None` from a pass that covered the scope is final.
            let found = if backwards {
                let lo = from.saturating_sub(UI_SCAN).max(scope.start);
                match s.find_prev(from, lo) {
                    None if lo == scope.start => {
                        let wrap = scope.end.saturating_sub(UI_SCAN).max(scope.start);
                        s.find_prev(scope.end, wrap)
                            .map(Some)
                            .or((wrap == scope.start).then_some(None))
                    }
                    found => found.map(Some),
                }
            } else {
                let ahead = (from + UI_SCAN).min(scope.end);
                match s.find(from, ahead) {
                    None if ahead == scope.end => {
                        let wrap = (scope.start + UI_SCAN).min(scope.end);
                        s.find(scope.start, wrap)
                            .map(Some)
                            .or((wrap == scope.end).then_some(None))
                    }
                    found => found.map(Some),
                }
            };
            match found {
                Some(Some(found)) => {
                    self.select_match(found, None);
                    return;
                }
                Some(None) => {
                    self.notice = Some("No results".to_owned());
                    return;
                }
                None => {}
            }
        }
        let sels = self.doc.selections.clone();
        let Some(f) = &mut self.find else {
            return;
        };
        let (generation, query) = (f.generation, f.query());
        f.nav = Some((generation, sels));
        self.search_thread().send(Job {
            lane: Lane::Find,
            generation,
            snapshot,
            query,
            work: Work::Find {
                from,
                scope,
                backwards,
            },
        });
    }

    fn select_match(&mut self, found: Range<u64>, index: Option<u64>) {
        self.doc
            .select(Selections::single(Selection::new(found.start, found.end)));
        self.reveal();
        if let Some(f) = &mut self.find {
            f.current = Some(found.clone());
            if let Some(i) = index {
                f.index = Some((found, i));
            }
        }
    }

    /// `F3` / `Shift+F3`: seeds an empty query from the selection first.
    pub(super) fn find_next(&mut self, backwards: bool) {
        if self
            .find
            .as_ref()
            .is_none_or(|f| f.find.as_str().is_empty())
        {
            let Some((text, _)) = self.seed_text() else {
                self.notice = Some("Nothing to search for".to_owned());
                return;
            };
            let f = self.find_box();
            f.find.set(&if f.mode == SearchMode::Regex {
                escape_regex(&text)
            } else {
                text
            });
            self.query_changed(false);
        }
        let primary = self.doc.selections.primary();
        // From an empty current match, step over it by one character.
        let on_empty_match = primary.is_empty()
            && self
                .find
                .as_ref()
                .and_then(|f| f.current.as_ref())
                .is_some_and(|m| *m == primary.range());
        let s = &self.doc.snapshot;
        let scope = self.find_scope();
        let from = match (backwards, on_empty_match) {
            (false, false) => primary.end(),
            (true, false) => primary.start(),
            // Past the scope end: the search wraps to its start.
            (false, true) if primary.head >= scope.end => scope.end + 1,
            (false, true) => edit::next_boundary(s, primary.head),
            // From the scope start: the last match, wrapping.
            (true, true) if primary.head <= scope.start => scope.end,
            (true, true) => edit::prev_boundary(s, primary.head),
        };
        self.find_from(from, backwards);
    }

    pub(super) fn toggle_find_option(&mut self, action: Action) {
        let f = self.find_box();
        match action {
            Action::ToggleMatchCase => f.match_case = !f.match_case,
            Action::ToggleWholeWord => f.whole_word = !f.whole_word,
            Action::ToggleRegex => {
                f.mode = if f.mode == SearchMode::Regex {
                    SearchMode::Text
                } else {
                    SearchMode::Regex
                }
            }
            Action::ToggleGlob => {
                f.mode = if f.mode == SearchMode::Glob {
                    SearchMode::Text
                } else {
                    SearchMode::Glob
                }
            }
            Action::TogglePreserveCase => {
                f.preserve = !f.preserve;
                return;
            }
            _ => {}
        }
        self.query_changed(true);
    }

    pub(super) fn toggle_find_in_selection(&mut self) {
        let primary = self.doc.selections.primary();
        let f = self.find_box();
        if f.scope.is_some() {
            f.scope = None;
        } else if primary.is_empty() {
            self.notice = Some("Select the text to search in first".to_owned());
            return;
        } else {
            f.scope = Some(primary.range());
        }
        self.query_changed(false);
    }

    /// Requests counts and viewport matches the open box is missing; a new
    /// text starts a new generation.
    pub(super) fn sync_find(&mut self) {
        self.revoke_stale_op();
        let snapshot = self.doc.snapshot.clone();
        let primary = self.doc.selections.primary();
        let Some(f) = &mut self.find else {
            return;
        };
        if !f.open || f.find.as_str().is_empty() || f.error.is_some() {
            return;
        }
        if !f.text.same_tree(&snapshot) {
            f.invalidate(&snapshot);
        }
        let generation = f.generation;
        let scope = f.scope(snapshot.len());
        let query = f.query();
        let ui_literal = f.mode == SearchMode::Text && f.match_case;
        let viewport = self.viewport_rows(&scope);
        let Some(f) = &mut self.find else {
            return;
        };
        let mut jobs = Vec::new();
        if f.count_sent != Some(generation) {
            f.count_sent = Some(generation);
            jobs.push(Work::Count {
                scope: scope.clone(),
            });
        }
        if f.viewport_sent.as_ref() != Some(&(generation, viewport.clone())) {
            f.viewport_sent = Some((generation, viewport.clone()));
            if !ui_literal {
                jobs.push(Work::Viewport {
                    rows: viewport.clone(),
                });
            }
        }
        if let Count::Done(_) = f.count
            && (!primary.is_empty() || f.current.as_ref() == Some(&primary.range()))
            && f.index.as_ref().is_none_or(|(r, _)| *r != primary.range())
            && f.index_sent != Some((generation, primary.start()))
        {
            f.index_sent = Some((generation, primary.start()));
            jobs.push(Work::Index {
                at: primary.start(),
                scope,
            });
        }
        if ui_literal && f.matches_for.as_ref() != Some(&(generation, viewport.clone())) {
            let never = || false;
            if let Some(m) = self.literal_matcher() {
                let found =
                    Searcher::new(m, &snapshot, &never).matches_in(&viewport, MAX_VIEW_MATCHES);
                if let Some(f) = &mut self.find {
                    f.clear_matches();
                    f.matches = found;
                    f.viewport_sent = Some((generation, viewport.clone()));
                    f.matches_for = Some((generation, viewport));
                }
            }
        }
        for work in jobs {
            self.search_thread().send(Job {
                lane: Lane::Find,
                generation,
                snapshot: snapshot.clone(),
                query: query.clone(),
                work,
            });
        }
    }

    /// Text of the visible rows inside `scope` for highlights: per row at
    /// most what the view lays out, at most [`VIEWPORT_MAX`] bytes in all.
    fn viewport_rows(&self, scope: &Range<u64>) -> Vec<Range<u64>> {
        let s = &self.doc.snapshot;
        let top = self.scroll.top_line;
        let mut budget = VIEWPORT_MAX;
        let mut rows: Vec<Range<u64>> = Vec::new();
        for line in top..top + u64::from(self.layout().rows) {
            let Some(start) = s.line_start(line) else {
                break;
            };
            let end = s.line_start(line + 1).unwrap_or(s.len());
            let end = end
                .min(start + rte_view::ROW_SCAN_LIMIT as u64)
                .min(start + budget);
            budget -= end - start;
            let (start, end) = (start.max(scope.start), end.min(scope.end));
            if start <= end {
                match rows.last_mut() {
                    Some(last) if last.end == start => last.end = end,
                    _ => rows.push(start..end),
                }
            }
            if budget == 0 {
                break;
            }
        }
        rows
    }

    /// Search work started by a key still runs: an operation, a navigation,
    /// or the count and highlights of the open box (benchmarks wait for it).
    pub(super) fn search_busy(&self) -> bool {
        if self.pending_op.is_some() {
            return true;
        }
        let Some(f) = &self.find else {
            return false;
        };
        if f.nav.as_ref().is_some_and(|(g, _)| *g == f.generation) {
            return true;
        }
        if !f.open || f.find.as_str().is_empty() || f.error.is_some() {
            return false;
        }
        let shown = f
            .matches_for
            .as_ref()
            .is_some_and(|(g, _)| *g == f.generation);
        !matches!(f.count, Count::Done(_)) || !shown
    }

    /// Applies replies of the search thread.
    pub(super) fn drain_search(&mut self) {
        while let Some(reply) = self.search.as_ref().and_then(SearchThread::try_recv) {
            match reply.lane {
                Lane::Find => self.find_reply(reply),
                Lane::Op => self.op_reply(reply),
            }
        }
    }

    pub(super) fn find_reply(&mut self, reply: Reply) {
        let current = self.doc.selections.clone();
        let Some(f) = &mut self.find else {
            return;
        };
        if reply.generation != f.generation || !reply.snapshot.same_tree(&self.doc.snapshot) {
            return;
        }
        match reply.outcome {
            Outcome::Invalid(error) => {
                f.error = Some(error);
                f.matches.clear();
                f.nav = None;
            }
            Outcome::Count {
                count,
                done,
                skipped,
                complex,
            } => {
                f.count = if done {
                    Count::Done(count)
                } else {
                    Count::Partial(count)
                };
                f.warning = match (complex, skipped) {
                    (0, 0) => None,
                    (0, n) => Some(format!("{n} lines skipped: invalid UTF-8 or too long")),
                    (n, _) => Some(format!("{n} lines skipped: too complex")),
                };
            }
            Outcome::Viewport { rows, matches } => {
                if f.viewport_sent
                    .as_ref()
                    .is_some_and(|(_, sent)| *sent == rows)
                {
                    f.clear_matches();
                    f.matches = matches;
                    f.viewport_sent = Some((f.generation, rows.clone()));
                    f.matches_for = Some((f.generation, rows));
                }
            }
            Outcome::Found { found, index } => {
                let Some((generation, sels)) = f.nav.take() else {
                    return;
                };
                if generation != f.generation || sels != current {
                    return;
                }
                match found {
                    Some(found) => self.select_match(found, index),
                    None => self.notice = Some("No results".to_owned()),
                }
            }
            Outcome::Index { at, index } => {
                let primary = current.primary();
                if let Some(i) = index
                    && primary.start() == at
                {
                    f.index = Some((primary.range(), i));
                }
            }
            _ => {}
        }
    }

    /// Replace (`Ctrl+Shift+1`, `Enter` in the replace field): replaces the
    /// selection when it is a match, then moves to the next match.
    pub(super) fn replace_one(&mut self) {
        if let Some(reason) = self.doc.locks.edit_refusal() {
            self.notice = Some(format!("Not editable: {reason}"));
            return;
        }
        if self
            .find
            .as_ref()
            .is_none_or(|f| f.find.as_str().is_empty())
        {
            self.open_find(true);
            return;
        }
        let at = self.doc.selections.primary().range();
        let scope = self.find_scope();
        let f = self.find.as_ref().expect("search box");
        let (query, replacement, preserve) = (f.query(), f.replacement(), f.preserve);
        self.send_op(
            OpPurpose::ReplaceOne,
            query,
            Work::ReplaceOne {
                at,
                scope,
                replacement,
                preserve,
            },
            true,
        );
    }

    /// Replace All (`Ctrl+Alt+Enter`): one undo step, or nothing.
    pub(super) fn replace_all(&mut self) {
        if let Some(reason) = self.doc.locks.edit_refusal() {
            self.notice = Some(format!("Not editable: {reason}"));
            return;
        }
        if self
            .find
            .as_ref()
            .is_none_or(|f| f.find.as_str().is_empty())
        {
            self.open_find(true);
            return;
        }
        let scope = self.find_scope();
        let cap = self.settings.replace_all_max_mib * 1024 * 1024;
        let f = self.find.as_ref().expect("search box");
        let (query, replacement, preserve) = (f.query(), f.replacement(), f.preserve);
        self.send_op(
            OpPurpose::ReplaceAll { retried: false },
            query,
            Work::ReplaceAll {
                scope,
                replacement,
                preserve,
                cap,
            },
            true,
        );
        self.notice = Some("Replacing\u{2026}".to_owned());
    }

    /// `Alt+Enter`: selects every match of the search box.
    pub(super) fn select_all_matches(&mut self) {
        let Some(query) = self
            .find
            .as_ref()
            .map(|f| f.query())
            .filter(|q| !q.text.is_empty())
        else {
            return;
        };
        let scope = self.find_scope();
        let limit = self.settings.multi_cursor_limit;
        self.send_op(
            OpPurpose::SelectAll,
            query,
            Work::SelectAll { scope, limit },
            true,
        );
    }

    /// Applies a replacement reply on the text it was computed for.
    pub(super) fn apply_replaced(&mut self, at: Range<u64>, text: Option<Vec<u8>>) {
        let from = match text {
            Some(text) => {
                let end = at.start + text.len() as u64;
                let cmd = Command {
                    tx: Transaction::single(Edit::new(at, text)),
                    kind: edit::EditKind::Other,
                    collapse: true,
                };
                self.run(Some(cmd), None);
                end
            }
            None => self.doc.selections.primary().start(),
        };
        self.query_changed(false);
        self.find_from(from, false);
    }

    pub(super) fn apply_replaced_all(&mut self, edits: Vec<Edit>) {
        let n = edits.len();
        if n == 0 {
            self.notice = Some("No results".to_owned());
            return;
        }
        let cmd = Command {
            tx: Transaction::new(edits),
            kind: edit::EditKind::Other,
            collapse: false,
        };
        self.run(Some(cmd), None);
        if self
            .notice
            .as_deref()
            .is_none_or(|n| !n.starts_with("Not editable"))
        {
            let s = if n == 1 { "" } else { "s" };
            self.notice = Some(format!("Replaced {} occurrence{s}", thousands(n as u64)));
        }
    }

    pub(super) fn retry_replace_all(&mut self, auth: Option<FindAuth>) {
        let scope = self.find_scope();
        let cap = self.settings.replace_all_max_mib * 1024 * 1024;
        let Some(f) = &self.find else {
            return;
        };
        if auth.is_none_or(|a| a != f.auth()) {
            self.notice = Some("The search changed; nothing was replaced".to_owned());
            return;
        }
        let (query, replacement, preserve) = (f.query(), f.replacement(), f.preserve);
        self.send_op(
            OpPurpose::ReplaceAll { retried: true },
            query,
            Work::ReplaceAll {
                scope,
                replacement,
                preserve,
                cap,
            },
            true,
        );
    }

    /// Keys while the box has the focus.
    pub(super) fn find_key(&mut self, key: Key, ctx: &mut Ctx) -> bool {
        let Some(f) = &mut self.find else {
            return false;
        };
        let Some(focus) = f.focus else {
            return false;
        };
        let (ctrl, shift) = (key.ctrl, key.shift);
        let field = match focus {
            Field::Find => &mut f.find,
            Field::Replace => &mut f.replace,
        };
        let word = |word, char| if ctrl { word } else { char };
        let mut edited = false;
        match key.vk {
            VK_ESCAPE => {
                self.close_find();
                return true;
            }
            VK_RETURN if ctrl && !shift => edited = field.insert_lines("\n"),
            VK_RETURN if focus == Field::Replace && !ctrl => {
                self.replace_one();
                return true;
            }
            VK_RETURN if !ctrl => {
                self.find_next(shift);
                return true;
            }
            VK_TAB if !ctrl => {
                if f.replace_open {
                    f.focus = Some(match focus {
                        Field::Find => Field::Replace,
                        Field::Replace => Field::Find,
                    });
                }
                return true;
            }
            VK_LEFT => {
                field.move_caret(word(Motion::WordLeft, Motion::Left), shift);
            }
            VK_RIGHT => {
                field.move_caret(word(Motion::WordRight, Motion::Right), shift);
            }
            VK_HOME => {
                field.move_caret(Motion::Home, shift);
            }
            VK_END => {
                field.move_caret(Motion::End, shift);
            }
            VK_BACK => edited = field.delete(false, ctrl),
            VK_DELETE => edited = field.delete(true, ctrl),
            VK_A if ctrl && !shift => {
                field.select_all();
            }
            VK_C | VK_X if ctrl && !shift => {
                let text = field.selected_text().to_owned();
                if !text.is_empty() {
                    ctx.set_clipboard(&text, clip::FORMAT, &[]);
                    if key.vk == VK_X {
                        edited = field.delete(false, false);
                    }
                }
            }
            VK_V if ctrl && !shift => {
                if let Some((text, _)) = ctx.clipboard(clip::FORMAT) {
                    edited = field.insert_lines(&text);
                }
            }
            VK_Z | VK_Y if ctrl => {}
            _ => {
                if let Some(def) = commands::for_key(key)
                    && matches!(
                        def.action,
                        Action::Find
                            | Action::Replace
                            | Action::FindNext
                            | Action::FindPrevious
                            | Action::ReplaceOne
                            | Action::ReplaceAll
                            | Action::ShowCommands
                            | Action::ShowFavorites
                            | Action::Save
                            | Action::Close
                            | Action::Quit
                            | Action::ZoomIn
                            | Action::ZoomOut
                            | Action::ZoomReset
                    )
                {
                    self.perform(def.action, ctx);
                }
            }
        }
        if edited && focus == Field::Find {
            self.query_changed(true);
        }
        true
    }

    /// A typed character while the box has the focus.
    pub(super) fn find_typed(&mut self, ch: char) {
        let Some(f) = &mut self.find else {
            return;
        };
        let mut buf = [0u8; 4];
        let text = ch.encode_utf8(&mut buf);
        match f.focus {
            Some(Field::Find) => {
                if f.find.insert(text) {
                    self.query_changed(true);
                }
            }
            Some(Field::Replace) => {
                f.replace.insert(text);
            }
            None => {}
        }
    }

    pub(super) fn find_geometry(&self) -> Option<FindGeometry> {
        let f = self.find.as_ref().filter(|f| f.open)?;
        let layout = self.layout();
        let right = layout.text.x + layout.text.w as i32;
        Some(FindGeometry::new(
            right,
            layout.title.h as i32,
            self.ui_cell(),
            f.replace_open,
        ))
    }

    /// A click on the box; returns false when it is outside.
    pub(super) fn find_click(&mut self, x: i32, y: i32) -> bool {
        let Some(hit) = self.find_geometry().and_then(|g| g.hit(x, y)) else {
            return false;
        };
        let f = self.find_box();
        match hit {
            FindHit::Fold => {
                f.replace_open = !f.replace_open;
                if !f.replace_open && f.focus == Some(Field::Replace) {
                    f.focus = Some(Field::Find);
                }
            }
            FindHit::FindText => f.focus = Some(Field::Find),
            FindHit::ReplaceText => f.focus = Some(Field::Replace),
            FindHit::Toggle(i) => {
                let action = [
                    Action::ToggleMatchCase,
                    Action::ToggleWholeWord,
                    Action::ToggleGlob,
                    Action::ToggleRegex,
                ][i];
                self.toggle_find_option(action);
            }
            FindHit::Preserve => f.preserve = !f.preserve,
            FindHit::Prev => self.find_next(true),
            FindHit::Next => self.find_next(false),
            FindHit::InSelection => self.toggle_find_in_selection(),
            FindHit::Close => self.close_find(),
            FindHit::ReplaceOne => self.replace_one(),
            FindHit::ReplaceAll => self.replace_all(),
            FindHit::Panel => {}
        }
        true
    }

    /// Fingerprint of what the box shows.
    pub(super) fn find_view(&self, h: &mut impl Hasher) {
        let Some(f) = self.find.as_ref().filter(|f| f.open) else {
            0u8.hash(h);
            return;
        };
        1u8.hash(h);
        (&f.find, &f.replace, f.focus, f.replace_open).hash(h);
        (f.mode, f.match_case, f.whole_word, f.preserve, &f.scope).hash(h);
        (
            f.generation,
            &f.matches_for,
            f.count,
            &f.index,
            &f.error,
            &f.warning,
        )
            .hash(h);
        f.matches.len().hash(h);
    }

    pub(super) fn draw_find(&mut self) {
        let Some(g) = self.find_geometry() else {
            return;
        };
        let Some(f) = &self.find else {
            return;
        };
        let cell = self.ui_cell();
        let (count, no_results) = f.counter(self.doc.selections.primary().range());
        let message = match (&f.error, &f.warning) {
            (Some(e), _) => Some((e.as_str(), Severity::Error)),
            (None, Some(w)) => Some((w.as_str(), Severity::Warning)),
            _ => None,
        };
        let frame = FindFrame {
            cell,
            theme: &self.theme,
            find: field_view(&f.find, f.focus == Some(Field::Find)),
            replace: field_view(&f.replace, f.focus == Some(Field::Replace)),
            toggles: [
                f.match_case,
                f.whole_word,
                f.mode == SearchMode::Glob,
                f.mode == SearchMode::Regex,
            ],
            preserve: f.preserve,
            in_selection: f.scope.is_some(),
            count: &count,
            no_results,
            message,
        };
        rte_view::draw_find(&g, &frame, &mut self.list.popup);
    }
}

/// A field with its caret and selection in cells.
fn field_view(field: &TextField, focused: bool) -> FindField<'_> {
    let cols = |end: usize| field.as_str()[..end].width() as u32;
    let sel = field.selection();
    FindField {
        text: field.as_str(),
        caret: cols(field.caret()),
        selection: (cols(sel.start), cols(sel.end)),
        focused,
    }
}

/// Escapes a seed for RegEx mode.
fn escape_regex(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for c in text.chars() {
        if c.is_ascii_punctuation() {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Purpose of an operation request on the [`Lane::Op`] lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OpPurpose {
    Occurrence { move_last: bool, hops: usize },
    SelectAll,
    ReplaceOne,
    ReplaceAll { retried: bool },
}

/// The search-box input an operation was computed for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FindAuth {
    query: Query,
    replace: String,
    preserve: bool,
    scope: Option<Range<u64>>,
}

/// An operation in flight: it applies only to the text and selections it
/// was computed for, and to the search-box input that started it.
pub(super) struct PendingOp {
    pub(super) generation: u64,
    pub(super) purpose: OpPurpose,
    pub(super) snapshot: Snapshot,
    pub(super) sels: Selections,
    pub(super) auth: Option<FindAuth>,
}

impl Editor {
    /// Sends an operation; `from_box` binds it to the search-box input.
    pub(super) fn send_op(&mut self, purpose: OpPurpose, query: Query, work: Work, from_box: bool) {
        self.op_generation += 1;
        let generation = self.op_generation;
        let snapshot = self.doc.snapshot.clone();
        let auth = from_box
            .then(|| self.find.as_ref().map(|f| f.auth()))
            .flatten();
        self.pending_op = Some(PendingOp {
            generation,
            purpose,
            snapshot: snapshot.clone(),
            sels: self.doc.selections.clone(),
            auth,
        });
        self.search_thread().send(Job {
            lane: Lane::Op,
            generation,
            snapshot,
            query,
            work,
        });
    }

    /// True when the search-box input that started the pending operation
    /// changed.
    fn op_input_changed(&self) -> bool {
        let Some(auth) = self.pending_op.as_ref().and_then(|op| op.auth.as_ref()) else {
            return false;
        };
        self.find.as_ref().is_none_or(|f| f.auth() != *auth)
    }

    /// Cancels a pending operation whose search-box input changed.
    fn revoke_stale_op(&mut self) {
        if !self.op_input_changed() {
            return;
        }
        let op = self.pending_op.take().expect("pending operation");
        if let Some(t) = &self.search {
            t.cancel(Lane::Op, op.generation + 1);
        }
        self.op_generation = self.op_generation.max(op.generation + 1);
        self.notice = Some(match op.purpose {
            OpPurpose::SelectAll => "The search changed; nothing was selected".to_owned(),
            _ => "The search changed; nothing was replaced".to_owned(),
        });
    }

    fn op_reply(&mut self, reply: Reply) {
        if self.op_input_changed() {
            self.revoke_stale_op();
            return;
        }
        let Some(op) = self
            .pending_op
            .take_if(|op| op.generation == reply.generation)
        else {
            return;
        };
        let current = reply.snapshot.same_tree(&self.doc.snapshot)
            && op.snapshot.same_tree(&self.doc.snapshot)
            && op.sels == self.doc.selections;
        if !current {
            if op.purpose == (OpPurpose::ReplaceAll { retried: false }) {
                self.retry_replace_all(op.auth);
            } else if matches!(op.purpose, OpPurpose::ReplaceAll { .. }) {
                self.notice = Some("The text changed; nothing was replaced".to_owned());
            }
            return;
        }
        match (op.purpose, reply.outcome) {
            (_, Outcome::Invalid(error)) => self.notice = Some(error),
            (OpPurpose::ReplaceOne, Outcome::Replaced { at, text }) => {
                self.apply_replaced(at, text);
            }
            (OpPurpose::ReplaceAll { .. }, Outcome::ReplacedAll(result)) => match result {
                Ok(edits) => self.apply_replaced_all(edits),
                Err(error) => self.notice = Some(error),
            },
            (OpPurpose::SelectAll, Outcome::Selected(result)) => self.apply_select_all(result),
            (purpose, Outcome::Found { found, .. }) => {
                if let OpPurpose::Occurrence { move_last, hops } = purpose
                    && let Some(next) = self.apply_occurrence(found, move_last, hops)
                {
                    self.next_occurrence(next, move_last, hops + 1);
                }
            }
            _ => {}
        }
    }
}
