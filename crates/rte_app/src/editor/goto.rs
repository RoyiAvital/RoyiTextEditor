//! Go to Line/Column (`Ctrl+G`): the palette in a line mode. Typing previews
//! the line, `Enter` places the caret, `Escape` restores the scroll.

use super::*;
use crate::palette::Choices;

/// A parsed query: 1-based line and column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct LineTarget {
    pub line: u64,
    pub col: Option<u64>,
}

/// Parses `N`, `N:C`, `N,C` or `-N` (lines from the end) for a document of
/// `lines` lines; lines past the end go to the last one. `None` otherwise.
pub(super) fn parse_target(query: &str, lines: u64) -> Option<LineTarget> {
    let query = query.trim();
    let query = query.strip_prefix(':').unwrap_or(query);
    let (line, col) = match query.split_once([':', ',']) {
        Some((line, col)) => (line.trim(), Some(col.trim())),
        None => (query, None),
    };
    let line: i64 = line.parse().ok()?;
    let line = if line < 0 {
        (lines as i64 + 1 + line).max(1) as u64
    } else {
        (line.max(1) as u64).min(lines)
    };
    let col = match col {
        None | Some("") => None,
        Some(col) => Some(col.parse::<u64>().ok()?.max(1)),
    };
    Some(LineTarget { line, col })
}

impl Editor {
    pub(super) fn open_go_to_line(&mut self) {
        self.open_palette(Mode::All);
        let Some(Popup::Palette(p)) = &mut self.popup else {
            return;
        };
        p.set_choices(Mode::Line, Choices::default());
        self.line_preview = Some(self.scroll);
        self.update_go_to_line();
    }

    /// The row and the preview for the current query.
    pub(super) fn update_go_to_line(&mut self) {
        let s = &self.doc.snapshot;
        let lines = s.line_count();
        let head = self.doc.selections.primary().head;
        let current = (s.line_of(head) + 1, self.caret_column(head) + 1);
        let Some(Popup::Palette(p)) = &mut self.popup else {
            return;
        };
        let target = parse_target(p.query.as_str(), lines);
        let label = match target {
            Some(LineTarget {
                line,
                col: Some(col),
            }) => format!("Go to line {line}, column {col}"),
            Some(LineTarget { line, col: None }) => format!("Go to line {line}"),
            None => format!(
                "Current line {}, column {}. Type a line number between 1 and {lines}",
                current.0, current.1
            ),
        };
        p.choices = Choices {
            labels: vec![label],
            hints: vec![String::new()],
            keep_order: true,
            groups: Vec::new(),
        };
        if let Some(store) = &self.store {
            p.refresh(store, self.settings.recent_count);
        }
        match target {
            Some(t) => self.show_line(t.line - 1),
            None => {
                if let Some(scroll) = self.line_preview {
                    self.scroll = scroll;
                }
            }
        }
    }

    /// `Enter` in the line mode: the caret goes to the target.
    pub(super) fn go_to_line(&mut self, query: &str) {
        let preview = self.line_preview.take();
        let s = &self.doc.snapshot;
        let Some(target) = parse_target(query, s.line_count()) else {
            if let Some(scroll) = preview {
                self.scroll = scroll;
            }
            return;
        };
        let content = edit::content_range(s, target.line - 1);
        let tab = self.options.tab_width;
        let at = match target.col {
            Some(col) => edit::offset_in_line(s, content, col - 1, tab),
            None => content.start,
        };
        self.select_single(Selection::caret(at));
        self.show_line(target.line - 1);
        self.reveal();
    }

    /// The palette closed another way: the preview's scroll is undone.
    pub(super) fn end_line_preview(&mut self) {
        let open = matches!(&self.popup, Some(Popup::Palette(p)) if p.mode == Mode::Line);
        if !open && let Some(scroll) = self.line_preview.take() {
            self.scroll = scroll;
        }
    }

    /// Scrolls `line` into the upper third of the view unless visible.
    pub(super) fn show_line(&mut self, line: u64) {
        let page = self.page();
        if line < self.scroll.top_line || line >= self.scroll.top_line + page {
            self.scroll.top_line = line.saturating_sub(page / 3);
            self.clamp_scroll();
        }
    }
}
