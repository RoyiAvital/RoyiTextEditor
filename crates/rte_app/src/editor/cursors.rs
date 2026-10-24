//! Multi-cursor commands (VS Code keys): cursors above and below, line ends,
//! line selection, occurrences of the selection, column selection and
//! cursor undo.

use std::ops::Range;

use rte_search::{Matcher, Mode as SearchMode, Query, Searcher};

use super::find::OpPurpose;
use super::*;
use crate::search::Work;

/// Cursor states kept for `Ctrl+U`.
const CURSOR_HISTORY: usize = 64;
/// Selections kept in all cursor states together.
const CURSOR_HISTORY_SELECTIONS: usize = 256 * 1024;
/// Bytes an occurrence search scans on the UI before the search thread takes over.
const UI_SCAN: u64 = 1024 * 1024;
/// Longest selection searched for occurrences.
const OCCURRENCE_MAX: u64 = 64 * 1024;

/// A column selection between two (line, column) cells and the selections
/// it produced; any other selection change ends it.
pub(super) struct Column {
    anchor: (u64, u64),
    head: (u64, u64),
    sels: Selections,
}

impl Editor {
    /// Records the selections for `Ctrl+U`, with the text they belong to.
    /// Oldest states go first beyond [`CURSOR_HISTORY`] states or
    /// [`CURSOR_HISTORY_SELECTIONS`] selections; larger states are not kept.
    pub(super) fn push_cursors(&mut self) {
        let sels = &self.doc.selections;
        if self
            .cursor_history
            .back()
            .is_some_and(|(text, last)| last == sels && text.same_tree(&self.doc.snapshot))
        {
            return;
        }
        if sels.len() > CURSOR_HISTORY_SELECTIONS {
            return;
        }
        let mut kept: usize = self.cursor_history.iter().map(|(_, s)| s.len()).sum();
        while self.cursor_history.len() >= CURSOR_HISTORY
            || kept + sels.len() > CURSOR_HISTORY_SELECTIONS
        {
            let (_, old) = self.cursor_history.pop_front().expect("a kept state");
            kept -= old.len();
        }
        self.cursor_history
            .push_back((self.doc.snapshot.clone(), sels.clone()));
    }

    /// Selects `sels` (recorded for cursor undo) unless they exceed the
    /// cursor limit.
    fn set_cursors(&mut self, sels: Selections) -> bool {
        let limit = self.settings.multi_cursor_limit;
        if sels.len() > limit {
            self.refuse_cursors();
            return false;
        }
        self.push_cursors();
        self.doc.select(sels);
        self.reveal();
        true
    }

    fn refuse_cursors(&mut self) {
        let limit = self.settings.multi_cursor_limit;
        self.notice = Some(format!("At most {limit} cursors; nothing changed"));
    }

    /// `Ctrl+U`. States recorded on another text (an edit since) are
    /// dropped: their offsets may no longer be valid positions.
    pub(super) fn cursor_undo(&mut self) {
        while let Some((text, sels)) = self.cursor_history.pop_back() {
            if text.same_tree(&self.doc.snapshot) && sels != self.doc.selections {
                self.doc.select(sels);
                self.reveal();
                return;
            }
        }
    }

    pub(super) fn add_cursor_vertical(&mut self, lines: i64) {
        let tab = self.options.tab_width;
        let sels = edit::add_cursors_vertical(&self.doc.snapshot, &self.doc.selections, lines, tab);
        self.set_cursors(sels);
    }

    pub(super) fn cursors_at_line_ends(&mut self) {
        let limit = self.settings.multi_cursor_limit;
        match edit::cursors_at_line_ends(&self.doc.snapshot, &self.doc.selections, limit) {
            Some(sels) => {
                self.set_cursors(sels);
            }
            None => self.refuse_cursors(),
        }
    }

    pub(super) fn expand_line_selection(&mut self) {
        let sels = edit::select_lines(&self.doc.snapshot, &self.doc.selections);
        self.set_cursors(sels);
    }

