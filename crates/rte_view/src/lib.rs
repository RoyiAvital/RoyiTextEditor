//! Viewport layout: turns the visible part of a snapshot plus window chrome
//! into a [`DisplayList`] of filled rectangles and glyph cells.
//!
//! All geometry is integer: every character occupies one or two cells of a
//! fixed size (monospaced font, LTR, no shaping).

mod find;
mod popup;

pub use find::{
    FIND_SYMBOLS, FindField, FindFrame, FindGeometry, FindHit, LINE_BREAK, ReplaceGeometry,
    Severity, TOGGLES, draw_find, draw_find_tooltip,
};
pub use popup::{
    Image, Layer, PaletteFrame, PaletteGeometry, PaletteRow, PanelFrame, PanelGeometry, PanelLine,
    Star, Tone, draw_palette, draw_panel,
};

use rte_edit::{Selection, column_after, column_of, content_range, offset_in_line, scan_end};
use rte_text::Snapshot;
use std::fmt::Write;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Color as `0x00RRGGBB`.
pub type Color = u32;

/// Bytes scanned per visible row and frame; longer lines are cut off.
pub const ROW_SCAN_LIMIT: usize = 64 * 1024;

/// Zero-width marks drawn on one cell; further marks are skipped (not removed).
pub const MAX_MARKS_PER_CELL: u32 = 4;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Theme {
    pub background: Color,
    pub text: Color,
    pub control: Color,
    pub line_number: Color,
    pub title_background: Color,
    pub title_text: Color,
    pub status_background: Color,
    pub status_text: Color,
    pub selection: Color,
    pub caret: Color,
    /// Unsaved marker of a document protected by its file.
    pub unsaved: Color,
    /// Unsaved marker of a document that cannot be saved (untitled).
    pub at_risk: Color,
    pub prompt_background: Color,
    pub prompt_text: Color,
    pub popup_background: Color,
    pub popup_border: Color,
    pub popup_input: Color,
    pub popup_dim: Color,
    /// Matched characters of a filtered row.
    pub popup_match: Color,
    pub popup_selected: Color,
    pub favorite: Color,
    pub link: Color,
    /// Visible search matches other than the current one.
    pub search_match: Color,
    /// The current match (also the selection) and its border.
    pub search_current: Color,
    pub search_current_border: Color,
    /// Find-in-selection range.
    pub search_range: Color,
    pub error_text: Color,
    pub error_background: Color,
    pub error_border: Color,
    /// Text colors of the syntax styles, indexed by [`SyntaxSpan::style`]:
    /// keyword, string, comment, number, type, function, constant, operator,
    /// punctuation, property, tag, attribute.
    pub syntax: [Color; 12],
    /// Background of a matching bracket pair.
    pub bracket_match: Color,
    /// The scroll slider, and while it is dragged.
    pub scroll_slider: Color,
    pub scroll_slider_active: Color,
}

impl Theme {
    pub const DARK: Theme = Theme {
        background: 0x1E1F22,
        text: 0xD4D4D4,
        control: 0x6A6F78,
        line_number: 0x5C6370,
        title_background: 0x18191B,
        title_text: 0xA0A4AB,
        status_background: 0x18191B,
        status_text: 0x8B9099,
        selection: 0x264F78,
        caret: 0xAEAFAD,
        unsaved: 0x4FA3E0,
        at_risk: 0xE0A040,
        prompt_background: 0x5A3E14,
        prompt_text: 0xF0E0C0,
        popup_background: 0x26282C,
        popup_border: 0x3E4148,
        popup_input: 0x1B1C1F,
        popup_dim: 0x80858F,
        popup_match: 0x5FB3F0,
        popup_selected: 0x264F78,
        favorite: 0xE8A33A,
        link: 0x5FB3F0,
        search_match: 0x603317,
        search_current: 0x515C6A,
        search_current_border: 0x8FA3BA,
        search_range: 0x2A2D33,
        error_text: 0xF48771,
        error_background: 0x5A1D1D,
        error_border: 0xE05252,
        syntax: [
            0x569CD6, // keyword
            0xCE9178, // string
            0x6A9955, // comment
            0xB5CEA8, // number
            0x4EC9B0, // type
            0xDCDCAA, // function
            0x4FC1FF, // constant
            0xD4D4D4, // operator
            0xA9AEB6, // punctuation
            0x9CDCFE, // property
            0x569CD6, // tag
            0x9CDCFE, // attribute
        ],
        bracket_match: 0x3E4A45,
        // VS Code's dark slider colors over the background.
        scroll_slider: 0x434446,
        scroll_slider_active: 0x5E5F61,
    };
}

/// Size of one character cell in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w as i32 && y < self.y + self.h as i32
    }

    /// Intersection, or `None` when empty.
    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = (self.x + self.w as i32).min(other.x + other.w as i32);
        let y1 = (self.y + self.h as i32).min(other.y + other.h as i32);
        (x0 < x1 && y0 < y1).then(|| Rect {
            x: x0,
            y: y0,
            w: (x1 - x0) as u32,
            h: (y1 - y0) as u32,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    pub tab_width: u32,
    pub line_numbers: bool,
    /// The title strip is shown (not in full screen).
    pub title_strip: bool,
    /// Width in pixels of the scroll strip right of the text (0: none).
    pub scroll_strip: u32,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            tab_width: 4,
            line_numbers: true,
            title_strip: true,
            scroll_strip: 0,
        }
    }
}

/// Screen areas for a window size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub title: Rect,
    pub gutter: Rect,
    pub text: Rect,
    /// Strip of the scroll slider, right of the text.
    pub scrollbar: Rect,
    pub status: Rect,
    /// Fully or partially visible text rows.
    pub rows: u32,
    /// Fully visible text columns.
    pub cols: u32,
}

impl Layout {
    pub fn new(width: u32, height: u32, cell: Cell, line_count: u64, options: &Options) -> Layout {
        let bar = cell.height + cell.height / 2;
        let title = Rect {
            x: 0,
            y: 0,
            w: width,
            h: if options.title_strip {
                bar.min(height)
            } else {
                0
            },
        };
        let status_h = bar.min(height.saturating_sub(title.h));
        let status = Rect {
            x: 0,
            y: (height - status_h) as i32,
            w: width,
            h: status_h,
        };
        let body_y = title.h as i32;
        let body_h = height - title.h - status_h;
        let gutter_cols = if options.line_numbers {
            digits(line_count) + 2
        } else {
            1
        };
        let gutter = Rect {
            x: 0,
            y: body_y,
            w: (gutter_cols * cell.width).min(width),
            h: body_h,
        };
        let strip = options.scroll_strip.min(width - gutter.w);
        let text = Rect {
            x: gutter.w as i32,
            y: body_y,
            w: width - gutter.w - strip,
            h: body_h,
        };
        let scrollbar = Rect {
            x: text.x + text.w as i32,
            y: body_y,
            w: strip,
            h: body_h,
        };
        Layout {
            title,
            gutter,
            text,
            scrollbar,
            status,
            rows: body_h.div_ceil(cell.height.max(1)),
            cols: text.w / cell.width.max(1),
        }
    }
}

fn digits(n: u64) -> u32 {
    n.max(1).ilog10() + 1
}

