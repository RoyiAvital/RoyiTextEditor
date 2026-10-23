//! Viewport layout: turns the visible part of a snapshot plus window chrome
//! into a [`DisplayList`] of filled rectangles and glyph cells.
//!
//! All geometry is integer: every character occupies one or two cells of a
//! fixed size (monospaced font, LTR, no shaping).

use rte_edit::{Selection, column_of, content_range, offset_in_line};
use rte_text::Snapshot;
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
}

impl Default for Options {
    fn default() -> Options {
        Options {
            tab_width: 4,
            line_numbers: true,
        }
    }
}

/// Screen areas for a window size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub title: Rect,
    pub gutter: Rect,
    pub text: Rect,
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
            h: bar.min(height),
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
        let text = Rect {
            x: gutter.w as i32,
            y: body_y,
            w: width - gutter.w,
            h: body_h,
        };
        Layout {
            title,
            gutter,
            text,
            status,
            rows: body_h.div_ceil(cell.height.max(1)),
            cols: text.w / cell.width.max(1),
        }
    }
}

fn digits(n: u64) -> u32 {
    n.max(1).ilog10() + 1
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
    pub color: Color,
    pub clip: Rect,
}

#[derive(Clone, Debug, Default)]
pub struct DisplayList {
    pub fills: Vec<Fill>,
    pub glyphs: Vec<Glyph>,
    /// Filled after the glyphs (carets).
    pub overlays: Vec<Fill>,
}

impl DisplayList {
    pub fn clear(&mut self) {
        self.fills.clear();
        self.glyphs.clear();
        self.overlays.clear();
    }
}

/// Builds display lists, reusing its buffers between frames.
#[derive(Default)]
pub struct ViewBuilder {
    line: Vec<u8>,
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
        fill(out, layout.title, theme.title_background);
        fill(out, layout.status, status_background);

        let title_y = layout.title.y + (layout.title.h.saturating_sub(cell.height) / 2) as i32;
        let title_cols = text_run(
            out,
            chrome.title,
            cell.width as i32,
            title_y,
            cell,
            theme.title_text,
            layout.title,
        );
        if let Some(color) = chrome.title_mark {
            let x = (cell.width * (title_cols + 2)) as i32;
            text_run(out, "\u{25CF}", x, title_y, cell, color, layout.title);
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
                out,
                "\u{2502}",
                divider_x as i32,
                title_y,
                cell,
                theme.line_number,
                layout.title,
            );
            text_run(
                out,
                chrome.app_name,
                name_x as i32,
                title_y,
                cell,
                theme.title_text,
                layout.title,
            );
        }
        let status_y = layout.status.y + (layout.status.h.saturating_sub(cell.height) / 2) as i32;
        text_run(
            out,
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
            out,
            chrome.status_right,
            right_x,
            status_y,
            cell,
            status_text,
            layout.status,
        );

        let mut number = String::new();
        for row in 0..layout.rows {
            let line = scroll.top_line + u64::from(row);
            let Some(range) = snapshot.line_range(line) else {
                break;
            };
            let y = layout.text.y + (row * cell.height) as i32;
            if options.line_numbers {
                number.clear();
                number.push_str(&(line + 1).to_string());
                let right = layout.gutter.x + layout.gutter.w as i32 - cell.width as i32;
                let x = right - number.len() as i32 * cell.width as i32;
                text_run(out, &number, x, y, cell, theme.line_number, layout.gutter);
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
            let mut content_end = range.end;
            if end == range.end && crlf && bytes.last() == Some(&b'\r') {
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
            selection_row(out, frame, row_rect, bytes, content.clone(), crlf);
            let text_x = layout.text.x;
            let clip = layout.text;
            layout_line(
                bytes,
                scroll.first_col,
                layout.cols + 1,
                options.tab_width,
                |col, ch, kind| {
                    let color = match kind {
                        Kind::Text => theme.text,
                        Kind::Control => theme.control,
                    };
                    out.glyphs.push(Glyph {
                        x: text_x + (col * cell.width) as i32,
                        y,
                        ch,
                        color,
                        clip,
                    });
                },
            );
            if caret != CaretStyle::Hidden {
                caret_row(out, frame, row_rect, bytes, content);
            }
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
    out: &mut DisplayList,
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
            out.glyphs.push(Glyph {
                x: gx,
                y,
                ch,
                color,
                clip,
            });
        }
        col += width;
    }
    col
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Text,
    /// Control character or invalid byte, drawn as a symbol.
    Control,
}

/// Lays out one line, calling `emit(visible_col, char, kind)` for every
/// character visible in `[first_col, first_col + cols)`.
pub fn layout_line(
    bytes: &[u8],
    first_col: u64,
    cols: u32,
    tab_width: u32,
    mut emit: impl FnMut(u32, char, Kind),
) {
    let end_col = first_col + u64::from(cols);
    let tab = u64::from(tab_width.max(1));
    let mut col = 0u64;
    // Visible column of the previous character and the marks drawn on it.
    let mut last_visible: Option<(u32, u32)> = None;
    let mut place =
        |col: &mut u64, last: &mut Option<(u32, u32)>, ch: char, width: u64, kind: Kind| {
            if width == 0 {
                if let Some((v, marks)) = last
                    && *marks < MAX_MARKS_PER_CELL
                {
                    *marks += 1;
                    emit(*v, ch, kind);
                }
                return;
            }
            *last = None;
            if *col >= first_col && *col + width <= end_col {
                let v = (*col - first_col) as u32;
                emit(v, ch, kind);
                *last = Some((v, 0));
            }
            *col += width;
        };
    for chunk in bytes.utf8_chunks() {
        for ch in chunk.valid().chars() {
            if col >= end_col {
                return;
            }
            let last = &mut last_visible;
            match ch {
                '\t' => {
                    col = (col / tab + 1) * tab;
                    *last = None;
                }
                '\0'..='\u{1F}' => {
                    let symbol = char::from_u32(0x2400 + ch as u32).unwrap();
                    place(&mut col, last, symbol, 1, Kind::Control);
                }
                '\u{7F}' => place(&mut col, last, '\u{2421}', 1, Kind::Control),
                _ => place(
                    &mut col,
                    last,
                    ch,
                    ch.width().unwrap_or(1) as u64,
                    Kind::Text,
                ),
            }
        }
        for _ in chunk.invalid() {
            if col >= end_col {
                return;
            }
            place(&mut col, &mut last_visible, '\u{FFFD}', 1, Kind::Control);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cells(bytes: &[u8], first_col: u64, cols: u32) -> Vec<(u32, char, Kind)> {
        let mut out = Vec::new();
        layout_line(bytes, first_col, cols, 4, |c, ch, k| out.push((c, ch, k)));
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
            )
        };
        // 400 px: "│ App" ends one cell before the right edge.
        assert_eq!(name(400), ("\u{2502}App".to_owned(), Some(400 - 60)));
        // "title" and its marker end at 80 px; the divider needs a 1-cell gap.
        assert_eq!(name(150), ("\u{2502}App".to_owned(), Some(90)));
        assert_eq!(name(149), (String::new(), None));
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
        layout_line(line, 0, 40, 4, |c, ch, _| drawn.push((c, ch)));
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
}
