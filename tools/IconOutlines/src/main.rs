//! Icon helpers for Meta/Icon.svg. Not part of the build.
//!
//! Text to SVG outlines, one `<path>` per glyph (the names in Meta/Icon.svg):
//! cargo run --release --manifest-path tools/IconOutlines/Cargo.toml -- glyphs
//!     assets/fonts/JetBrainsMono/JetBrainsMono-Bold.ttf 34 34.2
//!     105.2,140.3,175.3,210.3 Boaz,Gideon,Renana,Royi target/paths.txt
//! (font, size px, left x, baselines, words, output file)
//!
//! Preview sheet at 16-256 px: ... -- render Meta/Icon.svg target/icon-sheet.png

use resvg::tiny_skia::{Pixmap, Transform};
use std::fmt::Write;

struct Path {
    d: String,
    x: f32,
    y: f32,
    scale: f32,
}

impl Path {
    fn p(&self, x: f32, y: f32) -> (f32, f32) {
        (self.x + x * self.scale, self.y - y * self.scale)
    }
}

impl ttf_parser::OutlineBuilder for Path {
    fn move_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.p(x, y);
        write!(self.d, "M{x:.1} {y:.1}").unwrap();
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.p(x, y);
        write!(self.d, "L{x:.1} {y:.1}").unwrap();
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (x1, y1) = self.p(x1, y1);
        let (x, y) = self.p(x, y);
        write!(self.d, "Q{x1:.1} {y1:.1} {x:.1} {y:.1}").unwrap();
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (x1, y1) = self.p(x1, y1);
        let (x2, y2) = self.p(x2, y2);
        let (x, y) = self.p(x, y);
        write!(self.d, "C{x1:.1} {y1:.1} {x2:.1} {y2:.1} {x:.1} {y:.1}").unwrap();
    }
    fn close(&mut self) {
        self.d.push('Z');
    }
}

fn glyphs(font: &str, size: f32, x: f32, baselines: &[f32], words: &[&str], out: &str) {
    let mut text = String::new();
    let data = std::fs::read(font).unwrap();
    let face = ttf_parser::Face::parse(&data, 0).unwrap();
    let scale = size / face.units_per_em() as f32;
    let cap = face.capital_height().unwrap_or(0) as f32 * scale;
    eprintln!("cap height {cap:.1}, advance {:.1}", face.glyph_hor_advance(face.glyph_index('a').unwrap()).unwrap() as f32 * scale);
    for (word, &base) in words.iter().zip(baselines) {
        let mut path = Path { d: String::new(), x, y: base, scale };
        for c in word.chars() {
            let id = face.glyph_index(c).unwrap();
            face.outline_glyph(id, &mut path);
            writeln!(text, "    <path d=\"{}\"/>", path.d).unwrap();
            path.d.clear();
            path.x += face.glyph_hor_advance(id).unwrap() as f32 * scale;
        }
    }
    std::fs::write(out, text).unwrap();
}

fn render(svg: &str, out: &str) {
    let text = std::fs::read_to_string(svg).unwrap();
    let tree = resvg::usvg::Tree::from_str(&text, &resvg::usvg::Options::default()).unwrap();
    let sizes = [16u32, 24, 32, 48, 64, 96, 256];
    let mut sheet = Pixmap::new(256 * sizes.len() as u32, 256).unwrap();
    sheet.fill(resvg::tiny_skia::Color::from_rgba8(200, 200, 200, 255));
    for (k, &s) in sizes.iter().enumerate() {
        let mut p = Pixmap::new(s, s).unwrap();
        let f = s as f32 / tree.size().width();
        resvg::render(&tree, Transform::from_scale(f, f), &mut p.as_mut());
        let zoom = 256 / s;
        for y in 0..256u32.min(s * zoom) {
            for x in 0..256u32.min(s * zoom) {
                let px = p.pixel(x / zoom, y / zoom).unwrap();
                let i = (y * sheet.width() + k as u32 * 256 + x) as usize;
                let bg = sheet.pixels()[i];
                let a = px.alpha() as u32;
                let mix = |c: u8, b: u8| ((c as u32 * 255 + b as u32 * (255 - a)) / 255) as u8;
                let pm = resvg::tiny_skia::PremultipliedColorU8::from_rgba(
                    mix(px.red(), bg.red()),
                    mix(px.green(), bg.green()),
                    mix(px.blue(), bg.blue()),
                    255,
                )
                .unwrap();
                sheet.pixels_mut()[i] = pm;
            }
        }
    }
    sheet.save_png(out).unwrap();
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    match a[1].as_str() {
        "glyphs" => {
            let size: f32 = a[3].parse().unwrap();
            let x: f32 = a[4].parse().unwrap();
            let baselines: Vec<f32> = a[5].split(',').map(|v| v.parse().unwrap()).collect();
            let words: Vec<&str> = a[6].split(',').collect();
            glyphs(&a[2], size, x, &baselines, &words, &a[7]);
        }
        "render" => render(&a[2], &a[3]),
        _ => panic!(),
    }
}