/// The scroll slider in `layout.scrollbar` for `line_count` lines, a view
/// from `top_line` showing `page` full rows, at least `min` pixels high.
/// The bottom of the track is the last line at the top of the view (as far
/// as scrolling goes). `None` when there is no strip or the file fits.
pub fn slider(
    layout: &Layout,
    line_count: u64,
    top_line: u64,
    page: u64,
    min: u32,
) -> Option<Rect> {
    let track = layout.scrollbar;
    let lines = line_count.max(1);
    if track.w == 0 || track.h == 0 || (lines <= page && top_line == 0) {
        return None;
    }
    let last = lines - 1;
    let h = u64::from(track.h);
    let size = (h * page / (last + page)).max(u64::from(min)).min(h);
    let y = ((h - size) * top_line.min(last))
        .checked_div(last)
        .unwrap_or(0);
    Some(Rect {
        x: track.x,
        y: track.y + y as i32,
        w: track.w,
        h: size as u32,
    })
}

/// Full text rows of a layout (at least one).
pub fn page_rows(layout: &Layout, cell: Cell) -> u64 {
    u64::from((layout.text.h / cell.height.max(1)).max(1))
}

/// First visible line and column of the text area.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Scroll {
    pub top_line: u64,
    pub first_col: u64,
}

/// Text of the title strip and status line.
#[derive(Clone, Copy, Debug, Default)]
pub struct Chrome<'a> {
    pub title: &'a str,
    /// Color of the unsaved marker after the title, when shown.
    pub title_mark: Option<Color>,
    /// Right-aligned in the title strip; hidden when the title would reach it.
    pub app_name: &'a str,
    pub status_left: &'a str,
    pub status_right: &'a str,
    /// The status line shows a question awaiting an answer.
    pub prompt: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CaretStyle {
    #[default]
    Bar,
    /// Overtype mode.
    Underline,
    Hidden,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fill {
    pub rect: Rect,
    pub color: Color,
}

/// One character drawn with its cell's top-left corner at `(x, y)`; pixels
/// outside `clip` (its screen area) are not drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Glyph {
    pub x: i32,
    pub y: i32,
    pub ch: char,
    pub bold: bool,
    pub color: Color,
    pub clip: Rect,
}

#[derive(Clone, Debug, Default)]
pub struct DisplayList {
    pub fills: Vec<Fill>,
    pub glyphs: Vec<Glyph>,
    /// Filled after the glyphs (carets).
    pub overlays: Vec<Fill>,
    /// Drawn above everything else (palette, panels).
    pub popup: Layer,
    /// Drawn last: tooltips.
    pub tooltip: Layer,
}

impl DisplayList {
    pub fn clear(&mut self) {
        self.fills.clear();
        self.glyphs.clear();
        self.overlays.clear();
        self.popup.clear();
        self.tooltip.clear();
    }
}

/// Builds display lists, reusing its buffers between frames.
#[derive(Default)]
pub struct ViewBuilder {
    line: Vec<u8>,
    /// Visible bytes of a long row past its scan end.
    tail: Vec<u8>,
    /// Line-number text, reused between rows and frames.
    number: String,
}

/// Inputs of one frame.
pub struct Frame<'a> {
    pub snapshot: &'a Snapshot,
    pub layout: &'a Layout,
    pub scroll: Scroll,
    pub cell: Cell,
    pub theme: &'a Theme,
    pub chrome: Chrome<'a>,
    pub options: &'a Options,
    /// Sorted, non-overlapping selections.
    pub selections: &'a [Selection],
    pub caret: CaretStyle,
    pub highlights: Highlights<'a>,
    /// No document: the body shows only this hint, centered (no line
    /// numbers, rows, selections or caret).
    pub empty: Option<&'a str>,
    /// The scroll slider is being dragged.
    pub slider_active: bool,
}

/// Search decorations and syntax colors of a frame.
#[derive(Clone, Debug, Default)]
pub struct Highlights<'a> {
    /// Sorted, non-overlapping matches (at least the visible ones).
    pub matches: &'a [std::ops::Range<u64>],
    /// Find-in-selection range.
    pub scope: Option<std::ops::Range<u64>>,
    /// Sorted, non-overlapping syntax styles; other text has the text color.
    pub syntax: &'a [SyntaxSpan],
    /// Offsets of matching brackets (the pair at the caret).
    pub brackets: &'a [u64],
}

/// Bytes drawn in a syntax color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyntaxSpan {
    pub start: u64,
    pub end: u64,
    /// Index into [`Theme::syntax`].
    pub style: u8,
}

