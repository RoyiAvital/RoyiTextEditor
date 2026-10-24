//! The search box: geometry shared by drawing and mouse hit tests, and its
//! drawing. It sits at the top right of the text area and keeps the popup
//! font size when the editor is zoomed.

use unicode_width::UnicodeWidthStr;

use crate::popup::Layer;
use crate::{Cell, Color, Rect, Theme, text_run};

/// Symbols drawn by the search box (all must exist in the default face).
pub const FIND_SYMBOLS: &str = "\u{25B8}\u{25BE}\u{2191}\u{2193}\u{2261}\u{00D7}\u{00B6}";
const FOLDED: &str = "\u{25B8}";
const UNFOLDED: &str = "\u{25BE}";
const PREV: &str = "\u{2191}";
const NEXT: &str = "\u{2193}";
const IN_SELECTION: &str = "\u{2261}";
const CLOSE: &str = "\u{00D7}";
/// Shown for a line break in a field.
pub const LINE_BREAK: char = '\u{00B6}';

/// Toggle labels inside the find field: Match Case, Whole Word, Glob, RegEx.
pub const TOGGLES: [&str; 4] = ["Aa", "ab", "*?", ".*"];
const TOGGLE_COLS: u32 = 3;
/// Widest text area of a field, in cells.
const TEXT_COLS: u32 = 24;
const COUNT_COLS: u32 = 11;
const BUTTON_COLS: u32 = 3;
const REPLACE_LABEL: &str = "Replace";
const REPLACE_ALL_LABEL: &str = "Replace All";

