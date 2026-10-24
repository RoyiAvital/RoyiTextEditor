//! Popups drawn above the text: the command palette and text panels. Each
//! has a geometry shared by drawing and mouse hit tests.

use std::sync::Arc;

use unicode_width::UnicodeWidthStr;

use crate::{Cell, Color, Fill, Glyph, Rect, Theme, text_run};

/// Premultiplied `0xAARRGGBB` pixels, `width * height`, top-down rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub pixels: Arc<[u32]>,
}

/// A five-pointed star inside `rect`, outlined unless `filled`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Star {
    pub rect: Rect,
    pub color: Color,
    pub filled: bool,
}

/// Drawn in this order: fills, images, stars, glyphs.
#[derive(Clone, Debug, Default)]
pub struct Layer {
    pub fills: Vec<Fill>,
    pub images: Vec<Image>,
    pub stars: Vec<Star>,
    pub glyphs: Vec<Glyph>,
}

impl Layer {
    pub fn clear(&mut self) {
        self.fills.clear();
        self.images.clear();
        self.stars.clear();
        self.glyphs.clear();
    }

    fn fill(&mut self, rect: Rect, color: Color) {
        self.fills.push(Fill { rect, color });
    }

    /// Border and background of a popup.
    fn frame(&mut self, rect: Rect, theme: &Theme) {
        self.fill(rect, theme.popup_border);
        if rect.w > 2 && rect.h > 2 {
            let inner = Rect {
                x: rect.x + 1,
                y: rect.y + 1,
                w: rect.w - 2,
                h: rect.h - 2,
            };
            self.fill(inner, theme.popup_background);
        }
    }
}

/// Most palette rows shown at once.
pub const PALETTE_ROWS: usize = 12;
/// Widest palette, in cells.
const PALETTE_COLS: u32 = 80;
/// Cells left of a palette label (the star).
const STAR_COLS: u32 = 3;

/// Palette areas for a window size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaletteGeometry {
    pub panel: Rect,
    pub input: Rect,
    pub list: Rect,
    pub row_h: u32,
    /// Rows that fit (at least one).
    pub rows: usize,
}

impl PaletteGeometry {
    /// `top`: first pixel row below the title strip; `count`: rows to show.
    /// The palette moves up over the title strip when the client is too low.
    pub fn new(width: u32, height: u32, top: i32, cell: Cell, count: usize) -> PaletteGeometry {
        let (cw, ch) = (cell.width.max(1), cell.height.max(1));
        let pad = ch / 3;
        let window_cols = width / cw;
        let cols = window_cols
            .saturating_sub(4)
            .max(window_cols.min(20))
            .min(PALETTE_COLS);
        let w = (cols * cw + 2).min(width).max(2);
        let x = (width as i32 - w as i32) / 2;
        let input_h = ch + pad;
        let row_h = ch + ch / 3;
        // Border, padding and input above the list; padding and border below.
        let chrome = 1 + pad + input_h + pad + pad + 1;
        let y = (top + (ch / 2) as i32)
            .min(height as i32 - (chrome + row_h) as i32)
            .max(0);
        let input = Rect {
            x: x + 1 + pad as i32,
            y: y + 1 + pad as i32,
            w: w.saturating_sub(2 + 2 * pad),
            h: input_h,
        };
        let list_y = input.y + (input.h + pad) as i32;
        let avail = (height as i32 - list_y - (pad + 1) as i32).max(0) as u32;
        let rows = count
            .clamp(1, PALETTE_ROWS)
            .min((avail / row_h).max(1) as usize);
        let list = Rect {
            x: x + 1,
            y: list_y,
            w: w - 2,
            h: rows as u32 * row_h,
        };
        let panel = Rect {
            x,
            y,
            w,
            h: (list.y + (list.h + pad + 1) as i32 - y) as u32,
        };
        PaletteGeometry {
            panel,
            input,
            list,
            row_h,
            rows,
        }
    }

    /// Visible row index at a point.
    pub fn row_at(&self, x: i32, y: i32) -> Option<usize> {
        if !self.list.contains(x, y) {
            return None;
        }
        let i = ((y - self.list.y) / self.row_h as i32) as usize;
        (i < self.rows).then_some(i)
    }

    fn row(&self, i: usize) -> Rect {
        Rect {
            x: self.list.x,
            y: self.list.y + (i as u32 * self.row_h) as i32,
            w: self.list.w,
            h: self.row_h,
        }
    }

