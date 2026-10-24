//! Full-frame CPU cost (layout + draw, without presentation) for a screen of
//! code at 1080p and 4K. Run: `cargo bench -p rte_render`.

use std::borrow::Cow;

use divan::Bencher;
use rte_edit::Selection;
use rte_render::{Renderer, Target};
use rte_text::Snapshot;
use rte_view::{
    CaretStyle, Chrome, DisplayList, Frame, Layout, Options, Scroll, Theme, ViewBuilder,
};

static FONT: &[u8] = rte_assets::DEFAULT_FONT;

fn main() {
    divan::main();
}

fn code() -> Snapshot {
    let line = "    let value = compute(index, &table[offset..]); // comment with more text here\n";
    Snapshot::from_bytes(line.repeat(5000).as_bytes())
}

/// Screen sizes with the font size scaled like 100% and 200% DPI.
#[divan::bench(args = [(1920, 1080, 14.0), (3840, 2160, 28.0)])]
fn full_frame(bencher: Bencher, (width, height, px): (u32, u32, f32)) {
    frames(bencher, &code(), width, height, px);
}

/// Adversarial rows: one letter followed by 32,760 combining accents (the
/// 64 KiB row scan limit), at 1080p.
#[divan::bench]
fn combining_marks(bencher: Bencher) {
    let mut line = "a".to_string();
    line.push_str(&"\u{301}".repeat(32_760));
    line.push('\n');
    frames(
        bencher,
        &Snapshot::from_bytes(line.repeat(4100).as_bytes()),
        1920,
        1080,
        14.0,
    );
}

fn frames(bencher: Bencher, snapshot: &Snapshot, width: u32, height: u32, px: f32) {
    let mut renderer = Renderer::new(Cow::Borrowed(FONT), px).unwrap();
    renderer.warm_ascii();
    let cell = renderer.cell();
    let options = Options::default();
    let layout = Layout::new(width, height, cell, snapshot.line_count(), &options);
    let mut view = ViewBuilder::default();
    let mut list = DisplayList::default();
    let mut pixels = vec![0u32; (width * height) as usize];
    let mut top = 0;
    bencher.bench_local(|| {
        top = (top + 1) % 4000;
        let frame = Frame {
            snapshot,
            layout: &layout,
            scroll: Scroll {
                top_line: top,
                first_col: 0,
            },
            cell,
            theme: &Theme::DARK,
            chrome: Chrome {
                title: "C:\\Projects\\sample.rs",
                title_mark: Some(Theme::DARK.unsaved),
                app_name: "RoyiTextEditor",
                status_left: "5000 lines",
                status_right: "UTF-8 · CRLF",
                prompt: false,
            },
            options: &options,
            // Every visible row is partly selected; one caret.
            selections: &[Selection::new(100, 300_000)],
            caret: CaretStyle::Bar,
            highlights: Default::default(),
        };
        view.build(&frame, &mut list);
        renderer.draw(
            &list,
            &mut Target {
                pixels: &mut pixels,
                width,
                height,
            },
        );
    });
}