impl ViewBuilder {
    pub fn build(&mut self, frame: &Frame<'_>, out: &mut DisplayList) {
        out.clear();
        let Frame {
            snapshot,
            layout,
            scroll,
            cell,
            theme,
            chrome,
            options,
            selections: _,
            caret,
            highlights: _,
            empty,
            slider_active,
        } = *frame;
        let fill = |out: &mut DisplayList, rect: Rect, color| out.fills.push(Fill { rect, color });
        let status_background = if chrome.prompt {
            theme.prompt_background
        } else {
            theme.status_background
        };
        let status_text = if chrome.prompt {
            theme.prompt_text
        } else {
            theme.status_text
        };
        fill(out, layout.gutter, theme.background);
        fill(out, layout.text, theme.background);
        if layout.scrollbar.w > 0 {
            fill(out, layout.scrollbar, theme.background);
        }
        if empty.is_none() {
            let page = page_rows(layout, cell);
            let line_count = snapshot.line_count();
            if let Some(rect) = slider(layout, line_count, scroll.top_line, page, cell.height) {
                let color = if slider_active {
                    theme.scroll_slider_active
                } else {
                    theme.scroll_slider
                };
                fill(out, rect, color);
            }
        }
        fill(out, layout.title, theme.title_background);
        fill(out, layout.status, status_background);

        let title_y = layout.title.y + (layout.title.h.saturating_sub(cell.height) / 2) as i32;
        let title_cols = text_run(
            &mut out.glyphs,
            chrome.title,
            cell.width as i32,
            title_y,
            cell,
            theme.title_text,
            layout.title,
        );
        if let Some(color) = chrome.title_mark {
            let x = (cell.width * (title_cols + 2)) as i32;
            text_run(
                &mut out.glyphs,
                "\u{25CF}",
                x,
                title_y,
                cell,
                color,
                layout.title,
            );
        }
        // Title columns: margin, title, then a gap and the marker when shown.
        let title_end =
            cell.width * (1 + title_cols + if chrome.title_mark.is_some() { 2 } else { 0 });
        let name_w = chrome.app_name.width() as u32 * cell.width;
        let name_x = layout.title.w as i64 - i64::from(name_w + cell.width);
        // "│ Name ": the divider ends the area of the title.
        let divider_x = name_x - i64::from(2 * cell.width);
        if divider_x >= i64::from(title_end + cell.width) {
            text_run(
                &mut out.glyphs,
                "\u{2502}",
                divider_x as i32,
                title_y,
                cell,
                theme.line_number,
                layout.title,
            );
            text_run_bold(
                &mut out.glyphs,
                chrome.app_name,
                name_x as i32,
                title_y,
                cell,
                theme.unsaved,
                layout.title,
            );
        }
        let status_y = layout.status.y + (layout.status.h.saturating_sub(cell.height) / 2) as i32;
        text_run(
            &mut out.glyphs,
            chrome.status_left,
            cell.width as i32,
            status_y,
            cell,
            status_text,
            layout.status,
        );
        let right_w = chrome.status_right.chars().count() as i32 * cell.width as i32;
        let right_x = layout.status.w as i32 - right_w - cell.width as i32;
        text_run(
            &mut out.glyphs,
            chrome.status_right,
            right_x,
            status_y,
            cell,
            status_text,
            layout.status,
        );

        if let Some(hint) = empty {
            let text = layout.text;
            let cols = hint.width() as u32;
            let x = text.x + (text.w.saturating_sub(cols * cell.width) / 2) as i32;
            let y = text.y + (text.h.saturating_sub(cell.height) / 2) as i32;
            text_run(&mut out.glyphs, hint, x, y, cell, theme.popup_dim, text);
            return;
        }

        for row in 0..layout.rows {
            let line = scroll.top_line + u64::from(row);
            let Some(range) = snapshot.line_range(line) else {
                break;
            };
            let y = layout.text.y + (row * cell.height) as i32;
            if options.line_numbers {
                let number = &mut self.number;
                number.clear();
                let _ = write!(number, "{}", line + 1);
                let right = layout.gutter.x + layout.gutter.w as i32 - cell.width as i32;
                let x = right - number.len() as i32 * cell.width as i32;
                text_run(
                    &mut out.glyphs,
                    number,
                    x,
                    y,
                    cell,
                    theme.line_number,
                    layout.gutter,
                );
            }

            let end = range.end.min(range.start + ROW_SCAN_LIMIT as u64);
            // Rows inside one leaf are borrowed; others are joined into the reused buffer.
            let mut chunks = snapshot.chunks(range.start..end);
            let first = chunks.next().unwrap_or(&[]);
            let mut bytes: &[u8] = match chunks.next() {
                None => first,
                Some(second) => {
                    self.line.clear();
                    self.line.extend_from_slice(first);
                    self.line.extend_from_slice(second);
                    for chunk in chunks {
                        self.line.extend_from_slice(chunk);
                    }
                    &self.line
                }
            };
            // CRLF displays as one line break; a CR not followed by LF stays visible.
            let crlf = range.end < snapshot.len();
            let long = end < range.end;
            let mut content_end = range.end;
            if long {
                content_end = content_range(snapshot, line).end;
            } else if crlf && bytes.last() == Some(&b'\r') {
                bytes = &bytes[..bytes.len() - 1];
                content_end -= 1;
            }
            let row_rect = Rect {
                x: layout.text.x,
                y,
                w: layout.text.w,
                h: cell.height,
            };
            let content = range.start..content_end;
            let row_ranges = RowRanges {
                row: row_rect,
                bytes,
                content: content.clone(),
                has_break: crlf,
            };
            if let Some(scope) = &frame.highlights.scope {
                row_ranges.fill(out, frame, std::slice::from_ref(scope), theme.search_range);
            }
            selection_row(out, frame, row_rect, bytes, content.clone(), crlf);
            row_ranges.matches(out, frame);
            for &at in frame.highlights.brackets {
                if (content.start..content.end).contains(&at)
                    && let Some(rect) = row_ranges.rect(frame, &(at..at + 1))
                {
                    fill(out, rect, theme.bracket_match);
                }
            }
            let text_x = layout.text.x;
            let clip = layout.text;
            let syntax = frame.highlights.syntax;
            // Spans ending inside or after the row, advanced as offsets grow.
            let mut span = syntax.partition_point(|s| s.end <= range.start);
            let mut emit = |col: u32, ch, kind, offset: usize| {
                let color = match kind {
                    Kind::Text => {
                        let at = range.start + offset as u64;
                        while span < syntax.len() && syntax[span].end <= at {
                            span += 1;
                        }
                        match syntax.get(span) {
                            Some(s) if s.start <= at => theme.syntax[s.style as usize],
                            _ => theme.text,
                        }
                    }
                    Kind::Control => theme.control,
                };
                out.glyphs.push(Glyph {
                    x: text_x + (col * cell.width) as i32,
                    y,
                    ch,
                    bold: false,
                    color,
                    clip,
                });
            };
            // A long line is drawn exactly up to its scan end, then one
            // column per byte (the rule of carets, clicks and selections).
            let seam = if long { scan_end(bytes) } else { bytes.len() };
            let cols = layout.cols + 1;
            let reached = layout_line(
                &bytes[..seam],
                scroll.first_col,
                cols,
                options.tab_width,
                &mut emit,
            );
            if long
                && let Some(LineEnd {
                    col: seam_col,
                    last,
                }) = reached
            {
                let end_col = scroll.first_col + u64::from(cols);
                if end_col > seam_col {
                    let base = range.start + seam as u64;
                    let from = base + scroll.first_col.saturating_sub(seam_col).saturating_sub(3);
                    let to = (base + (end_col - seam_col) + 3).min(content_end);
                    if from < to {
                        self.tail.clear();
                        for chunk in snapshot.chunks(from..to) {
                            self.tail.extend_from_slice(chunk);
                        }
                        layout_beyond(
                            &self.tail,
                            (from - range.start) as usize,
                            (seam, seam_col),
                            scroll.first_col,
                            cols,
                            last,
                            &mut emit,
                        );
                    }
                }
            }
            if caret != CaretStyle::Hidden {
                caret_row(out, frame, row_rect, bytes, content);
            }
        }
    }
}

/// One visible row for drawing range fills.
struct RowRanges<'b> {
    row: Rect,
    bytes: &'b [u8],
    content: std::ops::Range<u64>,
    has_break: bool,
}

impl RowRanges<'_> {
    /// Screen rectangle of `range` on this row, if visible.
    fn rect(&self, frame: &Frame<'_>, range: &std::ops::Range<u64>) -> Option<Rect> {
        let content = &self.content;
        if range.start > content.end || range.end < content.start || range.is_empty() {
            return None;
        }
        let tab = frame.options.tab_width;
        let column = |offset: u64| column_of(self.bytes, (offset - content.start) as usize, tab);
        let start = column(range.start.max(content.start));
        let end = column(range.end.min(content.end));
        self.rect_cols(frame, range, start, end)
    }

    /// Rectangle of `range` given the columns of its visible part.
    fn rect_cols(
        &self,
        frame: &Frame<'_>,
        range: &std::ops::Range<u64>,
        start: u64,
        mut end: u64,
    ) -> Option<Rect> {
        if range.end > self.content.end && self.has_break {
            end += 1;
        }
        let first_col = frame.scroll.first_col;
        if end <= first_col || end <= start {
            return None;
        }
        let max = u64::from(frame.layout.cols) + 2;
        let s = start.saturating_sub(first_col).min(max) as u32;
        let e = (end - first_col).min(max) as u32;
        let cell_w = frame.cell.width;
        let rect = Rect {
            x: self.row.x + (s * cell_w) as i32,
            y: self.row.y,
            w: (e - s) * cell_w,
            h: self.row.h,
        };
        rect.intersect(&frame.layout.text)
    }

    /// Fills the parts of sorted `ranges` on this row.
    fn fill(
        &self,
        out: &mut DisplayList,
        frame: &Frame<'_>,
        ranges: &[std::ops::Range<u64>],
        color: Color,
    ) {
        let first = ranges.partition_point(|r| r.end < self.content.start);
        for range in &ranges[first..] {
            if range.start > self.content.end {
                break;
            }
            if let Some(rect) = self.rect(frame, range) {
                out.fills.push(Fill { rect, color });
            }
        }
    }

    /// Match fills; a match equal to a selection is the current one and
    /// gets a border. Columns are computed in one pass over the row.
    fn matches(&self, out: &mut DisplayList, frame: &Frame<'_>) {
        let theme = frame.theme;
        let matches = frame.highlights.matches;
        let content = &self.content;
        let tab = frame.options.tab_width;
        let last_col = frame.scroll.first_col + u64::from(frame.layout.cols) + 2;
        let mut at = (0usize, 0u64);
        let mut column = |offset: u64| {
            let offset = (offset - content.start) as usize;
            if offset < at.0 {
                at = (0, 0);
            }
            at = (offset, column_after(self.bytes, at, offset, tab));
            at.1
        };
        let first = matches.partition_point(|r| r.end < content.start);
        for range in &matches[first..] {
            if range.start > content.end {
                break;
            }
            if range.end < content.start || range.is_empty() {
                continue;
            }
            let start = column(range.start.max(content.start));
            if start > last_col {
                break;
            }
            let end = column(range.end.min(content.end));
            let Some(rect) = self.rect_cols(frame, range, start, end) else {
                continue;
            };
            let current = frame
                .selections
                .binary_search_by_key(&(range.start, range.end), |s| (s.start(), s.end()))
                .is_ok();
            if !current {
                out.fills.push(Fill {
                    rect,
                    color: theme.search_match,
                });
                continue;
            }
            let border = theme.search_current_border;
            out.fills.push(Fill {
                rect,
                color: border,
            });
            let inner = Rect {
                x: rect.x + 1,
                y: rect.y + 1,
                w: rect.w.saturating_sub(2),
                h: rect.h.saturating_sub(2),
            };
            out.fills.push(Fill {
                rect: inner,
                color: theme.search_current,
            });
        }
    }
}