    /// Clickable star area of visible row `i`.
    pub fn star_area(&self, i: usize, cell: Cell) -> Rect {
        Rect {
            w: STAR_COLS * cell.width,
            ..self.row(i)
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum PaletteRow<'a> {
    Header(&'a str),
    Command {
        label: &'a str,
        /// Sorted character indices of `label` matching the query.
        matched: &'a [u32],
        keys: &'a str,
        starred: bool,
        enabled: bool,
    },
}

pub struct PaletteFrame<'a> {
    pub cell: Cell,
    pub theme: &'a Theme,
    pub query: &'a str,
    /// Caret and selection in query cells.
    pub caret: u32,
    pub selection: (u32, u32),
    pub placeholder: &'a str,
    /// Shown when there are no rows.
    pub empty: &'a str,
    pub selected: Option<usize>,
}

/// Draws the palette with its visible `rows` (at most `rows` of the geometry).
pub fn draw_palette<'a>(
    g: &PaletteGeometry,
    f: &PaletteFrame<'_>,
    rows: impl IntoIterator<Item = PaletteRow<'a>>,
    out: &mut Layer,
) {
    let (theme, cell) = (f.theme, f.cell);
    let cw = cell.width as i32;
    out.frame(g.panel, theme);
    out.fill(g.input, theme.popup_input);
    let text_y = g.input.y + (g.input.h.saturating_sub(cell.height) / 2) as i32;
    // Scroll so the caret stays inside the input.
    let visible = (g.input.w / cell.width.max(1)).saturating_sub(1).max(1);
    let offset = (f.caret + 1).saturating_sub(visible);
    let text_x = g.input.x + cw / 2 - (offset * cell.width) as i32;
    let col_x = |col: u32| text_x + (col * cell.width) as i32;
    if f.query.is_empty() {
        let glyphs = &mut out.glyphs;
        text_run(
            glyphs,
            f.placeholder,
            text_x,
            text_y,
            cell,
            theme.popup_dim,
            g.input,
        );
    } else {
        let (start, end) = f.selection;
        let selection = Rect {
            x: col_x(start),
            y: text_y,
            w: (end.saturating_sub(start)) * cell.width,
            h: cell.height,
        };
        if let Some(selection) = selection.intersect(&g.input) {
            out.fill(selection, theme.popup_selected);
        }
        text_run(
            &mut out.glyphs,
            f.query,
            text_x,
            text_y,
            cell,
            theme.text,
            g.input,
        );
    }
    let caret = Rect {
        x: col_x(f.caret),
        y: text_y,
        w: (cell.height / 10).max(1),
        h: cell.height,
    };
    if let Some(caret) = caret.intersect(&g.input) {
        out.fill(caret, theme.caret);
    }

    let row_y = |r: Rect| r.y + (g.row_h.saturating_sub(cell.height) / 2) as i32;
    let mut drawn = 0;
    for (i, row) in rows.into_iter().enumerate().take(g.rows) {
        drawn += 1;
        let r = g.row(i);
        let y = row_y(r);
        match row {
            PaletteRow::Header(text) => {
                text_run(&mut out.glyphs, text, r.x + cw, y, cell, theme.popup_dim, r);
            }
            PaletteRow::Command {
                label,
                matched,
                keys,
                starred,
                enabled,
            } => {
                if f.selected == Some(i) {
                    out.fill(r, theme.popup_selected);
                }
                let area = g.star_area(i, cell);
                let size = cell.height.min(2 * cell.width) * 4 / 5;
                out.stars.push(Star {
                    rect: Rect {
                        x: area.x + (area.w.saturating_sub(size) / 2) as i32,
                        y: area.y + (area.h.saturating_sub(size) / 2) as i32,
                        w: size,
                        h: size,
                    },
                    color: if starred {
                        theme.favorite
                    } else {
                        theme.popup_dim
                    },
                    filled: starred,
                });
                let keys_x = r.x + r.w as i32 - cw - (keys.width() as u32 * cell.width) as i32;
                let label_x = r.x + (STAR_COLS * cell.width) as i32;
                let clip = Rect {
                    w: (keys_x - cw - r.x).max(0) as u32,
                    ..r
                };
                let base = if enabled { theme.text } else { theme.popup_dim };
                let mut col = 0;
                let mut next = matched.iter().copied().peekable();
                let mut buf = [0u8; 4];
                for (index, ch) in label.chars().enumerate() {
                    let color = if next.peek() == Some(&(index as u32)) {
                        next.next();
                        theme.popup_match
                    } else {
                        base
                    };
                    let x = label_x + (col * cell.width) as i32;
                    col += text_run(
                        &mut out.glyphs,
                        ch.encode_utf8(&mut buf),
                        x,
                        y,
                        cell,
                        color,
                        clip,
                    );
                }
                text_run(&mut out.glyphs, keys, keys_x, y, cell, theme.popup_dim, r);
            }
        }
    }
    if drawn == 0 {
        let r = g.row(0);
        let x = r.x + (STAR_COLS * cell.width) as i32;
        text_run(
            &mut out.glyphs,
            f.empty,
            x,
            row_y(r),
            cell,
            theme.popup_dim,
            r,
        );
    }
}

/// Style of a panel text span.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Normal,
    Dim,
    Heading,
    /// Underlined; the whole line is clickable.
    Link,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PanelLine {
    pub spans: Vec<(String, Tone)>,
    pub centered: bool,
}

impl PanelLine {
    pub fn new(spans: &[(&str, Tone)]) -> PanelLine {
        PanelLine {
            spans: spans
                .iter()
                .map(|&(t, tone)| (t.to_owned(), tone))
                .collect(),
            centered: false,
        }
    }

