//! RoyiTextEditor: single-file, keyboard-driven text editor.
//!
//! Startup overlaps work: the loader and glyph threads start first, the window
//! is created meanwhile, and the window is shown once its first frame is ready.

#![windows_subsystem = "windows"]

mod editor;
mod loader;
mod trace;

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::thread;

use rte_platform::{Waker, Window, WindowOptions};
use rte_render::Renderer;

use editor::{BASE_PX, Editor, Settings};
use trace::Trace;

/// Initial client size at 96 DPI.
const WINDOW_SIZE: (u32, u32) = (1100, 750);

struct Args {
    path: Option<PathBuf>,
    read_only: bool,
    trace_startup: bool,
    exit_after_first_frame: bool,
    exit_after_load: bool,
}

fn parse_args() -> Args {
    let mut args = Args {
        path: None,
        read_only: false,
        trace_startup: false,
        exit_after_first_frame: false,
        exit_after_load: false,
    };
    for arg in std::env::args_os().skip(1) {
        match arg.to_str() {
            Some("--read-only") => args.read_only = true,
            Some("--trace-startup") => args.trace_startup = true,
            Some("--exit-after-first-frame") => args.exit_after_first_frame = true,
            Some("--exit-after-load") => args.exit_after_load = true,
            _ => args.path = Some(PathBuf::from(arg)),
        }
    }
    args
}

fn main() {
    let args = parse_args();
    let mut trace = Trace::new(args.trace_startup);
    trace.mark("main");
    rte_platform::init_dpi_awareness();
    let dpi = rte_platform::system_dpi();

    let waker: Arc<OnceLock<Waker>> = Arc::new(OnceLock::new());
    // Events sent before the window exists find no waker; `Editor::new` drains them.
    let loads = args.path.clone().map(|path| {
        let waker = waker.clone();
        loader::spawn(path, move || waker.get().map_or((), Waker::wake))
    });
    let px = (BASE_PX * dpi as f32 / 96.0).round();
    let glyphs = thread::spawn(move || {
        let mut renderer = Renderer::new(Cow::Borrowed(rte_assets::DEFAULT_FONT), px)
            .expect("embedded font is valid");
        renderer.warm_ascii();
        renderer
    });

    let title = match &args.path {
        Some(p) => format!(
            "{} - RoyiTextEditor",
            p.file_name().map_or_else(
                || p.display().to_string(),
                |n| n.to_string_lossy().into_owned()
            )
        ),
        None => "RoyiTextEditor".to_owned(),
    };
    let window = match Window::create(&WindowOptions {
        title: &title,
        width: WINDOW_SIZE.0 * dpi / 96,
        height: WINDOW_SIZE.1 * dpi / 96,
    }) {
        Ok(window) => window,
        Err(_) => std::process::exit(1),
    };
    let _ = waker.set(window.waker());
    trace.mark("window_created");

    let first = loads.as_ref().and_then(|rx| rx.recv().ok());
    trace.mark("first_snapshot");
    let renderer = glyphs.join().expect("glyph thread");
    trace.mark("glyphs_ready");

    let editor = Editor::new(
        args.path.as_deref(),
        first,
        loads,
        renderer,
        window.dpi(),
        Settings {
            read_only: args.read_only,
            exit_after_first_frame: args.exit_after_first_frame,
            exit_after_load: args.exit_after_load,
        },
        trace,
    );
    std::process::exit(window.run(Box::new(editor)));
}