    /// Replaces an empty primary selection by the word at its caret; the
    /// next occurrences then match whole words. Returns false when there is
    /// no word.
    fn select_word(&mut self) -> bool {
        let sels = self.doc.selections.clone();
        let primary = sels.primary();
        let word = edit::word_at(&self.doc.snapshot, primary.head);
        if word.is_empty() {
            return false;
        }
        let mut all = sels.as_slice().to_vec();
        all[sels.primary_index()] = Selection::new(word.start, word.end);
        let next = Selections::new(all, sels.primary_index());
        if self.set_cursors(next) {
            self.occurrence_word = String::from_utf8(self.doc.snapshot.to_vec(word)).ok();
        }
        true
    }

    /// The literal query of the primary selection.
    fn occurrence_query(&mut self) -> Option<Query> {
        let primary = self.doc.selections.primary();
        let range = primary.range();
        if range.end - range.start > OCCURRENCE_MAX {
            self.notice = Some("The selection is too long to search for".to_owned());
            return None;
        }
        let Ok(text) = String::from_utf8(self.doc.snapshot.to_vec(range)) else {
            self.notice = Some("The selection is not valid UTF-8".to_owned());
            return None;
        };
        let whole_word = self.occurrence_word.as_deref() == Some(text.as_str());
        Some(Query {
            text,
            mode: SearchMode::Text,
            match_case: true,
            whole_word,
        })
    }

    /// `Ctrl+D` (and `Ctrl+K Ctrl+D` with `move_last`): selects the next
    /// occurrence of the primary selection; an empty selection selects its word.
    pub(super) fn add_next_occurrence(&mut self, move_last: bool) {
        if self.doc.selections.primary().is_empty() {
            self.select_word();
            return;
        }
        let from = self.doc.selections.primary().end();
        self.next_occurrence(from, move_last, 0);
    }

    /// Searches the next occurrence from `from`, wrapping; `hops` occurrences
    /// already selected were passed.
    pub(super) fn next_occurrence(&mut self, mut from: u64, move_last: bool, mut hops: usize) {
        let Some(query) = self.occurrence_query() else {
            return;
        };
        let Ok(Some(matcher)) = Matcher::new(&query) else {
            return;
        };
        let snapshot = self.doc.snapshot.clone();
        let len = snapshot.len();
        let never = || false;
        let mut s = Searcher::new(&matcher, &snapshot, &never);
        loop {
            let ahead = (from + UI_SCAN).min(len);
            let mut found = s.find(from, ahead);
            let mut scanned = ahead == len;
            if found.is_none() && scanned && from <= UI_SCAN {
                found = s.find(0, from.min(len));
            } else if found.is_none() {
                scanned = false;
            }
            if found.is_none() && !scanned {
                self.send_op(
                    OpPurpose::Occurrence { move_last, hops },
                    query,
                    Work::Find {
                        from,
                        scope: 0..len,
                        backwards: false,
                    },
                    false,
                );
                return;
            }
            match self.apply_occurrence(found, move_last, hops) {
                Some(next) => {
                    from = next;
                    hops += 1;
                }
                None => return,
            }
        }
    }

    /// Adds (or moves the primary to) `found`. When it is already selected,
    /// returns where to search next; after passing every selection once
    /// there is nothing left to add.
    pub(super) fn apply_occurrence(
        &mut self,
        found: Option<Range<u64>>,
        move_last: bool,
        hops: usize,
    ) -> Option<u64> {
        let Some(found) = found else {
            self.notice = Some("No more occurrences".to_owned());
            return None;
        };
        let sels = self.doc.selections.clone();
        let own = move_last && sels.primary().range() == found;
        if !own && sels.as_slice().iter().any(|s| s.range() == found) {
            if hops >= sels.len() {
                self.notice = Some("No more occurrences".to_owned());
                return None;
            }
            return Some(found.end);
        }
        let mut all = sels.as_slice().to_vec();
        if move_last {
            all.remove(sels.primary_index());
        }
        all.push(Selection::new(found.start, found.end));
        let primary = all.len() - 1;
        self.set_cursors(Selections::new(all, primary));
        None
    }

    /// `Ctrl+Shift+L` / `Ctrl+F2`: selects every occurrence of the primary
    /// selection (or of its word).
    pub(super) fn select_all_occurrences(&mut self) {
        if self.doc.selections.primary().is_empty() && !self.select_word() {
            return;
        }
        let Some(query) = self.occurrence_query() else {
            return;
        };
        let len = self.doc.snapshot.len();
        let limit = self.settings.multi_cursor_limit;
        self.send_op(
            OpPurpose::SelectAll,
            query,
            Work::SelectAll {
                scope: 0..len,
                limit,
            },
            false,
        );
    }