    /// Width in cells.
    pub fn cols(&self) -> u32 {
        self.spans.iter().map(|(t, _)| t.width() as u32).sum()
    }
}

/// Panel areas for a window size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PanelGeometry {
    pub panel: Rect,
    pub title: Rect,
    pub image: Option<Rect>,
    pub body: Rect,
    pub footer: Rect,
    pub line_h: u32,
    /// Body lines that fit.
    pub lines: usize,
}

impl PanelGeometry {
    /// `cols`: widest line; `count`: body lines; `image`: side of a square
    /// image, left out when it would leave no room for a line. The panel moves
    /// up over the title strip when the client is too low.
    pub fn new(
        width: u32,
        height: u32,
        top: i32,
        cell: Cell,
        cols: u32,
        count: usize,
        image: Option<u32>,
    ) -> PanelGeometry {
        let (cw, ch) = (cell.width.max(1), cell.height.max(1));
        let pad = ch / 2;
        let inner_cols = (cols + 6).min((width / cw).saturating_sub(2)).max(1);
        let w = (inner_cols * cw + 2).min(width).max(2);
        let x = (width as i32 - w as i32) / 2;
        let line_h = ch + ch / 5;
        // Borders, paddings, title and footer.
        let chrome = 2 + 4 * pad + 2 * line_h;
        let image = image.filter(|&side| chrome + line_h + side + pad <= height);
        let image_h = image.map_or(0, |side| side + pad);
        let y = (top + ch as i32)
            .min(height as i32 - (chrome + line_h + image_h) as i32)
            .max(0);
        let title = Rect {
            x: x + 1,
            y: y + 1 + pad as i32,
            w: w - 2,
            h: line_h,
        };
        let mut cursor = title.y + (line_h + pad) as i32;
        let image = image.map(|side| {
            let rect = Rect {
                x: x + (w as i32 - side as i32) / 2,
                y: cursor,
                w: side,
                h: side,
            };
            cursor += (side + pad) as i32;
            rect
        });
        // Keep a cell of margin below the panel when the lines allow it.
        let avail = (height as i32 - y - (chrome + image_h + ch) as i32).max(0) as u32;
        let lines = count.min((avail / line_h).max(1) as usize);
        let body = Rect {
            x: x + 1 + 3 * cw as i32,
            y: cursor,
            w: (w - 2).saturating_sub(6 * cw),
            h: lines as u32 * line_h,
        };
        let footer = Rect {
            x: x + 1,
            y: body.y + (body.h + pad) as i32,
            w: w - 2,
            h: line_h,
        };
        let panel = Rect {
            x,
            y,
            w,
            h: (footer.y + (footer.h + pad + 1) as i32 - y) as u32,
        };
        PanelGeometry {
            panel,
            title,
            image,
            body,
            footer,
            line_h,
            lines,
        }
    }

    /// Visible body line index at a point.
    pub fn line_at(&self, x: i32, y: i32) -> Option<usize> {
        if !self.body.contains(x, y) {
            return None;
        }
        let i = ((y - self.body.y) / self.line_h as i32) as usize;
        (i < self.lines).then_some(i)
    }
}

pub struct PanelFrame<'a> {
    pub cell: Cell,
    pub theme: &'a Theme,
    pub title: &'a str,
    pub footer: &'a str,
    /// Visible body lines, at most `lines` of the geometry.
    pub lines: &'a [PanelLine],
    /// Pixels of the geometry's square image.
    pub image: Option<&'a Arc<[u32]>>,
}

