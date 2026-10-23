//! CPU renderer: rasterizes glyphs with `swash` into a coverage cache and draws
//! [`DisplayList`]s into a `0x00RRGGBB` framebuffer.

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;

use rte_view::{Cell, Color, DisplayList, Rect};
use swash::scale::{Render, ScaleContext, Source};
use swash::zeno::Format;
use swash::{CacheKey, FontRef};

const OPAQUE: u32 = 0xFF00_0000;

/// Mutable view of a top-down framebuffer of `0xAARRGGBB` pixels. Every pixel
/// is written opaque: DWM honors alpha where the frame extends into the client.
pub struct Target<'a> {
    pub pixels: &'a mut [u32],
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontError;

impl fmt::Display for FontError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("font data is not a valid font")
    }
}

impl std::error::Error for FontError {}

struct Bitmap {
    left: i32,
    top: i32,
    width: u32,
    height: u32,
    coverage: Box<[u8]>,
}

/// Font face at one pixel size, with its glyph cache.
pub struct Renderer {
    data: Cow<'static, [u8]>,
    offset: u32,
    key: CacheKey,
    context: ScaleContext,
    px: f32,
    cell: Cell,
    baseline: i32,
    ascii: Vec<Option<Bitmap>>,
    other: HashMap<char, Option<Bitmap>>,
    contrast: [u8; 256],
}

impl Renderer {
    pub fn new(data: Cow<'static, [u8]>, px: f32) -> Result<Renderer, FontError> {
        let font = FontRef::from_index(&data, 0).ok_or(FontError)?;
        let (offset, key) = (font.offset, font.key);
        let mut contrast = [0u8; 256];
        for (i, c) in contrast.iter_mut().enumerate() {
            // Slightly thicker strokes compensate for blending in gamma space.
            *c = (255.0 * (i as f32 / 255.0).powf(0.8)).round() as u8;
        }
        let mut renderer = Renderer {
            data,
            offset,
            key,
            context: ScaleContext::new(),
            px,
            cell: Cell {
                width: 1,
                height: 1,
            },
            baseline: 0,
            ascii: Vec::new(),
            other: HashMap::new(),
            contrast,
        };
        renderer.set_size(px);
        Ok(renderer)
    }

    fn font(&self) -> FontRef<'_> {
        FontRef {
            data: &self.data,
            offset: self.offset,
            key: self.key,
        }
    }

    pub fn cell(&self) -> Cell {
        self.cell
    }

    pub fn px(&self) -> f32 {
        self.px
    }

    /// Changes the pixel size; clears the glyph cache.
    pub fn set_size(&mut self, px: f32) {
        self.px = px;
        let font = self.font();
        let metrics = font.metrics(&[]).scale(px);
        let zero = font.charmap().map('0');
        let advance = font.glyph_metrics(&[]).scale(px).advance_width(zero);
        let content = metrics.ascent + metrics.descent + metrics.leading;
        let height = (content * 1.1).round().max(1.0);
        self.cell = Cell {
            width: advance.round().max(1.0) as u32,
            height: height as u32,
        };
        self.baseline =
            (metrics.ascent + (height - metrics.ascent - metrics.descent) / 2.0).round() as i32;
        self.ascii = (0..128).map(|_| None).collect();
        self.other.clear();
    }

    /// Rasterizes printable ASCII ahead of the first frame.
    pub fn warm_ascii(&mut self) {
        for ch in ' '..='~' {
            self.bitmap(ch);
        }
    }

    fn bitmap(&mut self, ch: char) -> Option<&Bitmap> {
        let code = ch as usize;
        let cached = if code < 128 {
            self.ascii[code].is_some()
        } else {
            self.other.contains_key(&ch)
        };
        if !cached {
            let bitmap = self.rasterize(ch);
            if code < 128 {
                self.ascii[code] = Some(bitmap.unwrap_or_else(empty_bitmap));
            } else {
                self.other.insert(ch, bitmap);
            }
        }
        if code < 128 {
            self.ascii[code].as_ref().filter(|b| b.width > 0)
        } else {
            self.other.get(&ch).and_then(Option::as_ref)
        }
    }

    fn rasterize(&mut self, ch: char) -> Option<Bitmap> {
        let font = FontRef {
            data: &self.data,
            offset: self.offset,
            key: self.key,
        };
        let glyph = font.charmap().map(ch);
        let mut scaler = self.context.builder(font).size(self.px).hint(true).build();
        let image = Render::new(&[Source::Outline])
            .format(Format::Alpha)
            .render(&mut scaler, glyph)?;
        let p = image.placement;
        if p.width == 0 || p.height == 0 {
            return None;
        }
        Some(Bitmap {
            left: p.left,
            top: p.top,
            width: p.width,
            height: p.height,
            coverage: image.data.into_boxed_slice(),
        })
    }

    /// Draws `list` into `target`.
    pub fn draw(&mut self, list: &DisplayList, target: &mut Target<'_>) {
        for fill in &list.fills {
            fill_rect(target, fill.rect, fill.color);
        }
        let baseline = self.baseline;
        let contrast = self.contrast;
        for glyph in &list.glyphs {
            if let Some(bitmap) = self.bitmap(glyph.ch) {
                blit(
                    target,
                    bitmap,
                    glyph.x,
                    glyph.y + baseline,
                    glyph.color,
                    glyph.clip,
                    &contrast,
                );
            }
        }
    }
}