/// A clickable part of the search box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindHit {
    Fold,
    FindText,
    Toggle(usize),
    Prev,
    Next,
    InSelection,
    Close,
    ReplaceText,
    Preserve,
    ReplaceOne,
    ReplaceAll,
    /// Inside the box but on nothing clickable.
    Panel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplaceGeometry {
    pub field: Rect,
    pub text: Rect,
    pub preserve: Rect,
    pub one: Rect,
    pub all: Rect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FindGeometry {
    pub panel: Rect,
    pub fold: Rect,
    pub field: Rect,
    pub text: Rect,
    pub toggles: [Rect; 4],
    pub count: Rect,
    pub prev: Rect,
    pub next: Rect,
    pub in_selection: Rect,
    pub close: Rect,
    pub replace: Option<ReplaceGeometry>,
    /// Message area under the find field.
    pub message: Rect,
}

impl FindGeometry {
    /// `top`: first pixel row below the title strip; `right`: right edge
    /// of the text area.
    pub fn new(right: i32, top: i32, cell: Cell, replace: bool) -> FindGeometry {
        let (cw, ch) = (cell.width.max(1), cell.height.max(1));
        let pad = (ch / 4).max(1) as i32;
        let row_h = ch + ch / 3;
        let fixed = 2 + TOGGLE_COLS * 4 + 1 + COUNT_COLS + BUTTON_COLS * 4 + 1;
        let avail = (right.max(0) as u32 / cw).saturating_sub(fixed + 4);
        let text_cols = avail.clamp(6, TEXT_COLS);
        let field_w = (text_cols + TOGGLE_COLS * 4) * cw + 2;
        let w = (fixed - TOGGLE_COLS * 4) * cw + field_w;
        let x = right - 2 * cw as i32 - w as i32;
        let rows = if replace { 2 } else { 1 };
        let h = 2 * pad as u32 + rows * row_h + (rows - 1) * pad as u32;
        let panel = Rect { x, y: top, w, h };
        let cols = |c: u32| (c * cw) as i32;
        let row_y = top + pad;
        let fold = Rect {
            x,
            y: row_y,
            w: 2 * cw,
            h: h - 2 * pad as u32,
        };
        let field = Rect {
            x: x + cols(2),
            y: row_y,
            w: field_w,
            h: row_h,
        };
        let toggles_x = field.x + field.w as i32 - 1 - cols(TOGGLE_COLS * 4);
        let toggle = |i: u32| Rect {
            x: toggles_x + cols(TOGGLE_COLS * i),
            y: row_y + 2,
            w: TOGGLE_COLS * cw,
            h: row_h - 4,
        };
        let text = Rect {
            x: field.x + 1,
            y: row_y + 1,
            w: (toggles_x - field.x - 1).max(0) as u32,
            h: row_h - 2,
        };
        let count = Rect {
            x: field.x + field.w as i32 + cols(1),
            y: row_y,
            w: COUNT_COLS * cw,
            h: row_h,
        };
        let button = |i: u32| Rect {
            x: count.x + count.w as i32 + cols(BUTTON_COLS * i),
            y: row_y,
            w: BUTTON_COLS * cw,
            h: row_h,
        };
        let replace = replace.then(|| {
            let y = row_y + (row_h as i32) + pad;
            let field = Rect { y, ..field };
            let preserve = Rect {
                x: field.x + field.w as i32 - 1 - cols(TOGGLE_COLS),
                y: y + 2,
                w: TOGGLE_COLS * cw,
                h: row_h - 4,
            };
            let text = Rect {
                x: field.x + 1,
                y: y + 1,
                w: (preserve.x - field.x - 1).max(0) as u32,
                h: row_h - 2,
            };
            let one_w = (REPLACE_LABEL.len() as u32 + 2) * cw;
            let all_w = (REPLACE_ALL_LABEL.len() as u32 + 2) * cw;
            let one = Rect {
                x: count.x,
                y,
                w: one_w,
                h: row_h,
            };
            let all = Rect {
                x: one.x + one_w as i32 + cols(1),
                y,
                w: all_w,
                h: row_h,
            };
            ReplaceGeometry {
                field,
                text,
                preserve,
                one,
                all,
            }
        });
        FindGeometry {
            panel,
            fold,
            field,
            text,
            toggles: [toggle(0), toggle(1), toggle(2), toggle(3)],
            count,
            prev: button(0),
            next: button(1),
            in_selection: button(2),
            close: button(3),
            replace,
            message: Rect {
                x: field.x,
                y: panel.y + panel.h as i32,
                w: field.w,
                h: row_h,
            },
        }
    }

    pub fn hit(&self, x: i32, y: i32) -> Option<FindHit> {
        if !self.panel.contains(x, y) {
            return None;
        }
        if let Some(i) = self.toggles.iter().position(|r| r.contains(x, y)) {
            return Some(FindHit::Toggle(i));
        }
        let mut targets = vec![
            (self.fold, FindHit::Fold),
            (self.field, FindHit::FindText),
            (self.prev, FindHit::Prev),
            (self.next, FindHit::Next),
            (self.in_selection, FindHit::InSelection),
            (self.close, FindHit::Close),
        ];
        if let Some(r) = &self.replace {
            targets.extend([
                (r.preserve, FindHit::Preserve),
                (r.field, FindHit::ReplaceText),
                (r.one, FindHit::ReplaceOne),
                (r.all, FindHit::ReplaceAll),
            ]);
        }
        Some(
            targets
                .into_iter()
                .find(|(r, _)| r.contains(x, y))
                .map_or(FindHit::Panel, |(_, hit)| hit),
        )
    }
}

/// A text field of the box: text, caret and selection in cells.
#[derive(Clone, Copy, Debug)]
pub struct FindField<'a> {
    pub text: &'a str,
    pub caret: u32,
    pub selection: (u32, u32),
    pub focused: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

pub struct FindFrame<'a> {
    pub cell: Cell,
    pub theme: &'a Theme,
    pub find: FindField<'a>,
    pub replace: FindField<'a>,
    /// Match Case, Whole Word, Glob, RegEx.
    pub toggles: [bool; 4],
    pub preserve: bool,
    pub in_selection: bool,
    /// `2 of 9`, `No results`...
    pub count: &'a str,
    /// The count reports that nothing matches.
    pub no_results: bool,
    pub message: Option<(&'a str, Severity)>,
}

pub fn draw_find(g: &FindGeometry, f: &FindFrame<'_>, out: &mut Layer) {
    let (theme, cell) = (f.theme, f.cell);
    out.frame(g.panel, theme);
    let center = |out: &mut Layer, r: Rect, text: &str, color: Color| {
        let x = r.x + (r.w.saturating_sub(text.width() as u32 * cell.width) / 2) as i32;
        let y = r.y + (r.h.saturating_sub(cell.height) / 2) as i32;
        text_run(&mut out.glyphs, text, x, y, cell, color, r);
    };
    let fold = if g.replace.is_some() {
        UNFOLDED
    } else {
        FOLDED
    };
    center(out, g.fold, fold, theme.popup_dim);

    let border = match f.message {
        Some((_, Severity::Error)) => theme.error_border,
        Some((_, Severity::Warning)) => theme.at_risk,
        None if f.find.focused => theme.popup_match,
        None => theme.popup_border,
    };
    field(out, g.field, g.text, &f.find, border, "Find", f);
    for (i, (rect, label)) in g.toggles.iter().zip(TOGGLES).enumerate() {
        toggle(out, *rect, label, f.toggles[i], i == 1, f);
    }
    let count_color = if f.no_results {
        theme.error_text
    } else {
        theme.popup_dim
    };
    let count_y = g.count.y + (g.count.h.saturating_sub(cell.height) / 2) as i32;
    text_run(
        &mut out.glyphs,
        f.count,
        g.count.x,
        count_y,
        cell,
        count_color,
        g.count,
    );
    let nav = if f.no_results {
        theme.popup_dim
    } else {
        theme.text
    };
    center(out, g.prev, PREV, nav);
    center(out, g.next, NEXT, nav);
    if f.in_selection {
        toggled(out, g.in_selection, theme);
    }
    let in_sel = if f.in_selection { 0xFFFFFF } else { theme.text };
    center(out, g.in_selection, IN_SELECTION, in_sel);
    center(out, g.close, CLOSE, theme.text);

    if let Some(r) = &g.replace {
        let border = if f.replace.focused {
            theme.popup_match
        } else {
            theme.popup_border
        };
        field(out, r.field, r.text, &f.replace, border, "Replace", f);
        toggle(out, r.preserve, "AB", f.preserve, false, f);
        for (rect, label) in [(r.one, REPLACE_LABEL), (r.all, REPLACE_ALL_LABEL)] {
            outline(out, rect, theme.popup_border);
            center(out, rect, label, theme.text);
        }
    }

    if let Some((text, severity)) = f.message {
        let (background, edge, color) = match severity {
            Severity::Error => (theme.error_background, theme.error_border, theme.error_text),
            Severity::Warning => (theme.prompt_background, theme.at_risk, theme.prompt_text),
        };
        out.fill(g.message, edge);
        out.fill(inset(g.message), background);
        let y = g.message.y + (g.message.h.saturating_sub(cell.height) / 2) as i32;
        let x = g.message.x + (cell.width / 2) as i32;
        text_run(&mut out.glyphs, text, x, y, cell, color, inset(g.message));
    }
}

fn inset(r: Rect) -> Rect {
    Rect {
        x: r.x + 1,
        y: r.y + 1,
        w: r.w.saturating_sub(2),
        h: r.h.saturating_sub(2),
    }
}

fn outline(out: &mut Layer, r: Rect, color: Color) {
    let lines = [
        Rect { h: 1, ..r },
        Rect {
            y: r.y + r.h as i32 - 1,
            h: 1,
            ..r
        },
        Rect { w: 1, ..r },
        Rect {
            x: r.x + r.w as i32 - 1,
            w: 1,
            ..r
        },
    ];
    for line in lines {
        out.fill(line, color);
    }
}

fn toggled(out: &mut Layer, r: Rect, theme: &Theme) {
    out.fill(r, theme.popup_match);
    out.fill(inset(r), theme.popup_selected);
}

fn toggle(out: &mut Layer, r: Rect, label: &str, on: bool, underline: bool, f: &FindFrame<'_>) {
    let (theme, cell) = (f.theme, f.cell);
    if on {
        toggled(out, r, theme);
    }
    let color = if on { 0xFFFFFF } else { theme.popup_dim };
    let cols = label.width() as u32;
    let x = r.x + (r.w.saturating_sub(cols * cell.width) / 2) as i32;
    let y = r.y + (r.h.saturating_sub(cell.height) / 2) as i32;
    text_run(&mut out.glyphs, label, x, y, cell, color, r);
    if underline {
        let line = Rect {
            x,
            y: y + cell.height as i32 - 2,
            w: cols * cell.width,
            h: 1,
        };
        if let Some(line) = line.intersect(&r) {
            out.fill(line, color);
        }
    }
}

fn field(
    out: &mut Layer,
    rect: Rect,
    text_area: Rect,
    field: &FindField<'_>,
    border: Color,
    placeholder: &str,
    f: &FindFrame<'_>,
) {
    let (theme, cell) = (f.theme, f.cell);
    out.fill(rect, border);
    out.fill(inset(rect), theme.popup_input);
    let text_y = text_area.y + (text_area.h.saturating_sub(cell.height) / 2) as i32;
    let visible = (text_area.w / cell.width.max(1)).saturating_sub(1).max(1);
    let offset = (field.caret + 1).saturating_sub(visible);
    let text_x = text_area.x + (cell.width / 2) as i32 - (offset * cell.width) as i32;
    let col_x = |col: u32| text_x + (col * cell.width) as i32;
    if field.text.is_empty() {
        text_run(
            &mut out.glyphs,
            placeholder,
            text_x,
            text_y,
            cell,
            theme.popup_dim,
            text_area,
        );
    } else {
        let (start, end) = field.selection;
        if field.focused && end > start {
            let sel = Rect {
                x: col_x(start),
                y: text_y,
                w: (end - start) * cell.width,
                h: cell.height,
            };
            if let Some(sel) = sel.intersect(&text_area) {
                out.fill(sel, theme.popup_selected);
            }
        }
        let mut col = 0;
        let mut buf = [0u8; 4];
        for ch in field.text.chars() {
            let (shown, color) = if ch == '\n' {
                (LINE_BREAK, theme.popup_dim)
            } else {
                (ch, theme.text)
            };
            col += text_run(
                &mut out.glyphs,
                shown.encode_utf8(&mut buf),
                col_x(col),
                text_y,
                cell,
                color,
                text_area,
            );
        }
    }
    if field.focused {
        let caret = Rect {
            x: col_x(field.caret),
            y: text_y,
            w: (cell.height / 10).max(1),
            h: cell.height,
        };
        if let Some(caret) = caret.intersect(&text_area) {
            out.fill(caret, theme.caret);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CELL: Cell = Cell {
        width: 10,
        height: 20,
    };

    #[test]
    fn box_sits_top_right_and_hits_its_parts() {
        let g = FindGeometry::new(1200, 30, CELL, false);
        assert_eq!(g.panel.y, 30);
        assert_eq!(g.panel.x + g.panel.w as i32, 1200 - 20);
        assert!(g.replace.is_none());
        let mid = |r: Rect| (r.x + r.w as i32 / 2, r.y + r.h as i32 / 2);
        let (x, y) = mid(g.toggles[3]);
        assert_eq!(g.hit(x, y), Some(FindHit::Toggle(3)));
        let (x, y) = mid(g.text);
        assert_eq!(g.hit(x, y), Some(FindHit::FindText));
        let (x, y) = mid(g.close);
        assert_eq!(g.hit(x, y), Some(FindHit::Close));
        assert_eq!(g.hit(0, 0), None);
        assert!(g.text.x + g.text.w as i32 <= g.toggles[0].x);
        assert!(g.close.x + g.close.w as i32 <= g.panel.x + g.panel.w as i32);

        let r = FindGeometry::new(1200, 30, CELL, true);
        let rg = r.replace.unwrap();
        assert!(r.panel.h > g.panel.h);
        let (x, y) = mid(rg.all);
        assert_eq!(r.hit(x, y), Some(FindHit::ReplaceAll));
        let (x, y) = mid(rg.preserve);
        assert_eq!(r.hit(x, y), Some(FindHit::Preserve));
        assert!(rg.all.x + rg.all.w as i32 <= r.panel.x + r.panel.w as i32);
    }

    #[test]
    fn narrow_windows_shrink_the_fields() {
        let wide = FindGeometry::new(1200, 30, CELL, false);
        let narrow = FindGeometry::new(500, 30, CELL, false);
        assert!(narrow.text.w < wide.text.w);
        assert!(narrow.text.w >= 5 * CELL.width);
    }

    #[test]
    fn drawing_shows_text_toggles_count_and_messages() {
        let theme = Theme::DARK;
        let g = FindGeometry::new(1200, 30, CELL, true);
        let frame = FindFrame {
            cell: CELL,
            theme: &theme,
            find: FindField {
                text: "a\nb",
                caret: 3,
                selection: (3, 3),
                focused: true,
            },
            replace: FindField {
                text: "",
                caret: 0,
                selection: (0, 0),
                focused: false,
            },
            toggles: [false, false, false, true],
            preserve: false,
            in_selection: false,
            count: "2 of 9",
            no_results: false,
            message: Some(("Unclosed group at 1", Severity::Error)),
        };
        let mut out = Layer::default();
        draw_find(&g, &frame, &mut out);
        let text: String = out.glyphs.iter().map(|g| g.ch).collect();
        assert!(text.contains(&format!("a{LINE_BREAK}b")), "{text}");
        assert!(text.contains(".*"));
        assert!(text.contains("2of9"));
        assert!(text.contains("Replace"));
        assert!(text.contains("Unclosedgroupat1"));
        assert!(out.fills.iter().any(|f| f.color == theme.error_background));
        assert!(out.fills.iter().any(|f| f.color == theme.popup_selected));
    }
}