    pub(super) fn apply_select_all(&mut self, result: Result<Vec<Range<u64>>, String>) {
        let ranges = match result {
            Err(error) => {
                self.notice = Some(error);
                return;
            }
            Ok(ranges) if ranges.is_empty() => {
                self.notice = Some("No results".to_owned());
                return;
            }
            Ok(ranges) => ranges,
        };
        let at = self.doc.selections.primary().start();
        let primary = ranges.iter().position(|r| r.start >= at).unwrap_or(0);
        let sels = Selections::new(
            ranges
                .into_iter()
                .map(|r| Selection::new(r.start, r.end))
                .collect(),
            primary,
        );
        if self.set_cursors(sels)
            && let Some(f) = &mut self.find
        {
            f.focus = None;
        }
    }

    /// `Ctrl+Shift+Alt+Arrows` / `PgUp` / `PgDn`: grows a column selection
    /// from the primary caret.
    pub(super) fn column_select(&mut self, lines: i64, cols: i64) {
        let tab = self.options.tab_width;
        let s = &self.doc.snapshot;
        let (anchor, mut head) = match &self.column {
            Some(c) if c.sels == self.doc.selections => (c.anchor, c.head),
            _ => {
                let p = self.doc.selections.primary();
                (
                    edit::cell_of(s, p.anchor, tab),
                    edit::cell_of(s, p.head, tab),
                )
            }
        };
        let last = s.line_count() - 1;
        head.0 = head.0.saturating_add_signed(lines).min(last);
        head.1 = head.1.saturating_add_signed(cols);
        self.select_column(anchor, head);
    }

    pub(super) fn select_column(&mut self, anchor: (u64, u64), head: (u64, u64)) {
        self.place_column(anchor, head, true);
    }

    /// Selects the column between two cells; `record` keeps the previous
    /// selections for cursor undo (drags record only their start).
    pub(super) fn place_column(&mut self, anchor: (u64, u64), head: (u64, u64), record: bool) {
        if self
            .column
            .as_ref()
            .is_some_and(|c| (c.anchor, c.head) == (anchor, head) && c.sels == self.doc.selections)
        {
            return;
        }
        let tab = self.options.tab_width;
        let limit = self.settings.multi_cursor_limit;
        let Some(sels) = edit::box_select(&self.doc.snapshot, anchor, head, tab, limit) else {
            self.refuse_cursors();
            return;
        };
        if record {
            self.push_cursors();
        }
        self.doc.select(sels);
        self.reveal();
        self.column = Some(Column {
            anchor,
            head,
            sels: self.doc.selections.clone(),
        });
    }

    /// Line and column under a point, not clamped to the line's end.
    pub(super) fn cell_at(&self, x: i32, y: i32) -> (u64, u64) {
        let layout = self.layout();
        let cell = self.renderer.cell();
        let last = self.doc.snapshot.line_count() - 1;
        let row = (y - layout.text.y).div_euclid(cell.height.max(1) as i32);
        let line = (self.scroll.top_line as i64 + i64::from(row)).clamp(0, last as i64) as u64;
        let dx = (x - layout.text.x).max(0) as u64;
        let cw = u64::from(cell.width.max(1));
        (line, self.scroll.first_col + (dx + cw / 2) / cw)
    }

    /// `Alt+Click`: adds a caret at `pos` as the primary, or removes the
    /// caret there when it is not the only one.
    pub(super) fn toggle_cursor(&mut self, pos: u64) {
        let sels = self.doc.selections.clone();
        let mut all = sels.as_slice().to_vec();
        if all.len() > 1
            && let Some(i) = all.iter().position(|s| s.is_empty() && s.head == pos)
        {
            all.remove(i);
            let primary = all.len() - 1;
            self.set_cursors(Selections::new(all, primary));
            return;
        }
        all.push(Selection::caret(pos));
        let primary = all.len() - 1;
        self.set_cursors(Selections::new(all, primary));
    }
}