fn empty_bitmap() -> Bitmap {
    Bitmap {
        left: 0,
        top: 0,
        width: 0,
        height: 0,
        coverage: Box::default(),
    }
}

pub fn fill_rect(target: &mut Target<'_>, rect: Rect, color: Color) {
    let color = color | OPAQUE;
    let x0 = rect.x.clamp(0, target.width as i32) as usize;
    let y0 = rect.y.clamp(0, target.height as i32) as usize;
    let x1 = (rect.x + rect.w as i32).clamp(0, target.width as i32) as usize;
    let y1 = (rect.y + rect.h as i32).clamp(0, target.height as i32) as usize;
    let stride = target.width as usize;
    for y in y0..y1 {
        target.pixels[y * stride + x0..y * stride + x1].fill(color);
    }
}

/// Blends a coverage bitmap whose origin (pen position) is `(x, baseline)`,
/// drawing only inside `clip` and the target.
fn blit(
    target: &mut Target<'_>,
    bitmap: &Bitmap,
    x: i32,
    baseline: i32,
    color: Color,
    clip: Rect,
    contrast: &[u8; 256],
) {
    let left = x + bitmap.left;
    let top = baseline - bitmap.top;
    let (w, h) = (bitmap.width as i32, bitmap.height as i32);
    let x0 = clip.x.max(0);
    let y0 = clip.y.max(0);
    let x1 = clip
        .x
        .saturating_add_unsigned(clip.w)
        .min(target.width as i32);
    let y1 = clip
        .y
        .saturating_add_unsigned(clip.h)
        .min(target.height as i32);
    let cx0 = (x0 - left).max(0);
    let cy0 = (y0 - top).max(0);
    let cx1 = w.min(x1 - left);
    let cy1 = h.min(y1 - top);
    if cx0 >= cx1 || cy0 >= cy1 {
        return;
    }
    let (sr, sg, sb) = ((color >> 16) & 0xFF, (color >> 8) & 0xFF, color & 0xFF);
    let stride = target.width as usize;
    for gy in cy0..cy1 {
        let src = &bitmap.coverage[(gy * w + cx0) as usize..(gy * w + cx1) as usize];
        let row = (top + gy) as usize * stride + (left + cx0) as usize;
        let dst = &mut target.pixels[row..row + src.len()];
        for (d, &a) in dst.iter_mut().zip(src) {
            let a = u32::from(contrast[a as usize]);
            if a == 0 {
                continue;
            }
            if a == 255 {
                *d = color | OPAQUE;
                continue;
            }
            let (dr, dg, db) = ((*d >> 16) & 0xFF, (*d >> 8) & 0xFF, *d & 0xFF);
            let mix = |s: u32, d: u32| (s * a + d * (255 - a) + 127) / 255;
            *d = OPAQUE | (mix(sr, dr) << 16) | (mix(sg, dg) << 8) | mix(sb, db);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rte_text::Snapshot;
    use rte_view::{Chrome, Fill, Frame, Glyph, Layout, Options, Scroll, Theme, ViewBuilder};

    fn renderer() -> Renderer {
        Renderer::new(Cow::Borrowed(rte_assets::DEFAULT_FONT), 16.0).unwrap()
    }

    #[test]
    fn every_bundled_face_loads_and_draws() {
        let fonts = std::path::Path::new(rte_assets::FONTS_DIR);
        let mut faces = 0;
        for family in std::fs::read_dir(fonts).unwrap() {
            for file in std::fs::read_dir(family.unwrap().path()).unwrap() {
                let path = file.unwrap().path();
                if path.extension().is_none_or(|e| e != "ttf") {
                    continue;
                }
                let mut r = Renderer::new(Cow::Owned(std::fs::read(&path).unwrap()), 16.0)
                    .unwrap_or_else(|_| panic!("{} does not load", path.display()));
                assert!(
                    (7..=12).contains(&r.cell().width),
                    "{}: {:?}",
                    path.display(),
                    r.cell()
                );
                assert!(r.bitmap('M').is_some(), "{} has no 'M'", path.display());
                faces += 1;
            }
        }
        assert!(faces >= 18, "only {faces} faces found");
    }

    #[test]
    fn monospaced_cell_metrics() {
        let r = renderer();
        assert!(
            r.cell().width >= 8 && r.cell().width <= 11,
            "{:?}",
            r.cell()
        );
        assert!(
            r.cell().height >= 18 && r.cell().height <= 26,
            "{:?}",
            r.cell()
        );
    }

    #[test]
    fn draws_glyph_pixels_inside_its_cell() {
        let mut r = renderer();
        r.warm_ascii();
        let cell = r.cell();
        let (w, h) = (cell.width * 3, cell.height);
        let mut pixels = vec![0u32; (w * h) as usize];
        let mut list = DisplayList::default();
        list.fills.push(Fill {
            rect: Rect { x: 0, y: 0, w, h },
            color: 0x000000,
        });
        list.glyphs.push(Glyph {
            x: cell.width as i32,
            y: 0,
            ch: 'M',
            color: 0xFFFFFF,
            clip: Rect { x: 0, y: 0, w, h },
        });
        r.draw(
            &list,
            &mut Target {
                pixels: &mut pixels,
                width: w,
                height: h,
            },
        );
        let lit: Vec<u32> = (0..w * h)
            .filter(|&i| pixels[i as usize] != OPAQUE)
            .collect();
        assert!(!lit.is_empty());
        assert!(
            lit.iter()
                .all(|&i| (cell.width..2 * cell.width).contains(&(i % w)))
        );
    }

    #[test]
    fn clipped_drawing_never_panics() {
        let mut r = renderer();
        let mut pixels = vec![0u32; 4 * 4];
        let mut list = DisplayList::default();
        list.fills.push(Fill {
            rect: Rect {
                x: -5,
                y: -5,
                w: 100,
                h: 100,
            },
            color: 0x123456,
        });
        for (x, y) in [(-20, -20), (2, 2), (3, -10), (100, 100)] {
            list.glyphs.push(Glyph {
                x,
                y,
                ch: 'W',
                color: 0xFFFFFF,
                clip: Rect {
                    x: -50,
                    y: -50,
                    w: u32::MAX,
                    h: u32::MAX,
                },
            });
        }
        r.draw(
            &list,
            &mut Target {
                pixels: &mut pixels,
                width: 4,
                height: 4,
            },
        );
    }

    #[test]
    fn glyphs_stay_inside_their_region() {
        let mut r = renderer();
        r.warm_ascii();
        let cell = r.cell();
        let options = Options::default();
        let snapshot = Snapshot::from_bytes("Wjgy_|\n".repeat(40).as_bytes());
        let theme = Theme::DARK;
        // Heights where the last text row overlaps the status strip.
        for height in 200..220 {
            let width = 400;
            let layout = Layout::new(width, height, cell, snapshot.line_count(), &options);
            let mut list = DisplayList::default();
            ViewBuilder::default().build(
                &Frame {
                    snapshot: &snapshot,
                    layout: &layout,
                    scroll: Scroll::default(),
                    cell,
                    theme: &theme,
                    chrome: Chrome::default(),
                    options: &options,
                },
                &mut list,
            );
            let mut pixels = vec![0u32; (width * height) as usize];
            r.draw(
                &list,
                &mut Target {
                    pixels: &mut pixels,
                    width,
                    height,
                },
            );
            let s = layout.status;
            for y in s.y..s.y + s.h as i32 {
                for x in s.x..s.x + s.w as i32 {
                    assert_eq!(
                        pixels[(y as u32 * width + x as u32) as usize],
                        theme.status_background | OPAQUE,
                        "text drawn into the status strip at ({x}, {y}), height {height}"
                    );
                }
            }
        }
    }
}
