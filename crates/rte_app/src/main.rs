//! RoyiTextEditor: single-file, keyboard-driven text editor.
//!
//! Startup overlaps work: the loader and glyph threads start first, the window
//! is created meanwhile, and the window is shown once its first frame is ready.

#![windows_subsystem = "windows"]

mod clip;
mod document;
mod editor;
mod loader;
mod save;
mod trace;

use std::borrow::Cow;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::thread;

use rte_platform::{Waker, Window, WindowOptions};
use rte_render::Renderer;

use editor::{BASE_PX, Editor, Settings, window_title};
use save::SystemFiles;
use trace::Trace;

/// Initial client size at 96 DPI.
const WINDOW_SIZE: (u32, u32) = (1100, 750);

struct Args {
    path: Option<PathBuf>,
    read_only: bool,
    /// `--syntax <id>`: language id overriding detection by file name.
    syntax: Option<String>,
    trace_startup: bool,
    exit_after_first_frame: bool,
    exit_after_load: bool,
}

fn parse_args(raw: impl IntoIterator<Item = OsString>) -> Args {
    let mut args = Args {
        path: None,
        read_only: false,
        syntax: None,
        trace_startup: false,
        exit_after_first_frame: false,
        exit_after_load: false,
    };
    let mut raw = raw.into_iter();
    while let Some(arg) = raw.next() {
        match arg.to_str() {
            Some("--read-only") => args.read_only = true,
            Some("--syntax") => args.syntax = raw.next().and_then(|id| id.into_string().ok()),
            Some("--trace-startup") => args.trace_startup = true,
            Some("--exit-after-first-frame") => args.exit_after_first_frame = true,
            Some("--exit-after-load") => args.exit_after_load = true,
            _ => args.path = Some(PathBuf::from(arg)),
        }
    }
    args
}

fn main() {
    let args = parse_args(std::env::args_os().skip(1));
    let mut trace = Trace::new(args.trace_startup);
    trace.mark("main");
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

    let title = window_title(args.path.as_deref(), false);
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
        Arc::new(SystemFiles),
        Arc::new(move || waker.get().map_or((), Waker::wake)),
    );
    std::process::exit(window.run(Box::new(editor)));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(raw: &[&str]) -> Args {
        parse_args(raw.iter().map(OsString::from))
    }

    #[test]
    fn syntax_takes_the_next_argument_as_its_id() {
        let args = parse(&["--syntax", "json", "a.ipynb", "--read-only"]);
        assert_eq!(args.syntax.as_deref(), Some("json"));
        assert_eq!(args.path, Some(PathBuf::from("a.ipynb")));
        assert!(args.read_only);

        let args = parse(&["a.ipynb", "--syntax"]);
        assert_eq!(args.syntax, None);
        assert_eq!(args.path, Some(PathBuf::from("a.ipynb")));
    }
}
