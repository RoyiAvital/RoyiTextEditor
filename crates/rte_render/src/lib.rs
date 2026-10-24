//! CPU renderer: rasterizes glyphs with `swash` into a coverage cache and draws
//! [`DisplayList`]s into a `0x00RRGGBB` framebuffer.

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;

use rte_view::{Cell, Color, DisplayList, Glyph, Image, Layer, Rect};
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
    /// Glyph of each non-ASCII character seen; characters sharing a glyph
    /// (e.g. the missing-glyph box) share one bitmap.
    glyph_ids: HashMap<char, u16>,
    glyphs: HashMap<u16, Option<Bitmap>>,
    /// Star coverage by side and fill.
    stars: HashMap<(u32, bool), Bitmap>,
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
            glyph_ids: HashMap::new(),
            glyphs: HashMap::new(),
            stars: HashMap::new(),
            contrast,
        };
        renderer.apply_size(px);
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

    /// Changes the pixel size; a new size clears the glyph cache.
    pub fn set_size(&mut self, px: f32) {
        if px != self.px {
            self.apply_size(px);
        }
    }

    fn apply_size(&mut self, px: f32) {
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
        self.glyph_ids.clear();
        self.glyphs.clear();
    }

    /// Rasterizes printable ASCII ahead of the first frame.
    pub fn warm_ascii(&mut self) {
        for ch in ' '..='~' {
            self.bitmap(ch);
        }
    }

    fn bitmap(&mut self, ch: char) -> Option<&Bitmap> {
        let code = ch as usize;
        if code < 128 {
            if self.ascii[code].is_none() {
                let glyph = self.font().charmap().map(ch);
                self.ascii[code] = Some(self.rasterize(glyph).unwrap_or_else(empty_bitmap));
            }
            return self.ascii[code].as_ref().filter(|b| b.width > 0);
        }
        let glyph = match self.glyph_ids.get(&ch) {
            Some(&glyph) => glyph,
            None => {
                let glyph = self.font().charmap().map(ch);
                self.glyph_ids.insert(ch, glyph);
                glyph
            }
        };
        if !self.glyphs.contains_key(&glyph) {
            let bitmap = self.rasterize(glyph);
            self.glyphs.insert(glyph, bitmap);
        }
        self.glyphs.get(&glyph).and_then(Option::as_ref)
    }

    fn rasterize(&mut self, glyph: u16) -> Option<Bitmap> {
        let font = FontRef {
            data: &self.data,
            offset: self.offset,
            key: self.key,
        };
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

    /// Draws `list` into `target`, except its popup layer.
    pub fn draw(&mut self, list: &DisplayList, target: &mut Target<'_>) {
        for fill in &list.fills {
            fill_rect(target, fill.rect, fill.color);
        }
        self.draw_glyphs(&list.glyphs, target);
        for fill in &list.overlays {
            fill_rect(target, fill.rect, fill.color);
        }
    }

    /// Draws a popup layer above what `target` holds.
    pub fn draw_layer(&mut self, popup: &Layer, target: &mut Target<'_>) {
        for fill in &popup.fills {
            fill_rect(target, fill.rect, fill.color);
        }
        for image in &popup.images {
            draw_image(target, image);
        }
        for star in &popup.stars {
            let key = (star.rect.w.min(star.rect.h), star.filled);
            let bitmap = self
                .stars
                .entry(key)
                .or_insert_with(|| star_bitmap(key.0, key.1));
            blit(
                target,
                bitmap,
                star.rect.x,
                star.rect.y,
                star.color,
                star.rect,
                &self.contrast,
            );
        }
        self.draw_glyphs(&popup.glyphs, target);
    }

    fn draw_glyphs(&mut self, glyphs: &[Glyph], target: &mut Target<'_>) {
        let baseline = self.baseline;
        let contrast = self.contrast;
        for glyph in glyphs {
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

/// Coverage of a five-pointed star in a `side` square; the outline is the
/// star minus a smaller concentric one.
fn star_bitmap(side: u32, filled: bool) -> Bitmap {
    const SAMPLES: u32 = 4;
    let r = side as f32 / 2.0;
    // The points span 1.809 r vertically; center that span.
    let (cx, cy) = (r, r + 0.0955 * r);
    let points = |scale: f32| -> [(f32, f32); 10] {
        std::array::from_fn(|k| {
            let angle = (-90.0 + 36.0 * k as f32).to_radians();
            let radius = scale * if k % 2 == 0 { r } else { 0.45 * r };
            (cx + radius * angle.cos(), cy + radius * angle.sin())
        })
    };
    let outer = points(1.0);
    // Inner edges sit 1.4 px inside the outer ones (edge distance about 0.33 r).
    let inner = points((1.0 - 1.4 / (0.33 * r.max(1.0))).clamp(0.0, 0.85));
    let mut coverage = vec![0u8; (side * side) as usize];
    for y in 0..side {
        for x in 0..side {
            let mut hits = 0;
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let px = x as f32 + (sx as f32 + 0.5) / SAMPLES as f32;
                    let py = y as f32 + (sy as f32 + 0.5) / SAMPLES as f32;
                    if inside(&outer, px, py) && (filled || !inside(&inner, px, py)) {
                        hits += 1;
                    }
                }
            }
            coverage[(y * side + x) as usize] = (hits * 255 / (SAMPLES * SAMPLES)) as u8;
        }
    }
    Bitmap {
        left: 0,
        top: 0,
        width: side,
        height: side,
        coverage: coverage.into_boxed_slice(),
    }
}

/// Even-odd point-in-polygon test.
fn inside(polygon: &[(f32, f32)], x: f32, y: f32) -> bool {
    let mut result = false;
    let mut j = polygon.len() - 1;
    for (i, &(xi, yi)) in polygon.iter().enumerate() {
        let (xj, yj) = polygon[j];
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            result = !result;
        }
        j = i;
    }
    result
}