pub fn draw_panel(g: &PanelGeometry, f: &PanelFrame<'_>, out: &mut Layer) {
    let (theme, cell) = (f.theme, f.cell);
    out.frame(g.panel, theme);
    let centered_x = |r: Rect, cols: u32| r.x + (r.w.saturating_sub(cols * cell.width) / 2) as i32;
    let text_y = |r: Rect| r.y + (g.line_h.saturating_sub(cell.height) / 2) as i32;
    let x = centered_x(g.title, f.title.width() as u32);
    text_run(
        &mut out.glyphs,
        f.title,
        x,
        text_y(g.title),
        cell,
        theme.text,
        g.title,
    );
    if let (Some(rect), Some(pixels)) = (g.image, f.image) {
        out.images.push(Image {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            pixels: pixels.clone(),
        });
    }
    for (i, line) in f.lines.iter().enumerate().take(g.lines) {
        let r = Rect {
            y: g.body.y + (i as u32 * g.line_h) as i32,
            h: g.line_h,
            ..g.body
        };
        let mut x = if line.centered {
            centered_x(r, line.cols())
        } else {
            r.x
        };
        let y = text_y(r);
        for (text, tone) in &line.spans {
            let color = match tone {
                Tone::Normal => theme.text,
                Tone::Dim => theme.popup_dim,
                Tone::Heading => theme.popup_match,
                Tone::Link => theme.link,
            };
            let cols = text_run(&mut out.glyphs, text, x, y, cell, color, r);
            if *tone == Tone::Link {
                let underline = Rect {
                    x,
                    y: y + cell.height as i32 - 2,
                    w: cols * cell.width,
                    h: 1,
                };
                if let Some(underline) = underline.intersect(&r) {
                    out.fill(underline, color);
                }
            }
            x += (cols * cell.width) as i32;
        }
    }
    let x = centered_x(g.footer, f.footer.width() as u32);
    text_run(
        &mut out.glyphs,
        f.footer,
        x,
        text_y(g.footer),
        cell,
        theme.popup_dim,
        g.footer,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const CELL: Cell = Cell {
        width: 10,
        height: 20,
    };

    fn text(out: &Layer, clip: Rect) -> String {
        out.glyphs
            .iter()
            .filter(|g| clip.contains(g.x, g.y))
            .map(|g| g.ch)
            .collect()
    }

    #[test]
    fn palette_fits_the_window_and_hits_its_rows() {
        let g = PaletteGeometry::new(1000, 700, 30, CELL, 40);
        assert_eq!(g.rows, PALETTE_ROWS);
        assert_eq!(g.panel.w, 80 * 10 + 2);
        assert!(g.panel.y + (g.panel.h as i32) < 700);
        assert_eq!(g.row_at(g.list.x + 5, g.list.y), Some(0));
        assert_eq!(g.row_at(g.list.x + 5, g.list.y + g.row_h as i32), Some(1));
        assert_eq!(g.row_at(g.list.x + 5, g.list.y - 1), None);
        let star = g.star_area(1, CELL);
        assert!(star.contains(g.list.x, g.list.y + g.row_h as i32));
        // A small window shows fewer rows, but at least one.
        let small = PaletteGeometry::new(300, 120, 30, CELL, 40);
        assert_eq!(small.rows, 1);
        assert!(small.panel.x >= 0);
    }

    #[test]
    fn popups_fit_low_clients_by_covering_the_title_strip() {
        // From the smallest height holding the input and one row (72 px).
        for height in [72, 80, 100, 140] {
            let g = PaletteGeometry::new(200, height, 30, CELL, 40);
            assert!(g.panel.y >= 0, "{height}");
            assert!(
                g.list.y + g.row_h as i32 <= height as i32,
                "{height}: a row is visible"
            );
            assert!(g.panel.y + g.panel.h as i32 <= height as i32, "{height}");
            assert!(g.panel.w <= 200);
        }
        // Not enough room for the image and a line: the image is left out.
        let g = PanelGeometry::new(300, 150, 30, CELL, 20, 5, Some(64));
        assert_eq!(g.image, None);
        assert!(g.lines >= 1);
        assert!(g.panel.y >= 0 && g.panel.y + g.panel.h as i32 <= 150);
        let g = PanelGeometry::new(300, 400, 30, CELL, 20, 5, Some(64));
        assert!(g.image.is_some());
    }

    #[test]
    fn palette_rows_show_stars_matches_and_keys() {
        let g = PaletteGeometry::new(1000, 700, 30, CELL, 2);
        let rows = [
            PaletteRow::Header("recent"),
            PaletteRow::Command {
                label: "File: Save",
                matched: &[6, 7],
                keys: "Ctrl+S",
                starred: true,
                enabled: true,
            },
        ];
        let mut out = Layer::default();
        let frame = PaletteFrame {
            cell: CELL,
            theme: &Theme::DARK,
            query: "sa",
            caret: 2,
            selection: (2, 2),
            placeholder: "Type a command",
            empty: "Nothing",
            selected: Some(1),
        };
        draw_palette(&g, &frame, rows, &mut out);
        assert_eq!(text(&out, g.input), "sa");
        assert_eq!(text(&out, g.row(0)), "recent");
        assert_eq!(text(&out, g.row(1)), "File:SaveCtrl+S");
        let matched: String = out
            .glyphs
            .iter()
            .filter(|g| g.color == Theme::DARK.popup_match)
            .map(|g| g.ch)
            .collect();
        assert_eq!(matched, "Sa");
        assert_eq!(out.stars.len(), 1);
        assert!(out.stars[0].filled);
        assert!(
            g.star_area(1, CELL)
                .contains(out.stars[0].rect.x, out.stars[0].rect.y)
        );
        assert!(
            out.fills
                .iter()
                .any(|f| f.rect == g.row(1) && f.color == Theme::DARK.popup_selected)
        );
    }

    #[test]
    fn empty_palette_shows_its_hint() {
        let g = PaletteGeometry::new(1000, 700, 30, CELL, 0);
        let mut out = Layer::default();
        let frame = PaletteFrame {
            cell: CELL,
            theme: &Theme::DARK,
            query: "",
            caret: 0,
            selection: (0, 0),
            placeholder: "Type",
            empty: "Nothing",
            selected: None,
        };
        draw_palette(&g, &frame, [], &mut out);
        assert_eq!(text(&out, g.input), "Type");
        assert_eq!(text(&out, g.row(0)), "Nothing");
        assert!(out.stars.is_empty());
    }

    #[test]
    fn long_queries_scroll_to_the_caret_and_show_the_selection() {
        let g = PaletteGeometry::new(300, 400, 30, CELL, 1);
        let query = "abcdefghijklmnopqrstuvwxyz".repeat(2);
        let draw = |caret: u32, selection: (u32, u32)| {
            let mut out = Layer::default();
            let frame = PaletteFrame {
                cell: CELL,
                theme: &Theme::DARK,
                query: &query,
                caret,
                selection,
                placeholder: "",
                empty: "",
                selected: None,
            };
            draw_palette(&g, &frame, [], &mut out);
            out
        };
        let caret_at = |out: &Layer| {
            out.fills
                .iter()
                .find(|f| f.color == Theme::DARK.caret)
                .map(|f| f.rect.x)
        };
        let end = draw(52, (52, 52));
        let x = caret_at(&end).expect("the caret is visible at the end");
        assert!(x < g.input.x + g.input.w as i32);
        assert!(text(&end, g.input).ends_with("xyz"));
        let start = draw(0, (0, 3));
        assert!(text(&start, g.input).starts_with("abc"));
        assert!(
            start
                .fills
                .iter()
                .any(|f| f.color == Theme::DARK.popup_selected
                    && f.rect.w == 30
                    && g.input.contains(f.rect.x, f.rect.y))
        );
    }

    #[test]
    fn panel_lines_links_and_image() {
        let lines = vec![
            PanelLine::new(&[("Version ", Tone::Dim), ("1.0", Tone::Normal)]),
            PanelLine::new(&[("https://x", Tone::Link)]),
        ];
        let g = PanelGeometry::new(1000, 700, 30, CELL, 20, lines.len(), Some(64));
        let pixels: Arc<[u32]> = vec![0xFF00_0000; 64 * 64].into();
        let mut out = Layer::default();
        let frame = PanelFrame {
            cell: CELL,
            theme: &Theme::DARK,
            title: "About",
            footer: "Esc",
            lines: &lines,
            image: Some(&pixels),
        };
        draw_panel(&g, &frame, &mut out);
        assert_eq!(out.images.len(), 1);
        assert_eq!(out.images[0].width, 64);
        let image = g.image.unwrap();
        assert!(image.y >= g.title.y + g.title.h as i32);
        assert!(g.body.y >= image.y + 64);
        assert_eq!(text(&out, g.title), "About");
        assert_eq!(text(&out, g.footer), "Esc");
        let link_y = g.body.y + g.line_h as i32 + 1;
        assert_eq!(g.line_at(g.body.x + 1, link_y), Some(1));
        assert!(
            out.fills
                .iter()
                .any(|f| f.color == Theme::DARK.link && f.rect.h == 1)
        );
    }

    #[test]
    fn long_panels_show_the_lines_that_fit() {
        let g = PanelGeometry::new(800, 300, 30, CELL, 30, 100, None);
        assert!(g.lines > 0 && g.lines < 100);
        assert!(g.panel.y + (g.panel.h as i32) <= 300);
    }
}