/// Selection fills of one row (`bytes`: its scanned content); a selected
/// line break adds one cell.
fn selection_row(
    out: &mut DisplayList,
    frame: &Frame<'_>,
    row: Rect,
    bytes: &[u8],
    content: std::ops::Range<u64>,
    has_break: bool,
) {
    let sels = frame.selections;
    let first = sels.partition_point(|s| s.end() < content.start);
    let cell_w = frame.cell.width;
    let tab = frame.options.tab_width;
    let column = |offset: u64| column_of(bytes, (offset - content.start) as usize, tab);
    for sel in &sels[first..] {
        if sel.start() > content.end {
            break;
        }
        if sel.is_empty() {
            continue;
        }
        let start = column(sel.start().max(content.start));
        let mut end = column(sel.end().min(content.end));
        if sel.end() > content.end && has_break {
            end += 1;
        }
        let first_col = frame.scroll.first_col;
        if end <= first_col || end <= start {
            continue;
        }
        let max = u64::from(frame.layout.cols) + 2;
        let s = start.saturating_sub(first_col).min(max) as u32;
        let e = (end - first_col).min(max) as u32;
        let rect = Rect {
            x: row.x + (s * cell_w) as i32,
            y: row.y,
            w: (e - s) * cell_w,
            h: row.h,
        };
        if let Some(rect) = rect.intersect(&frame.layout.text) {
            out.fills.push(Fill {
                rect,
                color: frame.theme.selection,
            });
        }
    }
}

fn caret_row(
    out: &mut DisplayList,
    frame: &Frame<'_>,
    row: Rect,
    bytes: &[u8],
    content: std::ops::Range<u64>,
) {
    let sels = frame.selections;
    let cell = frame.cell;
    let thickness = (cell.height / 10).max(1);
    let first = sels.partition_point(|s| s.end() < content.start);
    for sel in &sels[first..] {
        if sel.start() > content.end {
            break;
        }
        if sel.head < content.start || sel.head > content.end {
            continue;
        }
        let col = column_of(
            bytes,
            (sel.head - content.start) as usize,
            frame.options.tab_width,
        );
        let Some(visible) = col.checked_sub(frame.scroll.first_col) else {
            continue;
        };
        if visible > u64::from(frame.layout.cols) + 1 {
            continue;
        }
        let x = row.x + (visible as u32 * cell.width) as i32;
        let rect = match frame.caret {
            CaretStyle::Underline => Rect {
                x,
                y: row.y + (cell.height - thickness) as i32,
                w: cell.width,
                h: thickness,
            },
            _ => Rect {
                x,
                y: row.y,
                w: thickness,
                h: cell.height,
            },
        };
        if let Some(rect) = rect.intersect(&frame.layout.text) {
            out.overlays.push(Fill {
                rect,
                color: frame.theme.caret,
            });
        }
    }
}

/// Byte offset under the point `(x, y)`: the nearest character boundary of
/// the row's line. Rows outside the viewport hit the lines there; above the
/// first line hits 0, below the last hits the end.
pub fn hit_test(
    snapshot: &Snapshot,
    layout: &Layout,
    scroll: Scroll,
    cell: Cell,
    options: &Options,
    x: i32,
    y: i32,
) -> u64 {
    let row = (y - layout.text.y).div_euclid(cell.height.max(1) as i32);
    let line = scroll.top_line as i64 + i64::from(row);
    if line < 0 {
        return 0;
    }
    if line as u64 > snapshot.newlines() {
        return snapshot.len();
    }
    let line = line as u64;
    let dx = (x - layout.text.x).max(0) as u64;
    let col = scroll.first_col + (dx + u64::from(cell.width) / 2) / u64::from(cell.width.max(1));
    offset_in_line(
        snapshot,
        content_range(snapshot, line),
        col,
        options.tab_width,
    )
}

/// Draws `text` from `(x, y)` and returns the cells it occupies.
fn text_run(
    out: &mut Vec<Glyph>,
    text: &str,
    x: i32,
    y: i32,
    cell: Cell,
    color: Color,
    clip: Rect,
) -> u32 {
    let mut col = 0;
    for ch in text.chars() {
        let width = ch.width().unwrap_or(1) as u32;
        let gx = x + (col * cell.width) as i32;
        if gx >= clip.x
            && gx + (width * cell.width) as i32 <= clip.x + clip.w as i32
            && !ch.is_whitespace()
        {
            out.push(Glyph {
                x: gx,
                y,
                ch,
                bold: false,
                color,
                clip,
            });
        }
        col += width;
    }
    col
}

/// [`text_run`] in bold type.
fn text_run_bold(
    out: &mut Vec<Glyph>,
    text: &str,
    x: i32,
    y: i32,
    cell: Cell,
    color: Color,
    clip: Rect,
) -> u32 {
    let first = out.len();
    let cols = text_run(out, text, x, y, cell, color, clip);
    for glyph in &mut out[first..] {
        glyph.bold = true;
    }
    cols
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Text,
    /// Control character or invalid byte, drawn as a symbol.
    Control,
}

/// Where [`layout_line`] ended after all of its bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineEnd {
    /// Column after the last byte.
    pub col: u64,
    /// Visible column of the last drawn character and the marks drawn on
    /// it; zero-width marks that follow join that cell.
    pub last: Option<(u32, u32)>,
}