/// Blends premultiplied image pixels over the target.
fn draw_image(target: &mut Target<'_>, image: &Image) {
    if image.pixels.len() < (image.width * image.height) as usize {
        return;
    }
    let stride = target.width as usize;
    for iy in 0..image.height as i32 {
        let y = image.y + iy;
        if y < 0 || y >= target.height as i32 {
            continue;
        }
        for ix in 0..image.width as i32 {
            let x = image.x + ix;
            if x < 0 || x >= target.width as i32 {
                continue;
            }
            let s = image.pixels[(iy * image.width as i32 + ix) as usize];
            let a = s >> 24;
            if a == 0 {
                continue;
            }
            let d = &mut target.pixels[y as usize * stride + x as usize];
            let mix = |shift: u32| {
                let (sc, dc) = ((s >> shift) & 0xFF, (*d >> shift) & 0xFF);
                (sc + (dc * (255 - a) + 127) / 255).min(255) << shift
            };
            *d = OPAQUE | mix(16) | mix(8) | mix(0);
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
    use rte_view::{
        CaretStyle, Chrome, Fill, Frame, Glyph, Layout, Options, Scroll, Theme, ViewBuilder,
    };

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
    fn missing_characters_share_one_bitmap_and_sizes_keep_caches() {
        let mut r = renderer();
        for code in 0x2_0000..0x2_0000 + 1000 {
            let ch = char::from_u32(code).unwrap();
            assert!(r.bitmap(ch).is_some(), "the missing-glyph box is drawn");
        }
        assert_eq!(r.glyph_ids.len(), 1000);
        assert_eq!(r.glyphs.len(), 1);
        r.set_size(r.px());
        assert_eq!(r.glyphs.len(), 1, "an unchanged size keeps the cache");
        r.set_size(r.px() + 1.0);
        assert!(r.glyphs.is_empty());
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
    fn popup_layer_covers_the_text_and_draws_stars_and_images() {
        let mut r = renderer();
        let (w, h) = (40u32, 40u32);
        let mut pixels = vec![0u32; (w * h) as usize];
        let all = Rect { x: 0, y: 0, w, h };
        let mut list = DisplayList::default();
        list.glyphs.push(Glyph {
            x: 0,
            y: 0,
            ch: 'M',
            color: 0xFFFFFF,
            clip: all,
        });
        list.popup.fills.push(Fill {
            rect: all,
            color: 0x000000,
        });
        let star = |x, filled| rte_view::Star {
            rect: Rect {
                x,
                y: 0,
                w: 20,
                h: 20,
            },
            color: 0xFF0000,
            filled,
        };
        list.popup.stars.push(star(0, true));
        list.popup.stars.push(star(20, false));
        list.popup.images.push(Image {
            x: 0,
            y: 30,
            width: 2,
            height: 1,
            pixels: vec![0xFF00_FF00, 0x8000_4000].into(),
        });
        let mut target = Target {
            pixels: &mut pixels,
            width: w,
            height: h,
        };
        r.draw(&list, &mut target);
        r.draw_layer(&list.popup, &mut target);
        let at = |x: u32, y: u32| pixels[(y * w + x) as usize] & 0xFF_FFFF;
        // The filled star covers its center; the outline leaves it empty.
        assert_eq!(at(10, 10), 0xFF0000);
        assert_eq!(at(30, 10), 0x000000);
        assert!((0..20).any(|x| at(20 + x, 10) >> 16 > 0x80));
        assert_eq!(at(0, 30), 0x00FF00);
        assert_eq!(at(1, 30), 0x004000, "premultiplied blend over black");
        // The text glyph is hidden by the popup background.
        assert!((0..20).all(|y| (0..20).all(|x| at(x, y) & 0xFFFF == 0)));
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
                    selections: &[],
                    caret: CaretStyle::Hidden,
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
