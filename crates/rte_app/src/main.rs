//! RoyiTextEditor: single-file, keyboard-driven text editor.
//!
//! Startup overlaps work: the loader and glyph threads start first, the window
//! is created meanwhile, and the window is shown once its first frame is ready.

#![windows_subsystem = "windows"]

mod bench;
mod clip;
mod commands;
mod config;
mod document;
mod editor;
mod favorites;
mod fuzzy;
mod loader;
mod palette;
mod save;
mod search;
mod syntax;
mod trace;

use std::borrow::Cow;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

use rte_platform::{Waker, Window, WindowOptions};
use rte_render::Renderer;

use editor::{BASE_PX, Editor, Settings, Wake, window_title};
use save::SystemFiles;
use syntax::EarlySyntax;
use trace::Trace;

/// Initial client size at 96 DPI.
const WINDOW_SIZE: (u32, u32) = (1100, 750);

#[allow(unsafe_code)]
fn install_allocator() {
    // SAFETY: the first call of `main`, before any other thread or
    // tree-sitter use.
    unsafe { rte_syntax::install_allocator() }
}

struct Args {
    path: Option<PathBuf>,
    read_only: bool,
    /// `--syntax <id>`: language id overriding detection by file name;
    /// `none` or `off` (any case) disables highlighting.
    syntax: Option<String>,
    trace_startup: bool,
    exit_after_first_frame: bool,
    exit_after_load: bool,
    /// `--exit-after-highlight`: also wait for the full highlight.
    exit_after_highlight: bool,
    /// `--bench-keys <keys>`, `--bench-runs <n>`, `--bench-setup <keys>`,
    /// `--bench-select <start>..<end>`, `--bench-handled`, `--bench-paced` and
    /// `--bench-full`: scripted input benchmark.
    bench_keys: Option<String>,
    bench_runs: Option<String>,
    bench_setup: Option<String>,
    bench_select: Option<String>,
    bench_handled: bool,
    bench_paced: bool,
    bench_full: bool,
}

fn parse_args(raw: impl IntoIterator<Item = OsString>) -> Args {
    let mut args = Args {
        path: None,
        read_only: false,
        syntax: None,
        trace_startup: false,
        exit_after_first_frame: false,
        exit_after_load: false,
        exit_after_highlight: false,
        bench_keys: None,
        bench_runs: None,
        bench_setup: None,
        bench_select: None,
        bench_handled: false,
        bench_paced: false,
        bench_full: false,
    };
    let mut raw = raw.into_iter();
    while let Some(arg) = raw.next() {
        match arg.to_str() {
            Some("--read-only") => args.read_only = true,
            Some("--syntax") => args.syntax = raw.next().and_then(|id| id.into_string().ok()),
            Some("--trace-startup") => args.trace_startup = true,
            Some("--exit-after-first-frame") => args.exit_after_first_frame = true,
            Some("--exit-after-load") => args.exit_after_load = true,
            Some("--exit-after-highlight") => args.exit_after_highlight = true,
            Some("--bench-keys") => args.bench_keys = raw.next().and_then(|k| k.into_string().ok()),
            Some("--bench-runs") => args.bench_runs = raw.next().and_then(|n| n.into_string().ok()),
            Some("--bench-setup") => {
                args.bench_setup = raw.next().and_then(|k| k.into_string().ok());
            }
            Some("--bench-select") => {
                args.bench_select = raw.next().and_then(|r| r.into_string().ok());
            }
            Some("--bench-handled") => args.bench_handled = true,
            Some("--bench-paced") => args.bench_paced = true,
            Some("--bench-full") => args.bench_full = true,
            _ => args.path = Some(PathBuf::from(arg)),
        }
    }
    args
}

impl Args {
    fn highlighting(&self) -> bool {
        !self
            .syntax
            .as_deref()
            .is_some_and(|id| id.eq_ignore_ascii_case("none") || id.eq_ignore_ascii_case("off"))
    }
}

fn main() {
    install_allocator();
    let args = parse_args(std::env::args_os().skip(1));
    let bench = match &args.bench_keys {
        None => None,
        Some(keys) => match bench::parse(
            keys,
            args.bench_runs.as_deref().unwrap_or("1"),
            args.bench_setup.as_deref().unwrap_or(""),
        )
        .and_then(|mut script| {
            script.select = args
                .bench_select
                .as_deref()
                .map(bench::parse_range)
                .transpose()?;
            script.handled = args.bench_handled;
            script.paced = args.bench_paced;
            script.full = args.bench_full;
            Ok(script)
        }) {
            Ok(script) => Some(script),
            Err(e) => {
                eprintln!("--bench-keys: {e}");
                std::process::exit(2);
            }
        },
    };
    let mut trace = Trace::new(args.trace_startup);
    trace.mark("main");
    let dpi = rte_platform::system_dpi();

    let waker: Arc<OnceLock<Waker>> = Arc::new(OnceLock::new());
    // The configuration and user state live beside the executable (portable).
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(PathBuf::from));
    let (config, config_problem) = exe_dir.as_ref().map_or_else(Default::default, |dir| {
        config::load(&dir.join(config::FILE_NAME))
    });
    let early = args
        .highlighting()
        .then(|| EarlySyntax::new(Mutex::new(Default::default())));
    let chosen = args.syntax.as_deref().and_then(rte_syntax::by_id);
    // The highlight query compiles while the file is read (a `--syntax`
    // choice applies only to the command-line file).
    if let Some(path) = args.path.as_deref()
        && let Some(lang) = chosen.or_else(|| rte_syntax::detect(path, b""))
        && early.is_some()
    {
        rte_syntax::warm(lang.grammar);
    }
    // Events sent before the window exists find no waker; `Editor::new` drains them.
    let wake: Wake = {
        let waker = waker.clone();
        Arc::new(move || waker.get().map_or((), Waker::wake))
    };
    let loads = args.path.clone().map(|path| {
        let on_done = early.clone().map(|slot| {
            let (max, injections) = (config.syntax_max_mib, config.injections);
            syntax::early_start(slot, path.clone(), chosen, max, injections, wake.clone())
        });
        let wake = wake.clone();
        loader::spawn(path, move || wake(), on_done, true)
    });
    let px = (BASE_PX * dpi as f32 / 96.0).round();
    let glyphs = thread::spawn(move || {
        let mut renderer = Renderer::new_with_bold(
            Cow::Borrowed(rte_assets::DEFAULT_FONT),
            Cow::Borrowed(rte_assets::BOLD_FONT),
            px,
        )
        .expect("embedded fonts are valid");
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

    let first = loads.as_ref().and_then(|load| load.events.recv().ok());
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
            exit_after_highlight: args.exit_after_highlight,
            recent_count: config.recent_count,
            multi_cursor_limit: config.multi_cursor_limit,
            replace_all_max_mib: config.replace_all_max_mib,
            user_dir: exe_dir.map(|dir| dir.join("UserSettings")),
            notice: config_problem,
            highlighting: args.highlighting(),
            syntax: args.syntax.clone(),
            syntax_max_mib: config.syntax_max_mib,
            injections: config.injections,
            early_syntax: early,
            bench,
        },
        trace,
        Arc::new(SystemFiles),
        wake,
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

    #[test]
    fn syntax_none_or_off_disables_highlighting() {
        assert!(parse(&["a.txt"]).highlighting());
        assert!(parse(&["--syntax", "json"]).highlighting());
        for id in ["none", "None", "OFF", "off"] {
            assert!(!parse(&["--syntax", id, "a.txt"]).highlighting(), "{id}");
        }
    }
}