/// Lays out one line, calling `emit(visible_col, char, kind, offset)` for
/// every character visible in `[first_col, first_col + cols)`; `offset` is
/// the character's byte offset in `bytes`. Returns where it ended when
/// every byte was laid out (`None` when the visible columns ended first).
pub fn layout_line(
    bytes: &[u8],
    first_col: u64,
    cols: u32,
    tab_width: u32,
    mut emit: impl FnMut(u32, char, Kind, usize),
) -> Option<LineEnd> {
    let end_col = first_col + u64::from(cols);
    let tab = u64::from(tab_width.max(1));
    let mut col = 0u64;
    // Visible column of the previous character and the marks drawn on it.
    let mut last_visible: Option<(u32, u32)> = None;
    let mut place = |col: &mut u64,
                     last: &mut Option<(u32, u32)>,
                     ch: char,
                     width: u64,
                     kind: Kind,
                     offset: usize| {
        if width == 0 {
            if let Some((v, marks)) = last
                && *marks < MAX_MARKS_PER_CELL
            {
                *marks += 1;
                emit(*v, ch, kind, offset);
            }
            return;
        }
        *last = None;
        if *col >= first_col && *col + width <= end_col {
            let v = (*col - first_col) as u32;
            emit(v, ch, kind, offset);
            *last = Some((v, 0));
        }
        *col += width;
    };
    let mut offset = 0;
    for chunk in bytes.utf8_chunks() {
        for ch in chunk.valid().chars() {
            if col >= end_col {
                return None;
            }
            let at = offset;
            offset += ch.len_utf8();
            let last = &mut last_visible;
            match ch {
                '\t' => {
                    col = (col / tab + 1) * tab;
                    *last = None;
                }
                '\0'..='\u{1F}' => {
                    let symbol = char::from_u32(0x2400 + ch as u32).unwrap();
                    place(&mut col, last, symbol, 1, Kind::Control, at);
                }
                '\u{7F}' => place(&mut col, last, '\u{2421}', 1, Kind::Control, at),
                _ => place(
                    &mut col,
                    last,
                    ch,
                    ch.width().unwrap_or(1) as u64,
                    Kind::Text,
                    at,
                ),
            }
        }
        for _ in chunk.invalid() {
            if col >= end_col {
                return None;
            }
            let at = offset;
            offset += 1;
            place(
                &mut col,
                &mut last_visible,
                '\u{FFFD}',
                1,
                Kind::Control,
                at,
            );
        }
    }
    Some(LineEnd {
        col,
        last: last_visible,
    })
}

/// Lays out bytes of a long line past its scan end (`seam`, exact column
/// `seam_col`), where each byte takes one column: a character is drawn once
/// at the column of its first byte. `window` holds the line's bytes from
/// offset `start` (at or after `seam`); `emit` gets offsets from the line
/// start. Characters starting before `first_col` are skipped, not shifted;
/// zero-width marks join the previous drawn cell, which is `last` (from
/// [`LineEnd`]) for marks right after the seam.
pub fn layout_beyond(
    window: &[u8],
    start: usize,
    seam: (usize, u64),
    first_col: u64,
    cols: u32,
    last: Option<(u32, u32)>,
    mut emit: impl FnMut(u32, char, Kind, usize),
) {
    let end_col = first_col + u64::from(cols);
    // The cell before the seam only takes marks that start right at it.
    let mut last = if start == seam.0 { last } else { None };
    let mut offset = start;
    for chunk in window.utf8_chunks() {
        let valid = chunk.valid().chars().map(|ch| (ch, ch.len_utf8()));
        let invalid = chunk.invalid().iter().map(|_| ('\u{FFFD}', 1));
        for (ch, len) in valid.chain(invalid) {
            let at = offset;
            offset += len;
            let col = seam.1 + (at - seam.0) as u64;
            if col >= end_col {
                return;
            }
            let (ch, kind) = match ch {
                '\t' => {
                    last = None;
                    continue;
                }
                '\0'..='\u{1F}' => (char::from_u32(0x2400 + ch as u32).unwrap(), Kind::Control),
                '\u{7F}' => ('\u{2421}', Kind::Control),
                '\u{FFFD}' if len == 1 => (ch, Kind::Control),
                _ => (ch, Kind::Text),
            };
            if kind == Kind::Text && ch.width() == Some(0) {
                if let Some((v, marks)) = &mut last
                    && *marks < MAX_MARKS_PER_CELL
                {
                    *marks += 1;
                    emit(*v, ch, kind, at);
                }
                continue;
            }
            last = None;
            if col >= first_col {
                let v = (col - first_col) as u32;
                emit(v, ch, kind, at);
                last = Some((v, 0));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cells(bytes: &[u8], first_col: u64, cols: u32) -> Vec<(u32, char, Kind)> {
        let mut out = Vec::new();
        layout_line(bytes, first_col, cols, 4, |c, ch, k, _| {
            out.push((c, ch, k))
        });
        out
    }

    #[test]
    fn ascii_and_tabs() {
        let got = cells(b"a\tb", 0, 10);
        assert_eq!(got, vec![(0, 'a', Kind::Text), (4, 'b', Kind::Text)]);
        let got = cells(b"ab\tc", 0, 10);
        assert_eq!(
            got,
            vec![
                (0, 'a', Kind::Text),
                (1, 'b', Kind::Text),
                (4, 'c', Kind::Text)
            ]
        );
    }

    #[test]
    fn horizontal_scroll_and_clip() {
        let got = cells(b"abcdef", 2, 3);
        assert_eq!(
            got,
            vec![
                (0, 'c', Kind::Text),
                (1, 'd', Kind::Text),
                (2, 'e', Kind::Text)
            ]
        );
    }

    #[test]
    fn wide_combining_control_and_invalid() {
        let got = cells("中e\u{301}\u{1}".as_bytes(), 0, 10);
        assert_eq!(
            got,
            vec![
                (0, '中', Kind::Text),
                (2, 'e', Kind::Text),
                (2, '\u{301}', Kind::Text),
                (3, '\u{2401}', Kind::Control),
            ]
        );
        let got = cells(b"a\xFFb", 0, 10);
        assert_eq!(
            got,
            vec![
                (0, 'a', Kind::Text),
                (1, '\u{FFFD}', Kind::Control),
                (2, 'b', Kind::Text)
            ]
        );
    }

    #[test]
    fn wide_char_cut_by_left_edge_is_hidden() {
        let got = cells("中a".as_bytes(), 1, 5);
        assert_eq!(got, vec![(1, 'a', Kind::Text)]);
    }

    #[test]
    fn layout_areas() {
        let cell = Cell {
            width: 10,
            height: 20,
        };
        let l = Layout::new(800, 600, cell, 12345, &Options::default());
        assert_eq!(l.title.h, 30);
        assert_eq!(l.status.y, 570);
        assert_eq!(l.gutter.w, 70);
        assert_eq!(l.text.x, 70);
        assert_eq!(l.text.h, 540);
        assert_eq!(l.rows, 27);
        assert_eq!(l.cols, 73);
    }

    fn build(bytes: &[u8], width: u32, height: u32) -> (Layout, DisplayList) {
        build_with(bytes, width, height, &[], CaretStyle::Hidden)
    }

    fn build_with(
        bytes: &[u8],
        width: u32,
        height: u32,
        selections: &[Selection],
        caret: CaretStyle,
    ) -> (Layout, DisplayList) {
        build_full(
            bytes,
            width,
            height,
            selections,
            caret,
            Highlights::default(),
        )
    }

    fn build_full(
        bytes: &[u8],
        width: u32,
        height: u32,
        selections: &[Selection],
        caret: CaretStyle,
        highlights: Highlights<'_>,
    ) -> (Layout, DisplayList) {
        let snapshot = Snapshot::from_bytes(bytes);
        let cell = Cell {
            width: 10,
            height: 20,
        };
        let options = Options::default();
        let layout = Layout::new(width, height, cell, snapshot.line_count(), &options);
        let mut list = DisplayList::default();
        ViewBuilder::default().build(
            &Frame {
                snapshot: &snapshot,
                layout: &layout,
                scroll: Scroll::default(),
                cell,
                theme: &Theme::DARK,
                chrome: Chrome {
                    title: "title",
                    title_mark: Some(Theme::DARK.unsaved),
                    app_name: "App",
                    status_left: "left",
                    status_right: "right",
                    prompt: false,
                },
                options: &options,
                selections,
                caret,
                highlights,
                empty: None,
                slider_active: false,
            },
            &mut list,
        );
        (layout, list)
    }

    fn body_text(list: &DisplayList, layout: &Layout) -> String {
        let clip = layout.text;
        list.glyphs
            .iter()
            .filter(|g| g.clip == clip)
            .map(|g| g.ch)
            .collect()
    }

    #[test]
    fn empty_frames_show_only_the_hint_inside_the_text_area() {
        let cell = Cell {
            width: 10,
            height: 20,
        };
        let options = Options::default();
        for (width, height) in [(400, 200), (120, 70), (30, 50), (0, 0)] {
            let snapshot = Snapshot::empty();
            let layout = Layout::new(width, height, cell, 1, &options);
            let mut list = DisplayList::default();
            let sels = [Selection::caret(0)];
            ViewBuilder::default().build(
                &Frame {
                    snapshot: &snapshot,
                    layout: &layout,
                    scroll: Scroll::default(),
                    cell,
                    theme: &Theme::DARK,
                    chrome: Chrome::default(),
                    options: &options,
                    selections: &sels,
                    caret: CaretStyle::Bar,
                    highlights: Highlights::default(),
                    empty: Some("Drop a file"),
                    slider_active: false,
                },
                &mut list,
            );
            assert!(list.overlays.is_empty(), "no caret at {width}x{height}");
            let body: Vec<&Glyph> = list
                .glyphs
                .iter()
                .filter(|g| g.clip != layout.title && g.clip != layout.status)
                .collect();
            assert!(body.iter().all(|g| g.clip == layout.text));
            if width == 400 {
                let text: String = body.iter().map(|g| g.ch).collect();
                assert_eq!(text, "Dropafile", "no line number, only the hint");
                assert!(body.iter().all(|g| layout.text.contains(g.x, g.y)));
            }
        }
    }

    #[test]
    fn syntax_spans_color_their_characters() {
        let spans = [
            SyntaxSpan {
                start: 0,
                end: 2,
                style: 0,
            },
            SyntaxSpan {
                start: 7,
                end: 10,
                style: 1,
            },
        ];
        let highlights = Highlights {
            syntax: &spans,
            ..Highlights::default()
        };
        let (layout, list) = build_full(
            "fn é\n\t\"x\" y".as_bytes(),
            400,
            200,
            &[],
            CaretStyle::Hidden,
            highlights,
        );
        let theme = Theme::DARK;
        let colors: Vec<(char, Color)> = list
            .glyphs
            .iter()
            .filter(|g| g.clip == layout.text)
            .map(|g| (g.ch, g.color))
            .collect();
        assert_eq!(
            colors,
            [
                ('f', theme.syntax[0]),
                ('n', theme.syntax[0]),
                (' ', theme.text),
                ('é', theme.text),
                ('"', theme.syntax[1]),
                ('x', theme.syntax[1]),
                ('"', theme.syntax[1]),
                (' ', theme.text),
                ('y', theme.text),
            ]
        );
    }

    #[test]
    fn the_slider_follows_the_view() {
        let cell = Cell {
            width: 10,
            height: 20,
        };
        let options = Options {
            scroll_strip: 6,
            ..Options::default()
        };
        let layout = Layout::new(400, 300, cell, 1000, &options);
        let plain = Layout::new(400, 300, cell, 1000, &Options::default());
        assert_eq!(layout.text.w + 6, plain.text.w);
        assert_eq!(layout.scrollbar.x, layout.text.x + layout.text.w as i32);
        let track = layout.scrollbar;
        let page = page_rows(&layout, cell);
        assert_eq!(slider(&layout, page, 0, page, 20), None, "the file fits");
        assert!(slider(&layout, page, 1, page, 20).is_some(), "scrolled");
        let top = slider(&layout, 1000, 0, page, 20).unwrap();
        assert_eq!((top.y, top.w), (track.y, 6));
        let bottom = slider(&layout, 1000, 999, page, 20).unwrap();
        assert_eq!(bottom.y + bottom.h as i32, track.y + track.h as i32);
        let middle = slider(&layout, 1000, 500, page, 20).unwrap();
        assert!(top.y < middle.y && middle.y < bottom.y);
        let huge = slider(&layout, 10_000_000, 5_000_000, page, 20).unwrap();
        assert_eq!(huge.h, 20, "minimum height");
        assert_eq!(slider(&plain, 1000, 0, page, 20), None, "no strip");

        let snapshot = Snapshot::from_bytes("x\n".repeat(999).as_bytes());
        for (active, color) in [
            (false, Theme::DARK.scroll_slider),
            (true, Theme::DARK.scroll_slider_active),
        ] {
            let mut list = DisplayList::default();
            ViewBuilder::default().build(
                &Frame {
                    snapshot: &snapshot,
                    layout: &layout,
                    scroll: Scroll::default(),
                    cell,
                    theme: &Theme::DARK,
                    chrome: Chrome::default(),
                    options: &options,
                    selections: &[],
                    caret: CaretStyle::Hidden,
                    highlights: Highlights::default(),
                    empty: None,
                    slider_active: active,
                },
                &mut list,
            );
            assert!(list.fills.contains(&Fill { rect: top, color }));
        }
    }

    #[test]
    fn full_screen_layouts_have_no_title_strip() {
        let cell = Cell {
            width: 10,
            height: 20,
        };
        let options = Options {
            title_strip: false,
            ..Options::default()
        };
        let layout = Layout::new(400, 300, cell, 10, &options);
        assert_eq!((layout.title.h, layout.text.y), (0, 0));
        assert_eq!(layout.status.y + layout.status.h as i32, 300);
    }

    #[test]
    fn builds_rows_with_line_numbers_and_crlf() {
        let (layout, list) = build(b"ab\r\ncd\n", 400, 200);
        let numbers: String = list
            .glyphs
            .iter()
            .filter(|g| g.clip == layout.gutter)
            .map(|g| g.ch)
            .collect();
        assert_eq!(numbers, "123");
        // The '\r' of the CRLF is not shown.
        assert_eq!(body_text(&list, &layout), "abcd");
        assert_eq!(list.fills.len(), 4);
    }

    #[test]
    fn carriage_return_without_line_feed_stays_visible() {
        let (layout, list) = build(b"a\r", 400, 200);
        assert_eq!(body_text(&list, &layout), "a\u{240D}");
        let (layout, list) = build(b"a\r\r\nb", 400, 200);
        assert_eq!(body_text(&list, &layout), "a\u{240D}b");
    }

    #[test]
    fn every_glyph_is_clipped_to_its_region() {
        for height in [0, 1, 5, 19, 20, 30, 50, 60, 61, 200] {
            for width in [0, 1, 9, 10, 400] {
                let (layout, list) = build(b"first line\nsecond\n", width, height);
                for g in &list.glyphs {
                    assert!(
                        [layout.title, layout.status, layout.gutter, layout.text].contains(&g.clip),
                        "glyph {g:?} has an unknown clip at {width}x{height}"
                    );
                }
            }
        }
    }

    #[test]
    fn zero_width_marks_per_cell_are_capped() {
        let mut line = b"a".to_vec();
        for _ in 0..10_000 {
            line.extend_from_slice("\u{301}".as_bytes());
        }
        line.push(b'b');
        let got = cells(&line, 0, 10);
        assert_eq!(got.len(), 2 + MAX_MARKS_PER_CELL as usize);
        assert_eq!(got.last(), Some(&(1, 'b', Kind::Text)));
    }

    #[test]
    fn selection_fills_include_selected_line_breaks() {
        let sels = [Selection::new(1, 5)];
        let (layout, list) = build_with(b"abc\ndef", 400, 200, &sels, CaretStyle::Hidden);
        let selected: Vec<Rect> = list
            .fills
            .iter()
            .filter(|f| f.color == Theme::DARK.selection)
            .map(|f| f.rect)
            .collect();
        let (x, y) = (layout.text.x, layout.text.y);
        assert_eq!(
            selected,
            vec![
                Rect {
                    x: x + 10,
                    y,
                    w: 30,
                    h: 20
                },
                Rect {
                    x,
                    y: y + 20,
                    w: 10,
                    h: 20
                },
            ]
        );
        assert!(list.overlays.is_empty());
    }

    #[test]
    fn matches_scope_and_the_current_match_are_filled() {
        let matches = [0..2, 3..5];
        let highlights = Highlights {
            matches: &matches,
            scope: Some(0..7),
            ..Highlights::default()
        };
        let sels = [Selection::new(3, 5)];
        let (layout, list) = build_full(
            b"ab\nab\nab",
            400,
            200,
            &sels,
            CaretStyle::Hidden,
            highlights,
        );
        let t = Theme::DARK;
        let rects = |color| -> Vec<Rect> {
            list.fills
                .iter()
                .filter(|f| f.color == color)
                .map(|f| f.rect)
                .collect()
        };
        let (x, y) = (layout.text.x, layout.text.y);
        assert_eq!(rects(t.search_match), vec![Rect { x, y, w: 20, h: 20 }]);
        assert_eq!(rects(t.search_current_border).len(), 1);
        assert_eq!(
            rects(t.search_current),
            vec![Rect {
                x: x + 1,
                y: y + 21,
                w: 18,
                h: 18
            }]
        );
        assert_eq!(
            rects(t.search_range).len(),
            3,
            "rows 0, 1 and the start of 2"
        );
    }

    #[test]
    fn dense_matches_on_a_long_row_draw_only_visible_columns() {
        let text = "a\tb".repeat(ROW_SCAN_LIMIT / 3);
        let matches: Vec<_> = (0..text.len() as u64).map(|i| i..i + 1).collect();
        let highlights = Highlights {
            matches: &matches,
            scope: None,
            ..Highlights::default()
        };
        let started = std::time::Instant::now();
        let (layout, list) = build_full(
            text.as_bytes(),
            400,
            200,
            &[],
            CaretStyle::Hidden,
            highlights,
        );
        assert!(started.elapsed() < std::time::Duration::from_millis(200));
        let fills: Vec<Rect> = list
            .fills
            .iter()
            .filter(|f| f.color == Theme::DARK.search_match)
            .map(|f| f.rect)
            .collect();
        let cols = layout.text.w / 10;
        assert!(fills.len() as u32 <= cols + 2, "{}", fills.len());
        // Tab stops follow the incremental columns: `a` 0, tab 1-3, `b` 4.
        let x = layout.text.x;
        assert_eq!(
            fills[1],
            Rect {
                x: x + 10,
                y: layout.text.y,
                w: 30,
                h: 20
            }
        );
        assert_eq!(fills[2].x, x + 40);
    }

    #[test]
    fn carets_are_overlays_at_their_column() {
        let sels = [Selection::caret(2), Selection::caret(4)];
        let (layout, list) = build_with(b"a\tb\ncd", 400, 200, &sels, CaretStyle::Bar);
        let (x, y) = (layout.text.x, layout.text.y);
        assert_eq!(
            list.overlays,
            vec![
                Fill {
                    rect: Rect {
                        x: x + 40,
                        y,
                        w: 2,
                        h: 20
                    },
                    color: Theme::DARK.caret
                },
                Fill {
                    rect: Rect {
                        x,
                        y: y + 20,
                        w: 2,
                        h: 20
                    },
                    color: Theme::DARK.caret
                },
            ]
        );
        let (_, list) = build_with(b"ab", 400, 200, &sels[..1], CaretStyle::Underline);
        assert_eq!(list.overlays[0].rect.w, 10);
        assert_eq!(list.overlays[0].rect.h, 2);
    }

    #[test]
    fn title_mark_follows_the_title() {
        let (layout, list) = build(b"", 400, 200);
        let mark = list.glyphs.iter().find(|g| g.ch == '\u{25CF}').unwrap();
        assert_eq!(mark.color, Theme::DARK.unsaved);
        assert_eq!(mark.x, 70);
        assert_eq!(mark.clip, layout.title);
    }

    #[test]
    fn app_name_is_right_aligned_and_yields_to_the_title() {
        let name = |width| {
            let (layout, list) = build(b"", width, 200);
            let glyphs: Vec<&Glyph> = list
                .glyphs
                .iter()
                .filter(|g| g.clip == layout.title && "\u{2502}App".contains(g.ch))
                .collect();
            (
                glyphs.iter().map(|g| g.ch).collect::<String>(),
                glyphs.first().map(|g| g.x),
                glyphs
                    .iter()
                    .find(|g| g.ch == 'A')
                    .map(|g| (g.color, g.bold)),
                glyphs.iter().find(|g| g.ch == '\u{2502}').map(|g| g.bold),
            )
        };
        // 400 px: "│ App" ends one cell before the right edge.
        assert_eq!(
            name(400),
            (
                "\u{2502}App".to_owned(),
                Some(400 - 60),
                Some((Theme::DARK.unsaved, true)),
                Some(false),
            )
        );
        // "title" and its marker end at 80 px; the divider needs a 1-cell gap.
        assert_eq!(
            name(150),
            (
                "\u{2502}App".to_owned(),
                Some(90),
                Some((Theme::DARK.unsaved, true)),
                Some(false),
            )
        );
        assert_eq!(name(149), (String::new(), None, None, None));
    }

    #[test]
    fn hit_test_matches_drawn_columns() {
        let text = "x\na\t\u{4E2D}e\u{301}z\u{1}\r\nlast";
        let snapshot = Snapshot::from_bytes(text.as_bytes());
        let cell = Cell {
            width: 10,
            height: 20,
        };
        let options = Options::default();
        let layout = Layout::new(400, 200, cell, snapshot.line_count(), &options);
        let hit = |col: i32, row: i32| {
            hit_test(
                &snapshot,
                &layout,
                Scroll::default(),
                cell,
                &options,
                layout.text.x + col * 10,
                layout.text.y + row * 20 + 5,
            )
        };
        // Each drawn base glyph starts at the offset hit at its column.
        let line = &text.as_bytes()[2..12];
        let mut drawn = Vec::new();
        layout_line(line, 0, 40, 4, |c, ch, _, _| drawn.push((c, ch)));
        let starts = [0u64, 2, 5, 8, 9];
        let bases: Vec<u32> = drawn
            .iter()
            .filter(|(_, ch)| *ch != '\u{301}')
            .map(|(c, _)| *c)
            .collect();
        assert_eq!(bases, vec![0, 4, 6, 7, 8]);
        for (col, start) in bases.iter().zip(starts) {
            assert_eq!(hit(*col as i32, 1), 2 + start, "column {col}");
        }
        // Inside a tab or wide glyph: nearest boundary; past the end: line end before CRLF.
        assert_eq!(hit(2, 1), 3);
        assert_eq!(hit(3, 1), 4);
        assert_eq!(hit(30, 1), 12);
        assert_eq!(hit(-3, -2), 0);
        assert_eq!(hit(2, 9), snapshot.len());
    }

    #[test]
    fn hits_beyond_the_scan_limit_land_on_character_boundaries() {
        let mut text = vec![b'a'; ROW_SCAN_LIMIT];
        for _ in 0..100 {
            text.extend_from_slice("\u{4E2D}".as_bytes());
        }
        let snapshot = Snapshot::from_bytes(&text);
        let cell = Cell {
            width: 10,
            height: 20,
        };
        let options = Options::default();
        let layout = Layout::new(400, 200, cell, 1, &options);
        let scroll = Scroll {
            top_line: 0,
            first_col: ROW_SCAN_LIMIT as u64 - 6,
        };
        for col in 0..30 {
            let x = layout.text.x + col * 10;
            let pos = hit_test(
                &snapshot,
                &layout,
                scroll,
                cell,
                &options,
                x,
                layout.text.y + 5,
            );
            assert!(
                std::str::from_utf8(&snapshot.to_vec(0..pos)).is_ok(),
                "column {col} hit {pos}"
            );
        }
    }

    /// Body glyphs `(visible column, char)` and caret columns of one row
    /// scrolled to `first_col`.
    fn long_row(bytes: &[u8], first_col: u64, carets: &[u64]) -> (Vec<(i32, char)>, Vec<i32>) {
        let snapshot = Snapshot::from_bytes(bytes);
        let cell = Cell {
            width: 10,
            height: 20,
        };
        let options = Options::default();
        let layout = Layout::new(400, 200, cell, snapshot.line_count(), &options);
        let sels: Vec<Selection> = carets.iter().map(|&c| Selection::caret(c)).collect();
        let mut list = DisplayList::default();
        ViewBuilder::default().build(
            &Frame {
                snapshot: &snapshot,
                layout: &layout,
                scroll: Scroll {
                    top_line: 0,
                    first_col,
                },
                cell,
                theme: &Theme::DARK,
                chrome: Chrome::default(),
                options: &options,
                selections: &sels,
                caret: CaretStyle::Bar,
                highlights: Highlights::default(),
                empty: None,
                slider_active: false,
            },
            &mut list,
        );
        let x = layout.text.x;
        let glyphs = list
            .glyphs
            .iter()
            .filter(|g| g.clip == layout.text && g.y == layout.text.y)
            .map(|g| ((g.x - x) / 10, g.ch))
            .collect();
        let carets = list.overlays.iter().map(|f| (f.rect.x - x) / 10).collect();
        (glyphs, carets)
    }

    #[test]
    fn long_lines_draw_their_end() {
        let mut text = vec![b'a'; 200 * 1024 - 3];
        text.extend_from_slice(b"END");
        let len = text.len() as u64;
        let (glyphs, carets) = long_row(&text, len - 10, &[len]);
        let tail: String = glyphs.iter().map(|g| g.1).collect();
        assert!(tail.ends_with("aEND"), "{tail}");
        assert_eq!(glyphs.last(), Some(&(9, 'D')));
        assert_eq!(carets, vec![10]);
    }

    #[test]
    fn a_character_straddling_the_scan_end_is_drawn_whole() {
        let mut text = vec![b'a'; ROW_SCAN_LIMIT - 1];
        text.extend_from_slice("\u{4E2D}bc".as_bytes());
        let first = ROW_SCAN_LIMIT as u64 - 5;
        let after = ROW_SCAN_LIMIT as u64 + 2;
        let (glyphs, carets) = long_row(&text, first, &[after]);
        assert!(glyphs.iter().all(|g| g.1 != '\u{FFFD}'), "{glyphs:?}");
        assert!(glyphs.contains(&(4, '\u{4E2D}')));
        // One column per byte past the scan end: 'b' and the caret after the character.
        assert!(glyphs.contains(&(7, 'b')));
        assert_eq!(carets, vec![7]);
    }

    #[test]
    fn characters_cut_by_the_left_edge_past_the_scan_end_are_skipped() {
        let mut text = vec![b'a'; ROW_SCAN_LIMIT];
        for _ in 0..100 {
            text.extend_from_slice("\u{4E2D}".as_bytes());
        }
        // Column 4 past the scan end is inside the second character (bytes 3..6).
        let (glyphs, _) = long_row(&text, ROW_SCAN_LIMIT as u64 + 4, &[]);
        assert!(glyphs.iter().all(|g| g.1 == '\u{4E2D}'), "{glyphs:?}");
        assert_eq!(glyphs.first(), Some(&(2, '\u{4E2D}')));
        assert_eq!(glyphs.last().map(|g| g.0 % 3), Some(2));
    }

    #[test]
    fn marks_invalid_bytes_and_crlf_past_the_scan_end() {
        let mut text = vec![b'a'; ROW_SCAN_LIMIT];
        text.extend_from_slice(b"e\xCC\x81\xFFz\r\nq");
        let (glyphs, _) = long_row(&text, ROW_SCAN_LIMIT as u64 - 2, &[]);
        assert_eq!(
            glyphs,
            vec![
                (0, 'a'),
                (1, 'a'),
                (2, 'e'),
                (2, '\u{301}'),
                (5, '\u{FFFD}'),
                (6, 'z'),
            ]
        );
    }

    #[test]
    fn marks_at_the_scan_end_join_the_cell_before_it() {
        let mark = "\u{301}".as_bytes();
        // A mark right at the seam, and one straddling it.
        for before in [ROW_SCAN_LIMIT, ROW_SCAN_LIMIT - 1] {
            let mut text = vec![b'a'; before];
            text.extend_from_slice(mark);
            text.push(b'z');
            let (glyphs, _) = long_row(&text, before as u64 - 2, &[]);
            assert_eq!(
                glyphs,
                vec![(0, 'a'), (1, 'a'), (1, '\u{301}'), (4, 'z')],
                "{before}"
            );
        }
        // Four marks before the seam: a fifth after it is not drawn.
        let mut text = vec![b'a'; ROW_SCAN_LIMIT - 4 * mark.len()];
        for _ in 0..5 {
            text.extend_from_slice(mark);
        }
        text.push(b'z');
        let first = (ROW_SCAN_LIMIT - 4 * mark.len()) as u64 - 1;
        let (glyphs, _) = long_row(&text, first, &[]);
        let marks = glyphs.iter().filter(|g| g.1 == '\u{301}').count();
        assert_eq!((glyphs[0], marks), ((0, 'a'), 4));
    }
}
