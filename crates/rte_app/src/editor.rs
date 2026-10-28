//! The editor window: input, view state, prompts and saving around a
//! [`Document`], plus the command palette and panels drawn above it.

mod code;
mod cursors;
mod find;
mod goto;
mod lifecycle;

use std::borrow::Cow;
use std::cell::RefCell;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

use rte_edit::{self as edit, Command, EditOptions, Selection, Selections};
use rte_platform::{App, Ctx, Dropped, Event, Key};
use rte_render::{Renderer, Target};
use rte_syntax::Lang;
use rte_text::{Encoding, LineEnding, Snapshot};
use rte_view::{
    CaretStyle, Cell, Chrome, DisplayList, Frame, Highlights, Layout, Options, PaletteFrame,
    PaletteGeometry, PaletteRow, PanelFrame, PanelGeometry, PanelLine, ROW_SCAN_LIMIT, Scroll,
    Theme, Tone, ViewBuilder, draw_palette, draw_panel, hit_test,
};
use unicode_width::UnicodeWidthStr;

use crate::bench;
use crate::clip;
use crate::commands::{
    self, Action, COMMANDS, CommandDef, EDITING_KEYS, key_name, vk::*, works_without_file,
};
use crate::document::{Document, Locks};
use crate::favorites::{self, FileStorage, MAX_FAVORITES, Store};
use crate::loader::{self, FileInfo, Load, LoadEvent, display_name};
use crate::palette::{Item, Mode, Motion, Palette};
use crate::save::{self, Baseline, FileOps, SaveOutcome, SaveRequest};
use crate::search::SearchThread;
use crate::syntax::{EarlySyntax, Syntax};
use crate::trace::Trace;

use cursors::Column;
use find::{FindBox, PendingOp};

/// Font size in pixels at 96 DPI and zoom 0.
pub const BASE_PX: f32 = 14.0;
const WHEEL_NOTCH: i32 = 120;
const WHEEL_LINES: i32 = 3;
const WHEEL_COLUMNS: i32 = 6;
/// Columns kept visible beside the caret when scrolling horizontally.
const REVEAL_MARGIN: u64 = 4;
/// Selections up to this size show a character count.
const COUNT_LIMIT: u64 = 64 * 1024;
/// Shown in the body of the empty state.
const EMPTY_HINT: &str = "Drop a file here to open it";

/// Wakes the window thread from worker threads.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// What follows once unsaved changes are settled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Exit {
    /// `Application: Quit` or the window close request.
    Quit,
    /// `File: Close`: back to the empty state.
    CloseFile,
}

/// A dropped file loading while the current state stays: it replaces the
/// current file only once its loader confirms it is a file.
struct PendingOpen {
    path: PathBuf,
    load: Load,
    /// Shown once the file is open.
    notice: Option<String>,
}

enum LoadStatus {
    Loading { loaded: u64, total: u64 },
    Ready,
    Failed(String),
}

pub struct Settings {
    pub read_only: bool,
    pub exit_after_first_frame: bool,
    pub exit_after_load: bool,
    /// `--exit-after-highlight`: like `exit_after_load`, and a code file
    /// also waits until it is highlighted from a parse of the whole file.
    pub exit_after_highlight: bool,
    /// Recent commands listed by `Ctrl+P` (configuration).
    pub recent_count: usize,
    /// Most cursors select-all may create (configuration).
    pub multi_cursor_limit: usize,
    /// Memory Replace All may use, in MiB (configuration).
    pub replace_all_max_mib: u64,
    /// Folder of the palette state file; `None` keeps it in memory.
    pub user_dir: Option<PathBuf>,
    /// Shown on the status line at startup (configuration problems).
    pub notice: Option<String>,
    /// `false` after `--syntax none` / `--syntax off`.
    pub highlighting: bool,
    /// `--syntax <id>`: language chosen instead of detection.
    pub syntax: Option<String>,
    /// Larger files are not highlighted (`syntax.maxFileMiB`).
    pub syntax_max_mib: u64,
    /// Embedded languages (`syntax.injections`).
    pub injections: bool,
    /// Syntax thread started while loading (adopted once).
    pub early_syntax: Option<EarlySyntax>,
    /// Scripted keys of an interaction benchmark.
    pub bench: Option<bench::Script>,
}

/// Drawn above the text; takes the keyboard while open.
enum Popup {
    Palette(Box<Palette>),
    Panel(Panel),
}

/// A read-only text panel (About, Keyboard Shortcuts).
struct Panel {
    title: &'static str,
    footer: &'static str,
    lines: Vec<PanelLine>,
    /// URL opened by a click on each line.
    links: Vec<Option<&'static str>>,
    icon: bool,
    /// First visible line.
    first: usize,
}

/// A question shown in the status line until answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Prompt {
    /// Unsaved changes, then what to do once they are saved or dropped.
    DirtyClose(Exit),
    /// The file changed on disk; `Baseline` is the version found there.
    Conflict(Baseline),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Unit {
    Char,
    Word,
    Line,
}

/// Mouse selection in progress: granularity and the range first clicked.
#[derive(Clone, Copy, Debug)]
struct Drag {
    unit: Unit,
    origin: (u64, u64),
    mode: DragMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DragMode {
    /// One selection replaces all others.
    Single,
    /// `Alt+Click`: the drag changes only the added (primary) selection.
    Add,
    /// `Shift+Alt` or middle-button drag from the anchor cell.
    Column((u64, u64)),
    /// The scroll slider, held `grab` pixels below its top.
    Slider { grab: i32 },
}

/// Snapshot, caret, syntax generation and the bracket pair found there.
type BracketPair = (Snapshot, u64, u64, Option<(u64, u64)>);

/// Snapshot, tab width and (line start, scan end offset, its column) of
/// long visible lines.
type Seams = (Option<Snapshot>, u32, Vec<(u64, u64, u64)>);

pub struct Editor {
    doc: Document,
    path: Option<PathBuf>,
    info: Option<FileInfo>,
    baseline: Option<Baseline>,
    renderer: Renderer,
    view: ViewBuilder,
    list: DisplayList,
    options: Options,
    theme: Theme,
    scroll: Scroll,
    size: (u32, u32),
    title: String,
    status: LoadStatus,
    loads: Option<Load>,
    /// A dropped file waiting for its loader's first answer.
    pending: Option<PendingOpen>,
    settings: Settings,
    dpi: u32,
    zoom: i32,
    wheel: (i32, i32),
    trace: Trace,
    notice: Option<String>,
    prompt: Option<Prompt>,
    saving: Option<Receiver<SaveOutcome>>,
    save_queued: bool,
    /// Quit or close the file once the running save finishes.
    closing: Option<Exit>,
    drag: Option<Drag>,
    files: Arc<dyn FileOps>,
    wake: Wake,
    /// Window title last set (main sets the first one).
    shown_title: String,
    /// Column of the last queried caret: snapshot, offset, tab width, column.
    caret_column: RefCell<Option<(Snapshot, u64, u32, u64)>>,
    /// Scan ends of long visible lines for the snapshot and tab width.
    seams: RefCell<Seams>,
    /// A frame was drawn (traced as `first_paint`).
    painted: bool,
    /// Syntax highlighting is on (`View: Toggle Syntax Highlighting`).
    highlighting: bool,
    /// Language of the document: detected, or chosen when `language_chosen`.
    language: Option<&'static Lang>,
    language_chosen: bool,
    /// The syntax thread, while highlighting a language.
    syntax: Option<Syntax>,
    /// Highlighting is off because the text exceeds `syntax.maxFileMiB`.
    syntax_over_limit: bool,
    /// Go to Symbol waits for the syntax thread.
    symbols_pending: bool,
    /// Last bracket pair: snapshot, caret, syntax generation, pair.
    bracket_cache: RefCell<Option<BracketPair>>,
    bench: Option<bench::Driver>,
    /// The first frame is on screen.
    shown: bool,
    popup: Option<Popup>,
    /// Favorite and recent commands, read when the palette first opens.
    store: Option<Store>,
    /// Drops the space character that follows a handled `Ctrl+Space`.
    skip_space: bool,
    /// Popup font when the editor is zoomed: popups keep the size of zoom 0.
    ui_renderer: Option<Renderer>,
    /// About icon scaled to a side: (side, pixels).
    about_icon: Option<(u32, Arc<[u32]>)>,
    /// The search box, created on first use.
    find: Option<Box<FindBox>>,
    /// The search thread, started on first use.
    search: Option<SearchThread>,
    /// Generation of operation requests (occurrences, replace, select all).
    op_generation: u64,
    pending_op: Option<PendingOp>,
    /// First key of a chord waiting for its second key.
    chord: Option<Key>,
    /// Selections before cursor commands, for `Ctrl+U`.
    cursor_history: std::collections::VecDeque<(Snapshot, Selections)>,
    column: Option<Column>,
    /// Word whose occurrences match whole words (`Ctrl+D` from a caret).
    occurrence_word: Option<String>,
    /// Selections before each Expand Selection, and the last result.
    expansions: Vec<Selections>,
    expanded: Option<Selections>,
    /// Offsets of the symbols listed by Go to Symbol.
    symbol_offsets: Vec<u64>,
    /// Go to Line: the scroll before its preview, restored by `Escape`.
    line_preview: Option<Scroll>,
    /// The window covers its monitor (`F11`), as the platform reported.
    full_screen: bool,
    /// Trace marks done: first highlighted frame, fully highlighted.
    marked: (bool, bool),
}

/// What a frame shows; a repaint is requested only when it changes.
struct Visible {
    snapshot: Snapshot,
    saved: Snapshot,
    selections: Selections,
    scroll: Scroll,
    px: f32,
    prompt: Option<Prompt>,
    notice: Option<String>,
    saving: bool,
    overtype: bool,
    highlighting: bool,
    /// Syntax generation and language.
    syntax: (u64, Option<&'static str>),
    locks: Locks,
    /// Load state and shown percentage.
    status: (u8, u64),
    new_file: bool,
    /// Fingerprint of what the popup shows.
    popup: u64,
    full_screen: bool,
    slider_active: bool,
}

impl Visible {
    fn same(&self, other: &Visible) -> bool {
        self.snapshot.same_tree(&other.snapshot)
            && self.saved.same_tree(&other.saved)
            && self.selections == other.selections
            && self.scroll == other.scroll
            && self.px == other.px
            && self.prompt == other.prompt
            && self.notice == other.notice
            && self.saving == other.saving
            && self.overtype == other.overtype
            && self.highlighting == other.highlighting
            && self.syntax == other.syntax
            && self.locks == other.locks
            && self.status == other.status
            && self.new_file == other.new_file
            && self.popup == other.popup
            && self.full_screen == other.full_screen
            && self.slider_active == other.slider_active
    }
}

/// Application name from `Meta/AppMeta.json` (exported by the build script).
pub const APP_NAME: &str = env!("RTE_APP_NAME");
const VERSION: &str = env!("CARGO_PKG_VERSION");
const REPOSITORY_URL: &str = env!("RTE_REPOSITORY_URL");
const DOCUMENTATION_URL: &str = env!("RTE_DOCUMENTATION_URL");
/// Square About icon from the build script: premultiplied `0xAARRGGBB`, little-endian.
static ABOUT_ICON: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/about_icon.bin"));
/// "The Zen of Royi Text Editor", shown in the About panel.
const MANIFEST: &str = include_str!("../../../Meta/Manifest.md");

/// Window title for `path` (the application name alone without a file),
/// marked when the document has unsaved changes.
pub fn window_title(path: Option<&Path>, dirty: bool) -> String {
    let Some(path) = path else {
        return APP_NAME.to_owned();
    };
    let mark = if dirty { "\u{25CF} " } else { "" };
    format!("{mark}{} - {APP_NAME}", display_name(path))
}

impl Editor {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        path: Option<&Path>,
        first: Option<LoadEvent>,
        loads: Option<Load>,
        renderer: Renderer,
        dpi: u32,
        mut settings: Settings,
        trace: Trace,
        files: Arc<dyn FileOps>,
        wake: Wake,
    ) -> Editor {
        let title = path.map_or_else(String::new, |p| p.display().to_string());
        let mut doc = Document::new(EditOptions::default());
        doc.locks.no_file = path.is_none();
        doc.locks.user = settings.read_only;
        doc.locks.loading = loads.is_some();
        let notice = settings.notice.clone();
        let highlighting = settings.highlighting;
        let bench = settings.bench.take().map(bench::Driver::new);
        let mut editor = Editor {
            doc,
            path: path.map(Path::to_path_buf),
            info: None,
            baseline: None,
            renderer,
            view: ViewBuilder::default(),
            list: DisplayList::default(),
            options: Options::default(),
            theme: Theme::DARK,
            scroll: Scroll::default(),
            size: (0, 0),
            title,
            status: if loads.is_some() {
                LoadStatus::Loading {
                    loaded: 0,
                    total: 0,
                }
            } else {
                LoadStatus::Ready
            },
            loads,
            pending: None,
            settings,
            dpi,
            zoom: 0,
            wheel: (0, 0),
            trace,
            notice,
            prompt: None,
            saving: None,
            save_queued: false,
            closing: None,
            drag: None,
            files,
            wake,
            shown_title: window_title(path, false),
            caret_column: RefCell::new(None),
            seams: RefCell::new((None, 0, Vec::new())),
            painted: false,
            highlighting,
            language: None,
            language_chosen: false,
            syntax: None,
            syntax_over_limit: false,
            symbols_pending: false,
            bracket_cache: RefCell::new(None),
            bench,
            shown: false,
            popup: None,
            store: None,
            skip_space: false,
            ui_renderer: None,
            about_icon: None,
            find: None,
            search: None,
            op_generation: 0,
            pending_op: None,
            chord: None,
            cursor_history: std::collections::VecDeque::new(),
            column: None,
            occurrence_word: None,
            expansions: Vec::new(),
            expanded: None,
            symbol_offsets: Vec::new(),
            line_preview: None,
            full_screen: false,
            marked: (false, false),
        };
        // `--syntax` belongs to the command-line file; without one it is unused.
        if editor.path.is_some() {
            editor.choose_initial_language();
        }
        if let Some(event) = first {
            editor.apply_load(event);
        }
        // Events queued before the waker existed produced no wake-up.
        editor.drain_loads();
        editor
    }

    /// Applies `--syntax <id>`; an unknown id keeps detection.
    fn choose_initial_language(&mut self) {
        let Some(id) = self.settings.syntax.as_deref() else {
            return;
        };
        if id.eq_ignore_ascii_case("none") || id.eq_ignore_ascii_case("off") {
            return;
        }
        match rte_syntax::by_id(id) {
            Some(lang) => {
                self.language = Some(lang);
                self.language_chosen = true;
            }
            None => {
                self.notice = Some(format!(
                    "Unknown syntax {id:?}: the language follows the file name"
                ));
            }
        }
    }

    /// Detects the language from the path and the first bytes, unless chosen.
    fn detect_language(&mut self) {
        if self.language_chosen {
            return;
        }
        let s = &self.doc.snapshot;
        self.language = self
            .path
            .as_deref()
            .and_then(|path| crate::syntax::detect(path, s));
    }

    /// Starts (or stops) the syntax thread for the current language, text
    /// and settings. Plain text runs no syntax work.
    fn restart_syntax(&mut self) {
        // Taken first so that no early thread outlives a decision against it.
        let early = self
            .settings
            .early_syntax
            .take()
            .and_then(|slot| slot.lock().ok()?.close());
        self.syntax = None;
        self.doc.journal = None;
        self.syntax_over_limit = false;
        self.symbols_pending = false;
        *self.bracket_cache.get_mut() = None;
        let Some(lang) = self.language else {
            return;
        };
        // The empty state runs no syntax work, whatever the language.
        if self.no_file() {
            return;
        }
        if !self.highlighting || !matches!(self.status, LoadStatus::Ready) {
            return;
        }
        let max = self.settings.syntax_max_mib;
        if self.doc.snapshot.len() > max * 1024 * 1024 {
            self.syntax_over_limit = true;
            self.notice = Some(format!(
                "Syntax highlighting off: the file is larger than {max} MiB"
            ));
            return;
        }
        self.doc.journal = Some(Vec::new());
        self.syntax = Some(match early {
            Some((l, syntax)) if std::ptr::eq(l, lang) && syntax.is_for(&self.doc.snapshot) => {
                syntax
            }
            _ => Syntax::start(
                lang,
                self.doc.snapshot.clone(),
                self.settings.injections,
                self.wake.clone(),
            ),
        });
        self.sync_syntax();
    }

    /// Sends edits and the visible range to the syntax thread. Growing past
    /// `syntax.maxFileMiB` stops highlighting; shrinking below restarts it.
    fn sync_syntax(&mut self) {
        let over = self.doc.snapshot.len() > self.settings.syntax_max_mib * 1024 * 1024;
        if over && self.syntax.is_some() || !over && self.syntax_over_limit {
            self.restart_syntax();
            return;
        }
        if self.syntax.is_none() {
            return;
        }
        let (visible, margin) = self.visible_bytes();
        if let Some(syntax) = &mut self.syntax {
            syntax.sync(&mut self.doc, margin.clone());
            syntax.set_visible(visible, margin);
        }
    }

    fn drain_syntax(&mut self) {
        let Some(syntax) = &mut self.syntax else {
            return;
        };
        syntax.poll();
        if let Some(e) = syntax.failed() {
            self.notice = Some(format!("Syntax highlighting off: {e}"));
            self.syntax = None;
            self.doc.journal = None;
            return;
        }
        if let Some(symbols) = syntax.take_symbols() {
            self.show_symbols(&symbols);
        }
    }

    /// Drawn bytes of the visible rows (at most [`ROW_SCAN_LIMIT`] per row,
    /// plus the visible part of a long line past it), adjacent rows merged.
    fn visible_ranges(&self) -> Vec<std::ops::Range<u64>> {
        let s = &self.doc.snapshot;
        let rows = u64::from(self.layout().rows.max(1));
        let top = self.scroll.top_line;
        let mut ranges: Vec<std::ops::Range<u64>> = Vec::new();
        let mut push = |r: std::ops::Range<u64>| match ranges.last_mut() {
            Some(last) if last.end >= r.start => last.end = last.end.max(r.end),
            _ => ranges.push(r),
        };
        for line in top..top + rows {
            let Some(range) = s.line_range(line) else {
                break;
            };
            // The line break belongs to the row (CRLF, comment ends).
            let with_break = (range.end + 1).min(s.len());
            push(range.start..with_break.min(range.start + ROW_SCAN_LIMIT as u64));
            if let Some(tail) = self.long_row_tail(range.clone()) {
                push(
                    tail.start..if tail.end == range.end {
                        with_break
                    } else {
                        tail.end
                    },
                );
            }
        }
        ranges
    }

    /// Bytes of a long line past its scan end that the view draws, with up
    /// to 3 more at each edge (whole characters); `None` when not visible.
    pub(super) fn long_row_tail(&self, line: std::ops::Range<u64>) -> Option<std::ops::Range<u64>> {
        if line.end - line.start <= ROW_SCAN_LIMIT as u64 {
            return None;
        }
        let first = self.scroll.first_col;
        let end_col = first + u64::from(self.layout().cols + 1);
        let (seam, seam_col) = self.seam(line.start, end_col)?;
        let base = line.start + seam;
        let from = base + first.saturating_sub(seam_col).saturating_sub(3);
        let to = (base + (end_col - seam_col) + 3).min(line.end);
        (from < to).then_some(from..to)
    }

    /// Scan end of the long line at `start` and its column, when that column
    /// is before `until` (reads only up to `until` columns otherwise, so a
    /// view at the line start never scans 64 KiB).
    fn seam(&self, start: u64, until: u64) -> Option<(u64, u64)> {
        let s = &self.doc.snapshot;
        let tab = self.options.tab_width;
        let mut cache = self.seams.borrow_mut();
        if !cache.0.as_ref().is_some_and(|c| c.same_tree(s)) || cache.1 != tab {
            *cache = (Some(s.clone()), tab, Vec::new());
        }
        if let Some(&(_, seam, col)) = cache.2.iter().find(|e| e.0 == start) {
            return (col < until).then_some((seam, col));
        }
        let limit = ROW_SCAN_LIMIT as u64;
        let mut window = until.saturating_mul(4).saturating_add(256).min(limit);
        loop {
            let bytes = s.to_vec(start..start + window);
            let end = edit::scan_end(&bytes);
            let col = edit::column_until(&bytes[..end], until, tab)?;
            if window == limit {
                if cache.2.len() >= 256 {
                    cache.2.clear();
                }
                cache.2.push((start, end as u64, col));
                return (col < until).then_some((end as u64, col));
            }
            window = window.saturating_mul(4).min(limit);
        }
    }

    /// The span of the drawn bytes, and a page of rows above and below it
    /// (parsed ahead), each row counted at most [`ROW_SCAN_LIMIT`] bytes.
    fn visible_bytes(&self) -> (std::ops::Range<u64>, std::ops::Range<u64>) {
        let s = &self.doc.snapshot;
        let rows = u64::from(self.layout().rows.max(1));
        let top = self.scroll.top_line;
        let ranges = self.visible_ranges();
        let visible = match (ranges.first(), ranges.last()) {
            (Some(first), Some(last)) => first.start..last.end,
            _ => s.len()..s.len(),
        };
        let page = rows * ROW_SCAN_LIMIT as u64;
        let before = s
            .line_start(top.saturating_sub(rows))
            .unwrap_or(0)
            .max(visible.start.saturating_sub(page));
        let after = s
            .line_start(top + 2 * rows)
            .unwrap_or(s.len())
            .min(visible.end + page)
            .max(visible.end);
        (visible, before..after)
    }

    /// Applies queued load events. A loader that disconnects before its final
    /// event becomes a visible failure instead of an endless "Loading".
    fn drain_loads(&mut self) {
        let Some(load) = &self.loads else {
            return;
        };
        let mut events = Vec::new();
        let disconnected = loop {
            match load.events.try_recv() {
                Ok(event) => events.push(event),
                Err(TryRecvError::Empty) => break false,
                Err(TryRecvError::Disconnected) => break true,
            }
        };
        for event in events {
            self.apply_load(event);
        }
        if disconnected {
            self.loads = None;
            if matches!(self.status, LoadStatus::Loading { .. }) {
                self.apply_load(LoadEvent::Failed {
                    snapshot: self.doc.snapshot.clone(),
                    error: "loading stopped unexpectedly".to_owned(),
                });
            }
        }
    }

    fn apply_load(&mut self, event: LoadEvent) {
        if !matches!(event, LoadEvent::Progress { .. }) {
            self.trace.mark("load_done");
            self.doc.locks.loading = false;
        }
        match event {
            LoadEvent::Progress {
                snapshot,
                loaded,
                total,
            } => {
                self.doc.load(snapshot);
                self.status = LoadStatus::Loading { loaded, total };
            }
            LoadEvent::Done {
                snapshot,
                info,
                baseline,
                read_ms,
            } => {
                self.trace.mark_at("file_read", read_ms);
                self.doc.load(snapshot);
                self.doc.locks.encoding = info.malformed;
                self.doc.options.line_ending = info.line_ending;
                self.doc.options.insert_spaces = !info.indent_tabs;
                self.info = Some(info);
                self.baseline = baseline;
                self.status = LoadStatus::Ready;
                self.detect_language();
                self.restart_syntax();
            }
            LoadEvent::Failed { snapshot, error } => {
                self.doc.load(snapshot);
                self.doc.locks.load_error = true;
                self.status = LoadStatus::Failed(error);
            }
            // A folder named on the command line: the empty state.
            LoadEvent::Rejected(reason) => {
                self.reset_document(None);
                self.notice = Some(reason);
            }
        }
        self.clamp_scroll();
    }

    /// `--exit-after-load`: report the trace and close once loading ended;
    /// `--exit-after-highlight` also waits for the full highlight of a code
    /// file (see [`App::presented`]).
    fn exit_if_loaded(&mut self, ctx: &mut Ctx) {
        let loaded = !matches!(self.status, LoadStatus::Loading { .. });
        let highlight_pending = self.settings.exit_after_highlight && self.syntax.is_some();
        if (self.settings.exit_after_load || self.settings.exit_after_highlight)
            && loaded
            && !highlight_pending
        {
            self.trace.report();
            ctx.close();
        }
    }

    /// Trace marks of the first highlighted frame and of the first one from
    /// a parse of the whole file (`--trace-startup`).
    fn mark_highlights(&mut self, ctx: &mut Ctx) {
        let Some(syntax) = &self.syntax else {
            return;
        };
        if !self.marked.0 && syntax.highlighted() {
            self.marked.0 = true;
            self.trace.mark("highlighted");
        }
        if !self.marked.1 && syntax.fully_highlighted() {
            self.marked.1 = true;
            self.trace.mark("fully_highlighted");
            if self.settings.exit_after_highlight {
                self.trace.report();
                ctx.close();
            }
        }
    }

    fn px(&self) -> f32 {
        let zoom = 1.1f32.powi(self.zoom);
        (BASE_PX * self.dpi as f32 / 96.0 * zoom).round()
    }

    fn layout(&self) -> Layout {
        let options = Options {
            title_strip: !self.full_screen,
            // 6 px at 96 DPI; none without a file.
            scroll_strip: if self.no_file() {
                0
            } else {
                (6 * self.dpi + 48) / 96
            },
            ..self.options
        };
        Layout::new(
            self.size.0,
            self.size.1,
            self.renderer.cell(),
            self.doc.snapshot.line_count(),
            &options,
        )
    }

    /// The scroll slider of the current view, if shown.
    fn slider(&self) -> Option<rte_view::Rect> {
        let layout = self.layout();
        let cell = self.renderer.cell();
        let page = rte_view::page_rows(&layout, cell);
        let lines = self.doc.snapshot.line_count();
        rte_view::slider(&layout, lines, self.scroll.top_line, page, cell.height)
    }

    fn slider_active(&self) -> bool {
        matches!(
            self.drag,
            Some(Drag {
                mode: DragMode::Slider { .. },
                ..
            })
        )
    }

    /// Scrolls so the slider's top is at `y` (window pixels), as far as
    /// its track allows.
    fn drag_slider(&mut self, y: i32) {
        let Some(slider) = self.slider() else {
            return;
        };
        let track = self.layout().scrollbar;
        let travel = i64::from(track.h - slider.h);
        if travel <= 0 {
            return;
        }
        let top = i64::from(y - track.y).clamp(0, travel);
        let last = self.doc.snapshot.line_count() as i64 - 1;
        self.scroll.top_line = ((top * last + travel / 2) / travel) as u64;
        self.clamp_scroll();
    }

    fn toggle_full_screen(&mut self, ctx: &mut Ctx) {
        ctx.set_full_screen(!self.full_screen);
    }

    /// Fully visible text rows (at least one).
    fn page(&self) -> u64 {
        let cell = self.renderer.cell();
        u64::from((self.layout().text.h / cell.height.max(1)).max(1))
    }

    fn scroll_lines(&mut self, delta: i64) {
        self.scroll.top_line = self.scroll.top_line.saturating_add_signed(delta);
        self.clamp_scroll();
    }

    fn scroll_cols(&mut self, delta: i64) {
        self.scroll.first_col = self.scroll.first_col.saturating_add_signed(delta);
    }

    fn clamp_scroll(&mut self) {
        let last = self.doc.snapshot.line_count() - 1;
        self.scroll.top_line = self.scroll.top_line.min(last);
    }

    fn set_zoom(&mut self, zoom: i32) {
        self.zoom = zoom.clamp(-8, 16);
        let px = self.px();
        self.renderer.set_size(px);
    }

    /// Scrolls so the primary caret is visible.
    fn reveal(&mut self) {
        let head = self.doc.selections.primary().head;
        let line = self.doc.snapshot.line_of(head);
        let page = self.page();
        if line < self.scroll.top_line {
            self.scroll.top_line = line;
        } else if line >= self.scroll.top_line + page {
            self.scroll.top_line = line + 1 - page;
        }
        let col = self.caret_column(head);
        let cols = u64::from(self.layout().cols.max(1));
        if col < self.scroll.first_col {
            self.scroll.first_col = col.saturating_sub(REVEAL_MARGIN);
        } else if col >= self.scroll.first_col + cols {
            self.scroll.first_col = (col + REVEAL_MARGIN).saturating_sub(cols - 1);
        }
    }

    fn edit_options(&self) -> EditOptions {
        EditOptions {
            tab_width: self.options.tab_width,
            ..self.doc.options
        }
    }

    /// Display column of `head`, reused while the text, caret and tab width
    /// are unchanged (reveal and the status line ask for the same caret).
    fn caret_column(&self, head: u64) -> u64 {
        let tab = self.options.tab_width;
        if let Some((snapshot, h, t, col)) = &*self.caret_column.borrow()
            && snapshot.same_tree(&self.doc.snapshot)
            && (*h, *t) == (head, tab)
        {
            return *col;
        }
        let col = edit::column_at(&self.doc.snapshot, head, tab);
        *self.caret_column.borrow_mut() = Some((self.doc.snapshot.clone(), head, tab, col));
        col
    }

    fn visible(&self) -> Visible {
        let status = match &self.status {
            LoadStatus::Loading { loaded, total } => (0, loaded * 100 / (*total).max(1)),
            LoadStatus::Ready => (1, 0),
            LoadStatus::Failed(_) => (2, 0),
        };
        Visible {
            snapshot: self.doc.snapshot.clone(),
            saved: self.doc.saved.clone(),
            selections: self.doc.selections.clone(),
            scroll: self.scroll,
            px: self.renderer.px(),
            prompt: self.prompt,
            notice: self.notice.clone(),
            saving: self.saving.is_some(),
            overtype: self.doc.options.overtype,
            highlighting: self.highlighting,
            syntax: (
                self.syntax.as_ref().map_or(0, Syntax::generation),
                self.language.map(|l| l.id),
            ),
            locks: self.doc.locks,
            status,
            new_file: self.info.is_some_and(|i| i.new_file),
            popup: self.popup_view(),
            full_screen: self.full_screen,
            slider_active: self.slider_active(),
        }
    }

    /// Applies `cmd` (if any) and reveals the caret; locks show a notice.
    fn run(&mut self, cmd: Option<Command>, typed: Option<char>) {
        let Some(cmd) = cmd else {
            return;
        };
        match self.doc.apply(&cmd, typed) {
            Ok(()) => self.reveal(),
            Err(reason) => self.notice = Some(format!("Not editable: {reason}")),
        }
    }

    /// Moves every caret to `f(snapshot, selection) = (head, goal)`; with
    /// `extend` the anchors stay.
    fn move_heads(
        &mut self,
        extend: bool,
        f: impl Fn(&Snapshot, &Selection) -> (u64, Option<u64>),
    ) {
        let snapshot = &self.doc.snapshot;
        let sels = self.doc.selections.map(|sel| {
            let (head, goal) = f(snapshot, sel);
            Selection {
                anchor: if extend { sel.anchor } else { head },
                head,
                goal,
            }
        });
        self.doc.select(sels);
        self.reveal();
    }

    fn select_single(&mut self, selection: Selection) {
        self.doc.select(Selections::single(selection));
    }

    fn undo_redo(&mut self, redo: bool) {
        let result = if redo {
            self.doc.redo()
        } else {
            self.doc.undo()
        };
        match result {
            Ok(true) => self.reveal(),
            Ok(false) => {}
            Err(reason) => self.notice = Some(format!("Not editable: {reason}")),
        }
    }

    fn copy(&mut self, ctx: &mut Ctx, cut: bool) {
        let opts = self.edit_options();
        let (snapshot, sels) = (&self.doc.snapshot, &self.doc.selections);
        if cut && let Some(reason) = self.doc.locks.edit_refusal() {
            self.notice = Some(format!("Not editable: {reason}"));
            return;
        }
        let (clip, cmd) = if cut {
            edit::cut(snapshot, sels, &opts)
        } else {
            (edit::copy(snapshot, sels, &opts), None)
        };
        let (text, data) = clip::encode(&clip, opts.line_ending);
        if !ctx.set_clipboard(&text, clip::FORMAT, &data) {
            self.notice = Some("The clipboard is not available".to_owned());
            return;
        }
        self.run(cmd, None);
    }

    fn paste(&mut self, ctx: &mut Ctx) {
        let Some((text, data)) = ctx.clipboard(clip::FORMAT) else {
            return;
        };
        let clip = clip::decode(text, data.as_deref());
        let opts = self.edit_options();
        let cmd = edit::paste(&self.doc.snapshot, &self.doc.selections, &clip, &opts);
        self.run(cmd, None);
    }

    /// `Ctrl+S`. Returns false when saving is refused.
    fn request_save(&mut self) -> bool {
        if let Some(reason) = self.doc.locks.save_refusal() {
            self.notice = Some(format!("Cannot save: {reason}"));
            return false;
        }
        if self.saving.is_some() {
            self.save_queued = true;
        } else {
            self.start_save(self.baseline);
        }
        true
    }

    /// Starts saving the current snapshot against `baseline`.
    fn start_save(&mut self, baseline: Option<Baseline>) {
        let Some(path) = self.path.clone() else {
            return;
        };
        let encoding = self.info.map_or(Encoding::Utf8, |i| i.encoding);
        let wake = self.wake.clone();
        // Edits after this point start a new undo step, so undo reaches the saved text.
        self.doc.history.seal();
        let request = SaveRequest {
            path,
            snapshot: self.doc.snapshot.clone(),
            encoding,
            baseline,
        };
        self.saving = Some(save::spawn(self.files.clone(), request, move || wake()));
    }

    fn drain_saves(&mut self, ctx: &mut Ctx) {
        let Some(rx) = &self.saving else {
            return;
        };
        let outcome = match rx.try_recv() {
            Ok(outcome) => outcome,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => SaveOutcome::Failed {
                error: "Saving stopped unexpectedly; the file may be unchanged".to_owned(),
            },
        };
        self.saving = None;
        match outcome {
            SaveOutcome::Saved {
                snapshot,
                baseline,
                warning,
            } => {
                self.doc.saved = snapshot;
                self.doc.history.seal();
                self.baseline = baseline;
                if let Some(info) = self.info.as_mut() {
                    info.new_file = false;
                }
                self.notice = Some(warning.unwrap_or_else(|| "Saved".to_owned()));
            }
            SaveOutcome::Conflict { disk } => {
                self.save_queued = false;
                self.prompt = Some(Prompt::Conflict(disk));
            }
            SaveOutcome::Failed { error } => {
                self.save_queued = false;
                self.notice = Some(error);
            }
        }
        if self.save_queued {
            self.save_queued = false;
            self.start_save(self.baseline);
        } else if let Some(exit) = self.closing
            && self.prompt.is_none()
        {
            self.closing = None;
            if self.doc.is_dirty() {
                self.prompt = Some(Prompt::DirtyClose(exit));
            } else {
                self.finish_exit(exit, ctx);
            }
        }
    }

    /// Quit (`Alt+F4`, `Ctrl+Shift+W`, the window close request), asking
    /// first about unsaved changes; waits for a running save.
    fn request_close(&mut self, ctx: &mut Ctx) {
        self.pending = None;
        if self.saving.is_some() {
            self.closing = Some(Exit::Quit);
        } else if self.doc.is_dirty() {
            self.prompt = Some(Prompt::DirtyClose(Exit::Quit));
        } else {
            ctx.close();
        }
    }

    fn prompt_key(&mut self, prompt: Prompt, key: Key, ctx: &mut Ctx) {
        let plain = !key.ctrl && !key.alt;
        match (prompt, key.vk) {
            (_, VK_ESCAPE) => {
                self.prompt = None;
                self.closing = None;
            }
            (Prompt::DirtyClose(exit), VK_RETURN | VK_S) if plain => {
                self.prompt = None;
                self.closing = Some(exit);
                if !self.request_save() {
                    self.closing = None;
                    self.prompt = Some(Prompt::DirtyClose(exit));
                }
            }
            (Prompt::DirtyClose(exit), VK_D | VK_N) if plain => {
                self.prompt = None;
                self.finish_exit(exit, ctx);
            }
            (Prompt::Conflict(disk), VK_O) if plain => {
                self.prompt = None;
                self.start_save(Some(disk));
            }
            _ => {}
        }
    }

    fn key(&mut self, key: Key, ctx: &mut Ctx) -> bool {
        if let Some(prompt) = self.prompt {
            self.prompt_key(prompt, key, ctx);
            return true;
        }
        if self.closing.is_some() {
            return true;
        }
        if let Some(first) = self.chord.take() {
            match commands::for_chord(first, key) {
                Some(def) => {
                    self.notice = None;
                    self.perform(def.action, ctx);
                }
                None => {
                    self.notice = Some(format!(
                        "The key combination {} {} is not a command",
                        key_name(first),
                        key_name(key)
                    ));
                }
            }
            return true;
        }
        if key.alt {
            // Unbound Alt keys keep their system behavior (window menu).
            let Some(def) = commands::for_key(key) else {
                return false;
            };
            // Line and page commands act on the text: not from the find box
            // or a popup (the palette can still run them by name).
            let text_focus = !self.find_focused() && self.popup.is_none();
            if text_focus || !commands::needs_text_focus(def.action) {
                self.perform(def.action, ctx);
            }
            return true;
        }
        self.skip_space = false;
        match self.popup {
            Some(Popup::Palette(_)) => return self.palette_key(key, ctx),
            Some(Popup::Panel(_)) => return self.panel_key(key, ctx),
            None => {}
        }
        if self.find_focused() {
            return self.find_key(key, ctx);
        }
        self.notice = None;
        if self.find_open() && key.vk == VK_ESCAPE && !key.ctrl && !key.shift {
            self.close_find();
            return true;
        }
        if commands::starts_chord(key) {
            self.chord = Some(key);
            self.notice = Some(format!(
                "({}) was pressed. Waiting for the second key of the chord\u{2026}",
                key_name(key)
            ));
            return true;
        }
        if let Some(def) = commands::for_key(key) {
            self.perform(def.action, ctx);
            return true;
        }
        if self.no_file() {
            return false;
        }
        self.edit_key(key)
    }

    /// Runs a registry command (from a key or the palette).
    fn perform(&mut self, action: Action, ctx: &mut Ctx) {
        if self.no_file() && !works_without_file(action) && action != Action::Close {
            self.notice = Some("No file is open".to_owned());
            return;
        }
        let (s, sels) = (&self.doc.snapshot, &self.doc.selections);
        match action {
            Action::Save => {
                self.request_save();
            }
            Action::Close => self.close_file(),
            Action::Quit => self.request_close(ctx),
            Action::ToggleReadOnly => {
                self.doc.locks.user = !self.doc.locks.user;
                let state = if self.doc.locks.user { "on" } else { "off" };
                self.notice = Some(format!("Read-only {state}"));
            }
            Action::Undo => self.undo_redo(false),
            Action::Redo => self.undo_redo(true),
            Action::Cut => self.copy(ctx, true),
            Action::Copy => self.copy(ctx, false),
            Action::Paste => self.paste(ctx),
            Action::DeleteWordLeft => self.run(edit::delete_word_left(s, sels), None),
            Action::DeleteWordRight => self.run(edit::delete_word_right(s, sels), None),
            Action::Outdent => {
                let cmd = edit::outdent(s, sels, &self.edit_options());
                self.run(cmd, None);
            }
            Action::ToggleOvertype => self.doc.options.overtype = !self.doc.options.overtype,
            Action::SelectAll => self.select_single(Selection::new(0, s.len())),
            Action::GoToTop | Action::SelectToTop => {
                self.move_heads(action == Action::SelectToTop, |_, _| (0, None));
            }
            Action::GoToBottom | Action::SelectToBottom => {
                self.move_heads(action == Action::SelectToBottom, |s, _| (s.len(), None));
            }
            Action::ScrollLineUp => self.scroll_lines(-1),
            Action::ScrollLineDown => self.scroll_lines(1),
            Action::ZoomIn => self.set_zoom(self.zoom + 1),
            Action::ZoomOut => self.set_zoom(self.zoom - 1),
            Action::ZoomReset => self.set_zoom(0),
            Action::ShowCommands => self.open_palette(Mode::All),
            Action::ShowFavorites => self.open_palette(Mode::Favorites),
            Action::About => self.popup = Some(Popup::Panel(about_panel())),
            Action::KeyboardShortcuts => self.popup = Some(Popup::Panel(shortcuts_panel())),
            Action::Documentation => ctx.open_url(DOCUMENTATION_URL),
            Action::ToggleSyntax => {
                self.highlighting = !self.highlighting;
                let state = if self.highlighting { "on" } else { "off" };
                self.notice = Some(format!("Syntax highlighting {state}"));
                self.restart_syntax();
            }
            Action::Find => self.open_find(false),
            Action::Replace => self.open_find(true),
            Action::FindNext => self.find_next(false),
            Action::FindPrevious => self.find_next(true),
            Action::ReplaceOne => self.replace_one(),
            Action::ReplaceAll => self.replace_all(),
            Action::SelectAllMatches => self.select_all_matches(),
            Action::ToggleMatchCase
            | Action::ToggleWholeWord
            | Action::ToggleRegex
            | Action::ToggleGlob
            | Action::TogglePreserveCase => self.toggle_find_option(action),
            Action::ToggleFindInSelection => self.toggle_find_in_selection(),
            Action::AddNextOccurrence => self.add_next_occurrence(false),
            Action::MoveToNextOccurrence => self.add_next_occurrence(true),
            Action::SelectAllOccurrences | Action::ChangeAllOccurrences => {
                self.select_all_occurrences();
            }
            Action::AddCursorAbove => self.add_cursor_vertical(-1),
            Action::AddCursorBelow => self.add_cursor_vertical(1),
            Action::CursorsAtLineEnds => self.cursors_at_line_ends(),
            Action::ExpandLineSelection => self.expand_line_selection(),
            Action::CursorUndo => self.cursor_undo(),
            Action::ColumnSelectUp => self.column_select(-1, 0),
            Action::ColumnSelectDown => self.column_select(1, 0),
            Action::ColumnSelectLeft => self.column_select(0, -1),
            Action::ColumnSelectRight => self.column_select(0, 1),
            Action::ColumnSelectPageUp => self.column_select(-(self.page() as i64), 0),
            Action::ColumnSelectPageDown => self.column_select(self.page() as i64, 0),
            Action::ChangeLanguage => self.open_language_mode(),
            Action::ToggleLineComment => self.comment(Some(edit::CommentMode::Toggle)),
            Action::AddLineComment => self.comment(Some(edit::CommentMode::Add)),
            Action::RemoveLineComment => self.comment(Some(edit::CommentMode::Remove)),
            Action::ToggleBlockComment => self.comment(None),
            Action::GoToBracket => self.go_to_bracket(),
            Action::ExpandSelection => self.expand_selection(),
            Action::ShrinkSelection => self.shrink_selection(),
            Action::GoToSymbol => self.open_symbols(),
            Action::MoveLineUp | Action::MoveLineDown => {
                if let Some(coded) = edit::move_lines(s, sels, action == Action::MoveLineUp) {
                    self.run_coded(coded, None);
                }
            }
            Action::CopyLineUp | Action::CopyLineDown => {
                let up = action == Action::CopyLineUp;
                let coded = edit::copy_lines(s, sels, up, &self.edit_options());
                self.run_coded(coded, None);
            }
            Action::DeleteLine => {
                let coded = edit::delete_lines(s, sels, &self.edit_options());
                self.run_coded(coded, None);
            }
            Action::InsertLineAbove | Action::InsertLineBelow => {
                let below = action == Action::InsertLineBelow;
                let coded = edit::insert_line(s, sels, below, &self.edit_options());
                self.run_coded(coded, None);
            }
            Action::IndentLine => {
                if let Some(coded) = edit::indent_lines(s, sels, &self.edit_options()) {
                    self.run_coded(coded, None);
                }
            }
            Action::ScrollPageUp => self.scroll_lines(-(self.page() as i64)),
            Action::ScrollPageDown => self.scroll_lines(self.page() as i64),
            Action::GoToLine => self.open_go_to_line(),
            Action::ToggleFullScreen => self.toggle_full_screen(ctx),
        }
    }

    /// Movement and editing keys that are not registry commands.
    fn edit_key(&mut self, key: Key) -> bool {
        let Key {
            vk, ctrl, shift, ..
        } = key;
        let tab = self.options.tab_width;
        let opts = self.edit_options();
        match (vk, ctrl) {
            (VK_LEFT, _) => self.move_heads(shift, |s, sel| {
                if !shift && !sel.is_empty() && !ctrl {
                    (sel.start(), None)
                } else if ctrl {
                    (edit::word_left(s, sel.head), None)
                } else {
                    (edit::prev_boundary(s, sel.head), None)
                }
            }),
            (VK_RIGHT, _) => self.move_heads(shift, |s, sel| {
                if !shift && !sel.is_empty() && !ctrl {
                    (sel.end(), None)
                } else if ctrl {
                    (edit::word_right(s, sel.head), None)
                } else {
                    (edit::next_boundary(s, sel.head), None)
                }
            }),
            (VK_UP | VK_DOWN, true) => self.scroll_lines(if vk == VK_UP { -1 } else { 1 }),
            (VK_UP | VK_DOWN | VK_PRIOR | VK_NEXT, false) => {
                let page = self.page() as i64;
                let lines = match vk {
                    VK_UP => -1,
                    VK_DOWN => 1,
                    VK_PRIOR => -page,
                    _ => page,
                };
                if vk == VK_PRIOR || vk == VK_NEXT {
                    self.scroll_lines(lines);
                }
                self.move_heads(shift, |s, sel| {
                    let (head, goal) = edit::vertical(s, sel.head, sel.goal, lines, tab);
                    (head, Some(goal))
                });
            }
            (VK_HOME, _) => self.move_heads(shift, |s, sel| {
                (
                    if ctrl {
                        0
                    } else {
                        edit::line_home(s, sel.head)
                    },
                    None,
                )
            }),
            (VK_END, _) => self.move_heads(shift, |s, sel| {
                (
                    if ctrl {
                        s.len()
                    } else {
                        edit::line_end(s, sel.head)
                    },
                    None,
                )
            }),
            (VK_ESCAPE, false) => {
                let primary = self.doc.selections.primary();
                let collapsed = if self.doc.selections.len() > 1 {
                    primary
                } else {
                    Selection::caret(primary.head)
                };
                self.select_single(collapsed);
            }
            (VK_BACK, _) => {
                if !ctrl && self.delete_pair() {
                    return true;
                }
                let (s, sels) = (&self.doc.snapshot, &self.doc.selections);
                let cmd = if ctrl {
                    edit::delete_word_left(s, sels)
                } else {
                    edit::backspace(s, sels)
                };
                self.run(cmd, None);
            }
            (VK_DELETE, _) => {
                let (s, sels) = (&self.doc.snapshot, &self.doc.selections);
                let cmd = if ctrl {
                    edit::delete_word_right(s, sels)
                } else {
                    edit::delete_forward(s, sels)
                };
                self.run(cmd, None);
            }
            (VK_RETURN, false) => {
                if self.newline_indent() {
                    return true;
                }
                let cmd = edit::newline(&self.doc.snapshot, &self.doc.selections, &opts);
                self.run(Some(cmd), None);
            }
            (VK_TAB, false) => {
                let cmd = edit::tab(&self.doc.snapshot, &self.doc.selections, &opts);
                self.run(Some(cmd), None);
            }
            _ => return false,
        }
        true
    }

    fn typed(&mut self, ch: char) {
        if self.prompt.is_some() || self.closing.is_some() || ch < ' ' || ch == '\u{7F}' {
            return;
        }
        if std::mem::take(&mut self.skip_space) && ch == ' ' {
            return;
        }
        match &mut self.popup {
            Some(Popup::Palette(p)) => {
                let mut buf = [0u8; 4];
                if p.query.insert(ch.encode_utf8(&mut buf)) {
                    self.refresh_palette();
                }
                return;
            }
            Some(Popup::Panel(_)) => return,
            None => {}
        }
        if self.find_focused() {
            self.find_typed(ch);
            return;
        }
        if self.no_file() {
            return;
        }
        self.notice = None;
        if self.auto_close(ch) {
            return;
        }
        let opts = self.edit_options();
        let mut buf = [0u8; 4];
        let cmd = edit::insert(
            &self.doc.snapshot,
            &self.doc.selections,
            ch.encode_utf8(&mut buf).as_bytes(),
            &opts,
        );
        self.run(Some(cmd), Some(ch));
    }

    /// Starts the favorites store on first use; its file is read by a worker.
    fn load_store(&mut self) {
        if self.store.is_some() {
            return;
        }
        self.store = Some(match &self.settings.user_dir {
            Some(dir) => {
                let wake = self.wake.clone();
                let file = FileStorage(dir.join(favorites::FILE_NAME));
                Store::spawn(file, move || wake())
            }
            None => Store::memory(),
        });
    }

    /// Applies the store worker's replies; a problem becomes the notice.
    fn drain_store(&mut self) {
        let Some(store) = &mut self.store else {
            return;
        };
        let rev = store.rev();
        if let Some(problem) = store.drain() {
            self.notice = Some(problem);
        }
        if store.rev() != rev
            && let Some(Popup::Palette(p)) = &mut self.popup
        {
            p.refresh_in_place(store, self.settings.recent_count);
        }
    }

    /// Opens the palette, or switches the open one to `mode`.
    fn open_palette(&mut self, mode: Mode) {
        self.load_store();
        let Some(store) = &self.store else {
            return;
        };
        let recent = self.settings.recent_count;
        match &mut self.popup {
            Some(Popup::Palette(p)) => {
                p.mode = mode;
                p.refresh(store, recent);
            }
            _ => self.popup = Some(Popup::Palette(Box::new(Palette::new(mode, store, recent)))),
        }
    }

    /// Rebuilds the palette rows after the query changed.
    fn refresh_palette(&mut self) {
        if matches!(&self.popup, Some(Popup::Palette(p)) if p.mode == Mode::Line) {
            self.update_go_to_line();
            return;
        }
        if let (Some(Popup::Palette(p)), Some(store)) = (&mut self.popup, &self.store) {
            p.refresh(store, self.settings.recent_count);
        }
    }

    /// Popups do not zoom: their font size is the one of zoom 0.
    fn ui_px(&self) -> f32 {
        (BASE_PX * self.dpi as f32 / 96.0).round()
    }

    /// Keeps the popup font at [`Self::ui_px`]; created when a popup opens
    /// while the editor is zoomed.
    fn sync_ui_renderer(&mut self) {
        let px = self.ui_px();
        if self.renderer.px() == px {
            self.ui_renderer = None;
        } else if let Some(renderer) = &mut self.ui_renderer {
            renderer.set_size(px);
        } else if self.popup.is_some() || self.find_open() {
            self.ui_renderer = Renderer::new(Cow::Borrowed(rte_assets::DEFAULT_FONT), px).ok();
        }
    }

    fn ui_cell(&self) -> Cell {
        self.ui_renderer.as_ref().unwrap_or(&self.renderer).cell()
    }

    fn palette_geometry(&self, p: &Palette) -> PaletteGeometry {
        let (w, h) = self.size;
        let top = self.layout().title.h as i32;
        PaletteGeometry::new(w, h, top, self.ui_cell(), p.items.len())
    }

    fn panel_geometry(&self, panel: &Panel) -> PanelGeometry {
        let (w, h) = self.size;
        let top = self.layout().title.h as i32;
        let cols = panel.lines.iter().map(PanelLine::cols).max().unwrap_or(0);
        let icon = panel.icon.then(|| self.icon_side());
        PanelGeometry::new(w, h, top, self.ui_cell(), cols, panel.lines.len(), icon)
    }

    fn icon_side(&self) -> u32 {
        self.ui_cell().height * 4
    }

    /// Screen area of the open popup.
    fn popup_area(&self) -> Option<rte_view::Rect> {
        match &self.popup {
            Some(Popup::Palette(p)) => Some(self.palette_geometry(p).panel),
            Some(Popup::Panel(panel)) => Some(self.panel_geometry(panel).panel),
            None => None,
        }
    }

    /// Fingerprint of what the open popup shows (repaint check).
    fn popup_view(&self) -> u64 {
        let mut h = DefaultHasher::new();
        match &self.popup {
            None => 0u8.hash(&mut h),
            Some(Popup::Palette(p)) => {
                1u8.hash(&mut h);
                p.mode.hash(&mut h);
                p.query.hash(&mut h);
                (p.selected, p.first, p.items.len()).hash(&mut h);
                self.store.as_ref().map(Store::rev).hash(&mut h);
            }
            Some(Popup::Panel(panel)) => {
                2u8.hash(&mut h);
                (panel.title, panel.first).hash(&mut h);
            }
        }
        self.find_view(&mut h);
        h.finish()
    }

    fn palette_key(&mut self, key: Key, ctx: &mut Ctx) -> bool {
        let Some(Popup::Palette(p)) = &self.popup else {
            return false;
        };
        let rows = self.palette_geometry(p).rows;
        let selected = p.selected;
        let Some(Popup::Palette(p)) = &mut self.popup else {
            return false;
        };
        let (page, ctrl, shift) = (rows as isize, key.ctrl, key.shift);
        let word = |word, char| if ctrl { word } else { char };
        let mut edited = false;
        match key.vk {
            VK_ESCAPE => self.popup = None,
            VK_RETURN if !ctrl => {
                if let Some(item) = selected {
                    self.run_palette_item(item, ctx);
                }
            }
            VK_UP => p.move_selection(-1, rows),
            VK_DOWN => p.move_selection(1, rows),
            VK_PRIOR => p.move_selection(-page, rows),
            VK_NEXT => p.move_selection(page, rows),
            VK_LEFT => {
                p.query
                    .move_caret(word(Motion::WordLeft, Motion::Left), shift);
            }
            VK_RIGHT => {
                p.query
                    .move_caret(word(Motion::WordRight, Motion::Right), shift);
            }
            VK_HOME => {
                p.query.move_caret(Motion::Home, shift);
            }
            VK_END => {
                p.query.move_caret(Motion::End, shift);
            }
            VK_BACK => edited = p.query.delete(false, ctrl),
            VK_DELETE => edited = p.query.delete(true, ctrl),
            VK_A if ctrl && !shift => {
                p.query.select_all();
            }
            VK_V if ctrl && !shift => {
                // The first line of the clipboard text.
                if let Some((text, _)) = ctx.clipboard(clip::FORMAT) {
                    edited = p.query.insert(text.lines().next().unwrap_or_default());
                }
            }
            VK_SPACE if ctrl && !shift => {
                self.skip_space = true;
                if let Some(item) = selected {
                    self.toggle_favorite(item);
                }
            }
            _ => self.popup_command(key, ctx),
        }
        if edited {
            self.refresh_palette();
        }
        true
    }

    fn panel_key(&mut self, key: Key, ctx: &mut Ctx) -> bool {
        let Some(Popup::Panel(panel)) = &self.popup else {
            return false;
        };
        let page = self.panel_geometry(panel).lines.max(1);
        let last = panel.lines.len().saturating_sub(page);
        let first = match key.vk {
            VK_ESCAPE => {
                self.popup = None;
                return true;
            }
            VK_UP => panel.first.saturating_sub(1),
            VK_DOWN => panel.first + 1,
            VK_PRIOR => panel.first.saturating_sub(page),
            VK_NEXT => panel.first + page,
            VK_HOME => 0,
            VK_END => last,
            _ => {
                self.popup_command(key, ctx);
                return true;
            }
        };
        if let Some(Popup::Panel(panel)) = &mut self.popup {
            panel.first = first.min(last);
        }
        true
    }

    /// Keys of popup commands keep working while a popup is open; other
    /// commands wait until it closes.
    fn popup_command(&mut self, key: Key, ctx: &mut Ctx) {
        if let Some(def) = commands::for_key(key)
            && matches!(
                def.action,
                Action::ShowCommands
                    | Action::ShowFavorites
                    | Action::About
                    | Action::KeyboardShortcuts
                    | Action::Quit
            )
        {
            self.perform(def.action, ctx);
        }
    }

    /// Closes the palette, records the command as recent and runs it. The
    /// command runs before the state file is written. A choice (language,
    /// symbol) is applied instead.
    fn run_palette_item(&mut self, item: usize, ctx: &mut Ctx) {
        let Some(Popup::Palette(p)) = &self.popup else {
            return;
        };
        if let Some(choice) = p.choice_at(item) {
            let (mode, query) = (p.mode, p.query.as_str().to_owned());
            self.popup = None;
            match mode {
                Mode::Language => self.set_language(choice),
                Mode::Symbol => self.go_to_symbol(choice),
                Mode::Line => self.go_to_line(&query),
                Mode::All | Mode::Favorites => {}
            }
            return;
        }
        let (Some(def), Some(index)) = (p.command_at(item), p.index_at(item)) else {
            return;
        };
        self.popup = None;
        if let Some(store) = &mut self.store {
            store.push_recent(index);
        }
        self.perform(def.action, ctx);
    }

    fn toggle_favorite(&mut self, item: usize) {
        let (Some(Popup::Palette(p)), Some(store)) = (&mut self.popup, &mut self.store) else {
            return;
        };
        let Some(index) = p.index_at(item) else {
            return;
        };
        if !store.set_favorite(index, !store.is_favorite(index)) {
            self.notice = Some(format!("At most {MAX_FAVORITES} favorites are kept"));
        }
        if p.mode == Mode::Favorites {
            p.refresh_in_place(store, self.settings.recent_count);
        }
    }

    fn popup_click(&mut self, x: i32, y: i32, ctx: &mut Ctx) {
        let cell = self.ui_cell();
        match &self.popup {
            Some(Popup::Palette(p)) => {
                let g = self.palette_geometry(p);
                if !g.panel.contains(x, y) {
                    self.popup = None;
                } else if let Some(row) = g.row_at(x, y) {
                    let item = p.first + row;
                    if g.star_area(row, cell).contains(x, y) {
                        self.toggle_favorite(item);
                    } else {
                        self.run_palette_item(item, ctx);
                    }
                }
            }
            Some(Popup::Panel(panel)) => {
                let g = self.panel_geometry(panel);
                if !g.panel.contains(x, y) {
                    self.popup = None;
                } else if let Some(line) = g.line_at(x, y)
                    && let Some(Some(url)) = panel.links.get(panel.first + line)
                {
                    ctx.open_url(url);
                }
            }
            None => {}
        }
    }

    fn popup_wheel(&mut self, delta: i32) {
        let steps = -delta.signum() as isize * WHEEL_LINES as isize;
        match &self.popup {
            Some(Popup::Palette(p)) => {
                let rows = self.palette_geometry(p).rows;
                if let Some(Popup::Palette(p)) = &mut self.popup {
                    p.scroll(steps, rows);
                }
            }
            Some(Popup::Panel(panel)) => {
                let page = self.panel_geometry(panel).lines;
                let last = panel.lines.len().saturating_sub(page);
                if let Some(Popup::Panel(panel)) = &mut self.popup {
                    panel.first = panel.first.saturating_add_signed(steps).min(last);
                }
            }
            None => {}
        }
    }

    fn scale_about_icon(&mut self, side: u32) {
        if self.about_icon.as_ref().is_some_and(|(s, _)| *s == side) {
            return;
        }
        let source: Vec<u32> = ABOUT_ICON
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&b| u32::from_le_bytes(b))
            .collect();
        let from = (source.len() as f64).sqrt() as u32;
        self.about_icon = Some((side, scale_image(&source, from, side).into()));
    }

    /// Adds the open popup to the display list.
    fn draw_popup(&mut self) {
        if matches!(&self.popup, Some(Popup::Panel(p)) if p.icon) {
            self.scale_about_icon(self.icon_side());
        }
        let cell = self.ui_cell();
        let theme = &self.theme;
        match &self.popup {
            Some(Popup::Palette(p)) => {
                let g = self.palette_geometry(p);
                let first = p.first.min(p.items.len());
                let visible = &p.items[first..(first + g.rows).min(p.items.len())];
                let (store, locks, has_path) = (&self.store, self.doc.locks, self.path.is_some());
                let rows = visible.iter().map(|item| match item {
                    Item::Header(text) => PaletteRow::Header(text),
                    Item::Command { index, matched } => {
                        let def = &COMMANDS[*index];
                        PaletteRow::Command {
                            label: def.label,
                            matched: p.matched(matched),
                            keys: commands::key_hint(*index),
                            starred: store.as_ref().is_some_and(|s| s.is_favorite(*index)),
                            enabled: enabled(def, locks, has_path),
                        }
                    }
                    Item::Choice { index, matched } => PaletteRow::Command {
                        label: &p.choices.labels[*index],
                        matched: p.matched(matched),
                        keys: &p.choices.hints[*index],
                        starred: false,
                        enabled: true,
                    },
                });
                let placeholder = match p.mode {
                    Mode::All => "Type a command name",
                    Mode::Favorites => "Search recent and favorite commands",
                    Mode::Language => "Select a language",
                    Mode::Symbol => "Go to a symbol",
                    Mode::Line => "Line, line:column or -line from the end",
                };
                let empty = match p.mode {
                    Mode::Favorites if p.query.as_str().is_empty() => {
                        "Star commands in Ctrl+Shift+P to add them here"
                    }
                    Mode::Language => "No matching languages",
                    Mode::Symbol if self.symbols_pending => "Collecting symbols…",
                    Mode::Symbol if p.choices.labels.is_empty() => "No symbols found",
                    Mode::Symbol => "No matching symbols",
                    _ => "No matching commands",
                };
                let query = p.query.as_str();
                let cols = |end: usize| query[..end].width() as u32;
                let selection = p.query.selection();
                let frame = PaletteFrame {
                    cell,
                    theme,
                    query,
                    caret: cols(p.query.caret()),
                    selection: (cols(selection.start), cols(selection.end)),
                    placeholder,
                    empty,
                    selected: p.selected.and_then(|s| s.checked_sub(first)),
                };
                draw_palette(&g, &frame, rows, &mut self.list.popup);
            }
            Some(Popup::Panel(panel)) => {
                let g = self.panel_geometry(panel);
                let first = panel.first.min(panel.lines.len());
                let frame = PanelFrame {
                    cell,
                    theme,
                    title: panel.title,
                    footer: panel.footer,
                    lines: &panel.lines[first..],
                    image: self
                        .about_icon
                        .as_ref()
                        .filter(|_| panel.icon)
                        .map(|(_, pixels)| pixels),
                };
                draw_panel(&g, &frame, &mut self.list.popup);
            }
            None => self.draw_find(),
        }
    }

    fn hit(&self, x: i32, y: i32) -> u64 {
        hit_test(
            &self.doc.snapshot,
            &self.layout(),
            self.scroll,
            self.renderer.cell(),
            &self.options,
            x,
            y,
        )
    }

    /// Whole line containing `pos`, including its line break.
    fn line_span(&self, pos: u64) -> (u64, u64) {
        let s = &self.doc.snapshot;
        let line = s.line_of(pos);
        let start = s.line_start(line).unwrap_or(0);
        (start, s.line_start(line + 1).unwrap_or(s.len()))
    }

    fn word_span(&self, pos: u64) -> (u64, u64) {
        let r = edit::word_at(&self.doc.snapshot, pos);
        (r.start, r.end)
    }

    fn mouse_down(&mut self, x: i32, y: i32, clicks: u32, shift: bool, alt: bool, middle: bool) {
        if self.prompt.is_some() || self.closing.is_some() || self.no_file() {
            return;
        }
        if self.find_open() && self.find_click(x, y) {
            return;
        }
        let layout = self.layout();
        if layout.scrollbar.contains(x, y) {
            if let Some(slider) = self.slider() {
                // A press off the slider centers it there.
                let grab = if slider.contains(x, y) {
                    y - slider.y
                } else {
                    slider.h as i32 / 2
                };
                self.drag = Some(Drag {
                    unit: Unit::Char,
                    origin: (0, 0),
                    mode: DragMode::Slider { grab },
                });
                self.drag_slider(y - grab);
            }
            return;
        }
        let body = layout.text.contains(x, y) || layout.gutter.contains(x, y);
        if !body {
            return;
        }
        self.notice = None;
        if let Some(f) = &mut self.find {
            f.focus = None;
        }
        if middle || (alt && shift) {
            let anchor = self.cell_at(x, y);
            self.drag = Some(Drag {
                unit: Unit::Char,
                origin: (0, 0),
                mode: DragMode::Column(anchor),
            });
            self.select_column(anchor, anchor);
            return;
        }
        let pos = self.hit(x, y);
        if alt && clicks == 1 {
            self.toggle_cursor(pos);
            self.drag = Some(Drag {
                unit: Unit::Char,
                origin: (pos, pos),
                mode: DragMode::Add,
            });
            return;
        }
        let (unit, origin) = match clicks {
            1 if shift => {
                let anchor = self.doc.selections.primary().anchor;
                (Unit::Char, (anchor, anchor))
            }
            1 => (Unit::Char, (pos, pos)),
            2 => (Unit::Word, self.word_span(pos)),
            _ => (Unit::Line, self.line_span(pos)),
        };
        self.drag = Some(Drag {
            unit,
            origin,
            mode: DragMode::Single,
        });
        self.drag_to(pos);
    }

    fn drag_move(&mut self, x: i32, y: i32) {
        match self.drag.map(|d| d.mode) {
            Some(DragMode::Column(anchor)) => {
                let head = self.cell_at(x, y);
                self.place_column(anchor, head, false);
            }
            Some(DragMode::Slider { grab }) => self.drag_slider(y - grab),
            Some(_) => {
                let pos = self.hit(x, y);
                self.drag_to(pos);
            }
            None => {}
        }
    }

    fn drag_to(&mut self, pos: u64) {
        let Some(Drag { unit, origin, mode }) = self.drag else {
            return;
        };
        let span = match unit {
            Unit::Char => (pos, pos),
            Unit::Word => self.word_span(pos),
            Unit::Line => self.line_span(pos),
        };
        let selection = if span.0 < origin.0 {
            Selection::new(origin.1, span.0)
        } else {
            Selection::new(origin.0, span.1.max(origin.1).max(pos))
        };
        if mode == DragMode::Add {
            let sels = &self.doc.selections;
            let mut all = sels.as_slice().to_vec();
            all[sels.primary_index()] = selection;
            let primary = sels.primary_index();
            self.doc.select(Selections::new(all, primary));
        } else {
            self.select_single(selection);
        }
        self.reveal();
    }

    fn status_text(&self) -> (String, String) {
        if self.no_file() {
            let left = self.notice.clone().unwrap_or_default();
            let mut right = Vec::new();
            if self.doc.locks.user {
                right.push("Read-only");
            }
            if !self.highlighting {
                right.push("Syntax off");
            }
            return (left, right.join("  \u{00B7}  "));
        }
        let name = self.path.as_deref().map(display_name).unwrap_or_default();
        let left = match self.prompt {
            Some(Prompt::DirtyClose(_)) => {
                format!("Save changes to {name}?   Enter: Save   D: Discard   Esc: Cancel")
            }
            Some(Prompt::Conflict(_)) => {
                format!("{name} changed on disk.   O: Overwrite   Esc: Cancel")
            }
            None if self.saving.is_some() => "Saving\u{2026}".to_owned(),
            None => match (&self.notice, &self.status) {
                (Some(notice), _) => notice.clone(),
                (None, LoadStatus::Loading { loaded, total }) => {
                    format!("Loading {}%", loaded * 100 / (*total).max(1))
                }
                (None, LoadStatus::Failed(error)) => format!("Read error: {error}"),
                (None, LoadStatus::Ready) => match self.info {
                    Some(info) if info.new_file => "New file".to_owned(),
                    Some(info) if info.malformed => "Malformed UTF-16, read-only".to_owned(),
                    _ => format!("{} lines", self.doc.snapshot.line_count()),
                },
            },
        };

        let s = &self.doc.snapshot;
        let primary = self.doc.selections.primary();
        let line = s.line_of(primary.head);
        // Past the exactly scanned bytes the column is approximate.
        let beyond = primary.head - s.line_start(line).unwrap_or(0) > ROW_SCAN_LIMIT as u64;
        let mut right = vec![format!(
            "Ln {}, Col {}{}",
            line + 1,
            if beyond { "~" } else { "" },
            self.caret_column(primary.head) + 1
        )];
        if self.doc.selections.len() > 1 {
            right.push(format!("{} selections", self.doc.selections.len()));
        } else if !primary.is_empty() {
            let range = primary.range();
            let len = range.end - range.start;
            right.push(if len <= COUNT_LIMIT {
                let chars = String::from_utf8_lossy(&s.to_vec(range)).chars().count();
                format!("{chars} selected")
            } else {
                format!("{len} bytes selected")
            });
        }
        if let Some(info) = self.info {
            right.push(
                match info.encoding {
                    Encoding::Utf8 => "UTF-8",
                    Encoding::Utf8Bom => "UTF-8 BOM",
                    Encoding::Utf16Le => "UTF-16 LE",
                    Encoding::Utf16Be => "UTF-16 BE",
                }
                .to_owned(),
            );
            right.push(
                match info.line_ending {
                    LineEnding::Lf => "LF",
                    LineEnding::CrLf => "CRLF",
                }
                .to_owned(),
            );
        }
        if self.doc.options.overtype {
            right.push("OVR".to_owned());
        }
        if !self.highlighting {
            right.push("Syntax off".to_owned());
        } else if let Some(lang) = self.language {
            right.push(lang.name.to_owned());
        }
        if !self.doc.locks.loading
            && let Some(reason) = self.doc.locks.edit_refusal()
        {
            right.push(format!("RO: {reason}"));
        }
        (left, right.join("  \u{00B7}  "))
    }
}

impl App for Editor {
    fn event(&mut self, event: Event, ctx: &mut Ctx) -> bool {
        self.size = ctx.size();
        let before = self.visible();
        let layout_changed = matches!(event, Event::Shown | Event::Resized { .. });
        let mut handled = true;
        match event {
            Event::Shown => {
                self.trace.mark("first_frame");
                if !self.settings.exit_after_load && !self.settings.exit_after_highlight {
                    self.trace.report();
                }
                if self.settings.exit_after_first_frame {
                    ctx.close();
                }
                self.shown = true;
                self.exit_if_loaded(ctx);
            }
            Event::Resized { .. } => {}
            Event::DpiChanged(dpi) => {
                self.dpi = dpi;
                let px = self.px();
                self.renderer.set_size(px);
            }
            Event::Key(key) => {
                if let Some(bench) = &mut self.bench {
                    bench.arrived(bench::Step::Key(key));
                }
                handled = self.key(key, ctx);
            }
            Event::Char(ch) => {
                if let Some(bench) = &mut self.bench {
                    bench.arrived(bench::Step::Char(ch));
                }
                self.typed(ch);
            }
            Event::Wheel {
                delta,
                horizontal,
                shift,
                ctrl,
            } => {
                if self.popup.is_some() {
                    self.popup_wheel(delta);
                } else if ctrl && !horizontal {
                    self.set_zoom(self.zoom + delta.signum());
                } else if horizontal || shift {
                    // Positive horizontal wheel delta scrolls right; vertical with Shift mirrors it.
                    let delta = if horizontal { delta } else { -delta };
                    self.wheel.1 += delta;
                    let notches = self.wheel.1 / WHEEL_NOTCH;
                    self.wheel.1 %= WHEEL_NOTCH;
                    self.scroll_cols(i64::from(notches * WHEEL_COLUMNS));
                } else {
                    self.wheel.0 += delta;
                    let notches = self.wheel.0 * WHEEL_LINES / WHEEL_NOTCH;
                    self.wheel.0 -= notches * WHEEL_NOTCH / WHEEL_LINES;
                    self.scroll_lines(-i64::from(notches));
                }
            }
            Event::MouseDown {
                x,
                y,
                clicks,
                shift,
                alt,
                middle,
                ..
            } => {
                if self.popup.is_some() {
                    if !middle {
                        self.popup_click(x, y, ctx);
                    }
                } else {
                    self.mouse_down(x, y, clicks, shift, alt, middle);
                }
            }
            Event::MouseMove { x, y } => {
                if self.drag.is_some() {
                    self.drag_move(x, y);
                }
            }
            Event::MouseUp => self.drag = None,
            Event::CloseRequested => self.request_close(ctx),
            Event::FilesDropped(dropped) => self.files_dropped(dropped),
            Event::FullScreen(Ok(on)) => self.full_screen = on,
            Event::FullScreen(Err(reason)) => {
                self.notice = Some(format!("Full screen failed: {reason}"));
            }
            Event::Wake => {
                self.drain_pending();
                self.drain_loads();
                self.drain_saves(ctx);
                self.drain_store();
                self.drain_search();
                self.drain_syntax();
                self.exit_if_loaded(ctx);
            }
        }
        self.sync_syntax();
        let title = window_title(self.path.as_deref(), self.doc.is_dirty());
        if title != self.shown_title {
            ctx.set_title(&title);
            self.shown_title = title;
        }
        if self.prompt.is_some() {
            self.popup = None;
        }
        self.end_line_preview();
        self.sync_find();
        self.sync_ui_renderer();
        let redraw = layout_changed || !self.visible().same(&before);
        if redraw {
            ctx.redraw();
        }
        let busy = self.bench.is_some() && (self.search_busy() || self.symbols_pending);
        // Scripts start once the file is loaded and, with syntax, highlighted.
        let full = self.bench.as_ref().is_some_and(bench::Driver::full);
        let ready = self.shown
            && !matches!(self.status, LoadStatus::Loading { .. })
            && self.syntax.as_ref().is_none_or(|s| {
                if full {
                    s.fully_highlighted()
                } else {
                    s.highlighted()
                }
            });
        if ready && let Some(r) = self.bench.as_mut().and_then(bench::Driver::take_select) {
            let len = self.doc.snapshot.len();
            self.doc.select(Selections::single(Selection::new(
                r.start.min(len),
                r.end.min(len),
            )));
            ctx.redraw();
        }
        if let Some(bench) = &mut self.bench {
            bench.check_frame(redraw, busy, ctx);
            if ready {
                bench.send(ctx);
            }
        }
        handled
    }

    fn paint(&mut self, pixels: &mut [u32], width: u32, height: u32) {
        self.size = (width, height);
        let cell = self.renderer.cell();
        let layout = self.layout();
        let (left, right) = self.status_text();
        let title_mark = self.doc.is_dirty().then_some(self.theme.unsaved);
        let caret = if self.prompt.is_some() || self.no_file() {
            CaretStyle::Hidden
        } else if self.doc.options.overtype {
            CaretStyle::Underline
        } else {
            CaretStyle::Bar
        };
        self.sync_syntax();
        let slider_active = self.slider_active();
        let brackets: Vec<u64> = self
            .bracket_pair()
            .map(|(a, b)| vec![a.min(b), a.max(b)])
            .unwrap_or_default();
        let ranges = match self.syntax {
            Some(_) => self.visible_ranges(),
            None => Vec::new(),
        };
        let syntax = match &mut self.syntax {
            Some(syntax) => syntax.spans(&self.doc.snapshot, &ranges),
            None => &[],
        };
        let frame = Frame {
            snapshot: &self.doc.snapshot,
            layout: &layout,
            scroll: self.scroll,
            cell,
            theme: &self.theme,
            chrome: Chrome {
                title: &self.title,
                title_mark,
                app_name: APP_NAME,
                status_left: &left,
                status_right: &right,
                prompt: self.prompt.is_some(),
            },
            options: &self.options,
            selections: self.doc.selections.as_slice(),
            caret,
            highlights: {
                let (matches, scope) = find::highlights(self.find.as_deref(), &self.doc.snapshot);
                Highlights {
                    matches,
                    scope,
                    syntax,
                    brackets: &brackets,
                }
            },
            empty: self.path.is_none().then_some(EMPTY_HINT),
            slider_active,
        };
        self.view.build(&frame, &mut self.list);
        self.draw_popup();
        let mut target = Target {
            pixels,
            width,
            height,
        };
        self.renderer.draw(&self.list, &mut target);
        let ui = self.ui_renderer.as_mut().unwrap_or(&mut self.renderer);
        ui.draw_layer(&self.list.popup, &mut target);
        if !self.painted {
            self.painted = true;
            self.trace.mark("first_paint");
        }
    }

    fn presented(&mut self, ctx: &mut Ctx) {
        self.mark_highlights(ctx);
        let busy = self.bench.is_some() && (self.search_busy() || self.symbols_pending);
        let highlighted = self.syntax.as_ref().map(Syntax::highlighted);
        if let Some(bench) = &mut self.bench {
            bench.presented(busy, highlighted, ctx);
        }
    }

    fn is_caption(&self, x: i32, y: i32) -> bool {
        self.layout().title.contains(x, y)
    }

    fn is_text(&self, x: i32, y: i32) -> bool {
        !self.no_file()
            && self.layout().text.contains(x, y)
            && !self.popup_area().is_some_and(|r| r.contains(x, y))
            && !self
                .find_geometry()
                .is_some_and(|g| g.panel.contains(x, y) || g.message.contains(x, y))
    }
}

/// Whether `def` can run under `locks`; without a file only the commands
/// of the empty state run.
fn enabled(def: &CommandDef, locks: Locks, has_path: bool) -> bool {
    if !has_path {
        return works_without_file(def.action);
    }
    match def.action {
        Action::Save => locks.save_refusal().is_none(),
        _ => !def.mutates || locks.edit_refusal().is_none(),
    }
}

/// The About panel: name, version, links and the manifest.
fn about_panel() -> Panel {
    let centered = |spans: &[(&str, Tone)]| PanelLine {
        centered: true,
        ..PanelLine::new(spans)
    };
    let mut lines = vec![
        centered(&[(APP_NAME, Tone::Normal)]),
        centered(&[("Version ", Tone::Dim), (VERSION, Tone::Normal)]),
        PanelLine::default(),
        PanelLine::new(&[("Repository     ", Tone::Dim), (REPOSITORY_URL, Tone::Link)]),
        PanelLine::new(&[
            ("Documentation  ", Tone::Dim),
            (DOCUMENTATION_URL, Tone::Link),
        ]),
        PanelLine::default(),
    ];
    let mut links = vec![
        None,
        None,
        None,
        Some(REPOSITORY_URL),
        Some(DOCUMENTATION_URL),
    ];
    for line in MANIFEST.lines().map(str::trim).filter(|l| !l.is_empty()) {
        lines.push(match line.strip_prefix('#') {
            Some(title) => centered(&[(title.trim_start_matches('#').trim(), Tone::Heading)]),
            None => centered(&[(line, Tone::Normal)]),
        });
    }
    links.resize(lines.len(), None);
    Panel {
        title: "About",
        footer: "Click a link to open it  \u{00B7}  Esc: Close",
        lines,
        links,
        icon: true,
        first: 0,
    }
}

/// The Keyboard Shortcuts panel: every command with its keys, then the
/// editing keys.
fn shortcuts_panel() -> Panel {
    let width = COMMANDS
        .iter()
        .map(|c| c.label.len())
        .chain(EDITING_KEYS.iter().map(|(name, _)| name.len()))
        .max()
        .unwrap_or(0)
        + 3;
    let row = |name: &str, keys: &str, tone: Tone, keys_tone: Tone| {
        PanelLine::new(&[(&format!("{name:<width$}"), tone), (keys, keys_tone)])
    };
    let mut lines = vec![row("Command", "Keys", Tone::Heading, Tone::Heading)];
    for c in COMMANDS {
        let keys = commands::keys_text(c);
        lines.push(if keys.is_empty() {
            row(c.label, "Palette only", Tone::Normal, Tone::Dim)
        } else {
            row(c.label, &keys, Tone::Normal, Tone::Normal)
        });
    }
    lines.push(PanelLine::default());
    lines.push(row("Editing", "Keys", Tone::Heading, Tone::Heading));
    for (name, keys) in EDITING_KEYS {
        lines.push(row(name, keys, Tone::Normal, Tone::Normal));
    }
    Panel {
        title: "Keyboard Shortcuts",
        footer: "Up, Down, PgUp, PgDn: Scroll  \u{00B7}  Esc: Close",
        links: Vec::new(),
        icon: false,
        first: 0,
        lines,
    }
}

/// Resizes a square image of premultiplied pixels with a box filter.
fn scale_image(source: &[u32], from: u32, to: u32) -> Vec<u32> {
    let span = |i: u32| {
        let start = i * from / to;
        (start, ((i + 1) * from / to).max(start + 1))
    };
    let mut out = Vec::with_capacity((to * to) as usize);
    for y in 0..to {
        let (y0, y1) = span(y);
        for x in 0..to {
            let (x0, x1) = span(x);
            let mut sum = [0u32; 4];
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let p = source[(sy * from + sx) as usize];
                    for (k, s) in sum.iter_mut().enumerate() {
                        *s += (p >> (24 - 8 * k)) & 0xFF;
                    }
                }
            }
            let n = (y1 - y0) * (x1 - x0);
            let pixel = sum
                .iter()
                .enumerate()
                .fold(0, |acc, (k, s)| acc | ((s + n / 2) / n) << (24 - 8 * k));
            out.push(pixel);
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::single_range_in_vec_init)]
mod tests {
    use super::*;
    use std::borrow::Cow;
    use std::sync::mpsc;
    use std::time::Duration;

    use crate::save::fake::FakeFiles;

    fn path() -> PathBuf {
        PathBuf::from(r"C:\docs\test.txt")
    }

    fn renderer() -> Renderer {
        Renderer::new_with_bold(
            Cow::Borrowed(rte_assets::DEFAULT_FONT),
            Cow::Borrowed(rte_assets::BOLD_FONT),
            14.0,
        )
        .unwrap()
    }

    fn settings(read_only: bool, exit_after_load: bool) -> Settings {
        Settings {
            read_only,
            exit_after_first_frame: false,
            exit_after_load,
            exit_after_highlight: false,
            recent_count: 3,
            multi_cursor_limit: 10_000,
            replace_all_max_mib: 512,
            user_dir: None,
            notice: None,
            highlighting: true,
            syntax: None,
            syntax_max_mib: 32,
            injections: true,
            early_syntax: None,
            bench: None,
        }
    }

    fn info() -> FileInfo {
        FileInfo {
            encoding: Encoding::Utf8,
            line_ending: LineEnding::Lf,
            indent_tabs: false,
            malformed: false,
            new_file: false,
        }
    }

    fn done(text: &[u8]) -> LoadEvent {
        LoadEvent::Done {
            snapshot: Snapshot::from_bytes(text),
            info: info(),
            baseline: None,
            read_ms: 0.0,
        }
    }

    fn loading_editor(first: Option<LoadEvent>, loads: Receiver<LoadEvent>) -> Editor {
        Editor::new(
            Some(&path()),
            first,
            Some(Load::new(loads)),
            renderer(),
            96,
            settings(false, true),
            Trace::new(false),
            Arc::new(FakeFiles::default()),
            Arc::new(|| {}),
        )
    }

    /// An editor on `text`, stored in `files` at [`path`].
    fn editor_on(files: &Arc<FakeFiles>, text: &[u8], read_only: bool) -> Editor {
        files.put(&path(), text);
        let (tx, rx) = mpsc::channel();
        tx.send(LoadEvent::Done {
            snapshot: Snapshot::from_bytes(text),
            info: info(),
            baseline: Some(files.baseline(&path())),
            read_ms: 0.0,
        })
        .unwrap();
        drop(tx);
        Editor::new(
            Some(&path()),
            None,
            Some(Load::new(rx)),
            renderer(),
            96,
            settings(read_only, false),
            Trace::new(false),
            files.clone(),
            Arc::new(|| {}),
        )
    }

    fn text(editor: &Editor) -> String {
        String::from_utf8(editor.doc.snapshot.to_vec(0..editor.doc.snapshot.len())).unwrap()
    }

    fn key(vk: u16, ctrl: bool, shift: bool) -> Event {
        Event::Key(Key {
            vk,
            ctrl,
            shift,
            alt: false,
        })
    }

    fn type_text(editor: &mut Editor, ctx: &mut Ctx, text: &str) {
        for ch in text.chars() {
            editor.event(Event::Char(ch), ctx);
        }
    }

    /// Delivers wake-ups until the running save finished.
    fn settle(editor: &mut Editor, ctx: &mut Ctx) {
        for _ in 0..5000 {
            if editor.saving.is_none() {
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
            editor.event(Event::Wake, ctx);
        }
        panic!("save did not finish");
    }

    #[test]
    fn loader_finished_before_the_waker_is_applied_at_startup() {
        // Progress and Done are both queued before any wake-up could be delivered.
        let (tx, rx) = mpsc::channel();
        tx.send(LoadEvent::Progress {
            snapshot: Snapshot::from_bytes(b"pre"),
            loaded: 3,
            total: 6,
        })
        .unwrap();
        tx.send(done(b"prefix")).unwrap();
        let first = rx.recv().ok();
        let mut editor = loading_editor(first, rx);
        assert!(matches!(editor.status, LoadStatus::Ready));
        assert!(!editor.doc.locks.loading);
        assert_eq!(text(&editor), "prefix");
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(Event::Shown, &mut ctx);
        assert!(ctx.close_requested(), "--exit-after-load must close");
    }

    #[test]
    fn loader_that_stops_without_a_final_event_is_reported() {
        let (tx, rx) = mpsc::channel();
        tx.send(LoadEvent::Progress {
            snapshot: Snapshot::from_bytes(b"part"),
            loaded: 4,
            total: 100,
        })
        .unwrap();
        drop(tx);
        let first = rx.recv().ok();
        let editor = loading_editor(first, rx);
        assert!(matches!(editor.status, LoadStatus::Failed(_)));
        assert!(editor.doc.locks.load_error);
        assert_eq!(editor.doc.snapshot.len(), 4);
    }

    #[test]
    fn late_events_are_applied_on_wake() {
        let (tx, rx) = mpsc::channel();
        tx.send(LoadEvent::Progress {
            snapshot: Snapshot::from_bytes(b"a"),
            loaded: 1,
            total: 2,
        })
        .unwrap();
        let first = rx.recv().ok();
        let mut editor = loading_editor(first, rx);
        assert!(matches!(editor.status, LoadStatus::Loading { .. }));
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(Event::Char('x'), &mut ctx);
        assert_eq!(text(&editor), "a", "the loading prefix is read-only");
        tx.send(done(b"ab")).unwrap();
        editor.event(Event::Wake, &mut ctx);
        assert!(matches!(editor.status, LoadStatus::Ready));
        assert!(ctx.close_requested());
    }

    #[test]
    fn typing_moving_and_undo() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, b"one\ntwo", false);
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(key(VK_END, false, false), &mut ctx);
        type_text(&mut editor, &mut ctx, "ab cd");
        assert_eq!(text(&editor), "oneab cd\ntwo");
        assert_eq!(
            ctx.requested_title(),
            Some(format!("\u{25CF} test.txt - {APP_NAME}").as_str())
        );
        editor.event(key(VK_DOWN, false, true), &mut ctx);
        assert_eq!(
            editor.doc.selections.primary(),
            Selection {
                anchor: 8,
                head: 12,
                goal: Some(8)
            }
        );
        editor.event(key(VK_RETURN, false, false), &mut ctx);
        assert_eq!(text(&editor), "oneab cd\n");
        editor.event(key(VK_Z, true, false), &mut ctx);
        assert_eq!(text(&editor), "oneab cd\ntwo");
        editor.event(key(VK_Z, true, false), &mut ctx);
        editor.event(key(VK_Z, true, false), &mut ctx);
        assert_eq!(text(&editor), "one\ntwo");
        assert_eq!(
            ctx.requested_title(),
            Some(format!("test.txt - {APP_NAME}").as_str())
        );
        editor.event(key(VK_Y, true, false), &mut ctx);
        assert_eq!(text(&editor), "oneab \ntwo");
    }

    #[test]
    fn read_only_refuses_edits_with_a_notice() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, b"abc", true);
        let mut ctx = Ctx::headless(800, 600, 96);
        type_text(&mut editor, &mut ctx, "x");
        editor.event(key(VK_DELETE, false, false), &mut ctx);
        assert_eq!(text(&editor), "abc");
        assert_eq!(editor.notice.as_deref(), Some("Not editable: read-only"));
        assert!(editor.status_text().1.contains("RO: read-only"));
        editor.event(key(VK_RIGHT, false, true), &mut ctx);
        assert_eq!(editor.doc.selections.primary(), Selection::new(0, 1));
    }

    #[test]
    fn line_copy_and_paste_through_the_clipboard() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, b"one\ntwo", false);
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(key(VK_C, true, false), &mut ctx);
        editor.event(key(VK_DOWN, false, false), &mut ctx);
        editor.event(key(VK_V, true, false), &mut ctx);
        assert_eq!(text(&editor), "one\none\ntwo");
        editor.event(key(VK_X, true, false), &mut ctx);
        assert_eq!(text(&editor), "one\none");
        editor.event(key(VK_A, true, false), &mut ctx);
        editor.event(key(VK_V, true, false), &mut ctx);
        assert_eq!(text(&editor), "two\n");
    }

    #[test]
    fn starting_without_a_file_shows_the_empty_state() {
        let mut editor = empty_editor(false);
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(Event::Shown, &mut ctx);
        assert!(editor.no_file() && editor.doc.locks.no_file);
        assert_eq!(window_title(None, false), APP_NAME);
        // Typing, clicks and editing commands change nothing.
        type_text(&mut editor, &mut ctx, "x");
        editor.event(click(200, 200), &mut ctx);
        editor.event(key(VK_V, true, false), &mut ctx);
        assert!(editor.doc.snapshot.is_empty());
        editor.event(key(VK_S, true, false), &mut ctx);
        assert!(editor.saving.is_none());
        assert_eq!(editor.notice.as_deref(), Some("No file is open"));
        editor.event(key(VK_F, true, false), &mut ctx);
        assert!(!editor.find_open());
        // Close keeps the window; the palette dims all but the empty-state commands.
        editor.event(key(VK_W, true, false), &mut ctx);
        assert!(editor.notice.as_deref().unwrap().contains("Alt+F4"));
        assert!(!ctx.close_requested());
        let locks = editor.doc.locks;
        for def in COMMANDS {
            assert_eq!(
                enabled(def, locks, false),
                works_without_file(def.action),
                "{}",
                def.label
            );
        }
        let (left, right) = editor.status_text();
        assert!(left.contains("Alt+F4"), "the notice: {left}");
        assert_eq!(right, "", "no position, encoding or language");
        // Quit needs no answer.
        editor.event(key(VK_W, true, true), &mut ctx);
        assert!(ctx.close_requested());
        let mut editor = empty_editor(false);
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(Event::CloseRequested, &mut ctx);
        assert!(ctx.close_requested());
    }

    #[test]
    fn save_writes_and_cleans() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, b"abc", false);
        let mut ctx = Ctx::headless(800, 600, 96);
        type_text(&mut editor, &mut ctx, "x");
        editor.event(key(VK_S, true, false), &mut ctx);
        settle(&mut editor, &mut ctx);
        assert_eq!(files.get(&path()).unwrap(), b"xabc");
        assert!(!editor.doc.is_dirty());
        assert_eq!(editor.notice.as_deref(), Some("Saved"));
        assert_eq!(editor.baseline, Some(files.baseline(&path())));
    }

    #[test]
    fn queued_save_uses_the_text_at_its_start_and_edits_stay_dirty() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, b"", false);
        let (open, gate) = mpsc::channel();
        *files.gate.lock().unwrap() = Some(gate);
        let mut ctx = Ctx::headless(800, 600, 96);
        type_text(&mut editor, &mut ctx, "a");
        editor.event(key(VK_S, true, false), &mut ctx);
        type_text(&mut editor, &mut ctx, "b");
        editor.event(key(VK_S, true, false), &mut ctx);
        type_text(&mut editor, &mut ctx, "c");
        editor.event(key(VK_S, true, false), &mut ctx);
        assert!(editor.save_queued);
        open.send(()).unwrap();
        // First save: "a"; the queued one starts with the current text.
        for _ in 0..5000 {
            if !editor.save_queued && editor.saving.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
            editor.event(Event::Wake, &mut ctx);
        }
        assert!(
            editor.doc.is_dirty(),
            "edits made during the save stay unsaved"
        );
        settle(&mut editor, &mut ctx);
        assert_eq!(files.get(&path()).unwrap(), b"abc");
        assert!(!editor.doc.is_dirty());
    }

    #[test]
    fn close_during_a_save_waits_and_then_closes() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, b"", false);
        let (open, gate) = mpsc::channel();
        *files.gate.lock().unwrap() = Some(gate);
        let mut ctx = Ctx::headless(800, 600, 96);
        type_text(&mut editor, &mut ctx, "a");
        editor.event(key(VK_S, true, false), &mut ctx);
        editor.event(Event::CloseRequested, &mut ctx);
        type_text(&mut editor, &mut ctx, "ignored");
        assert_eq!(editor.status_text().0, "Saving\u{2026}");
        assert!(!ctx.close_requested());
        open.send(()).unwrap();
        settle(&mut editor, &mut ctx);
        assert!(ctx.close_requested());
        assert_eq!(files.get(&path()).unwrap(), b"a");
    }

    #[test]
    fn dirty_close_prompt() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, b"", false);
        let mut ctx = Ctx::headless(800, 600, 96);
        type_text(&mut editor, &mut ctx, "a");
        editor.event(Event::CloseRequested, &mut ctx);
        assert_eq!(editor.prompt, Some(Prompt::DirtyClose(Exit::Quit)));
        let prompt = editor.status_text().0;
        assert!(prompt.contains("D: Discard"));
        assert!(!prompt.contains("Don't Save"));
        editor.event(key(VK_ESCAPE, false, false), &mut ctx);
        assert_eq!(editor.prompt, None);
        assert!(!ctx.close_requested());
        // Close asks too; after the save the file closes and the session stays.
        editor.event(key(VK_W, true, false), &mut ctx);
        assert_eq!(editor.prompt, Some(Prompt::DirtyClose(Exit::CloseFile)));
        editor.event(key(VK_RETURN, false, false), &mut ctx);
        settle(&mut editor, &mut ctx);
        assert!(!ctx.close_requested());
        assert!(editor.no_file());
        assert_eq!(files.get(&path()).unwrap(), b"a");

        let mut editor = editor_on(&files, b"", false);
        let mut ctx = Ctx::headless(800, 600, 96);
        type_text(&mut editor, &mut ctx, "z");
        editor.event(Event::CloseRequested, &mut ctx);
        editor.event(key(VK_D, false, false), &mut ctx);
        assert!(ctx.close_requested());
        assert_eq!(files.get(&path()).unwrap(), b"");
    }

    #[test]
    fn conflict_prompt_overwrites_only_on_request() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, b"old", false);
        files.put(&path(), b"changed elsewhere");
        let mut ctx = Ctx::headless(800, 600, 96);
        type_text(&mut editor, &mut ctx, "x");
        editor.event(key(VK_S, true, false), &mut ctx);
        settle(&mut editor, &mut ctx);
        assert!(matches!(editor.prompt, Some(Prompt::Conflict(_))));
        assert_eq!(files.get(&path()).unwrap(), b"changed elsewhere");
        editor.event(key(VK_O, false, false), &mut ctx);
        settle(&mut editor, &mut ctx);
        assert_eq!(files.get(&path()).unwrap(), b"xold");
        assert!(!editor.doc.is_dirty());
    }

    #[test]
    fn typing_during_a_save_stays_a_separate_undo_step() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, b"", false);
        let (open, gate) = mpsc::channel();
        *files.gate.lock().unwrap() = Some(gate);
        let mut ctx = Ctx::headless(800, 600, 96);
        type_text(&mut editor, &mut ctx, "a");
        editor.event(key(VK_S, true, false), &mut ctx);
        type_text(&mut editor, &mut ctx, "b");
        open.send(()).unwrap();
        settle(&mut editor, &mut ctx);
        assert!(editor.doc.is_dirty());
        editor.event(key(VK_Z, true, false), &mut ctx);
        assert_eq!(text(&editor), "a");
        assert!(!editor.doc.is_dirty(), "undo reaches the saved text");
    }

    /// An editor started without a file.
    fn empty_editor(read_only: bool) -> Editor {
        Editor::new(
            None,
            None,
            None,
            renderer(),
            96,
            settings(read_only, false),
            Trace::new(false),
            Arc::new(FakeFiles::default()),
            Arc::new(|| {}),
        )
    }

    /// A real file for drops (the loader reads the disk), removed on drop.
    struct TempFile(PathBuf);

    impl TempFile {
        fn new(name: &str, text: &[u8]) -> TempFile {
            // Unique per call: tests run in parallel and reuse names.
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let dir = std::env::temp_dir().join(format!(
                "rte_drop_{}_{n}_{}",
                std::process::id(),
                name.replace('.', "_")
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join(name);
            std::fs::write(&path, text).unwrap();
            TempFile(path)
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            if let Some(dir) = self.0.parent() {
                let _ = std::fs::remove_dir_all(dir);
            }
        }
    }

    fn drop_file(editor: &mut Editor, ctx: &mut Ctx, path: &Path, others: u32) {
        let dropped = Dropped {
            path: path.to_path_buf(),
            others,
        };
        editor.event(Event::FilesDropped(Ok(dropped)), ctx);
    }

    /// Wakes the editor until no dropped file is pending and none is loading.
    fn settle_open(editor: &mut Editor, ctx: &mut Ctx) {
        pump(editor, ctx, |e| {
            e.pending.is_none() && !matches!(e.status, LoadStatus::Loading { .. })
        });
    }

    #[test]
    fn dropping_a_file_opens_it_in_the_empty_state() {
        let file = TempFile::new("dropped.txt", b"hello");
        let mut editor = empty_editor(false);
        let mut ctx = Ctx::headless(800, 600, 96);
        drop_file(&mut editor, &mut ctx, &file.0, 0);
        assert!(
            editor
                .notice
                .as_deref()
                .unwrap()
                .starts_with("Opening dropped.txt")
        );
        settle_open(&mut editor, &mut ctx);
        assert_eq!(editor.path.as_deref(), Some(file.0.as_path()));
        assert_eq!(text(&editor), "hello");
        assert!(!editor.doc.locks.no_file);
        assert_eq!(
            ctx.requested_title(),
            Some(window_title(Some(&file.0), false).as_str())
        );
        type_text(&mut editor, &mut ctx, "x");
        assert_eq!(text(&editor), "xhello", "editable");
    }

    #[test]
    fn a_dropped_file_replaces_a_clean_file_and_resets_its_state() {
        let file = TempFile::new("next.py", b"def f():\n    pass\n");
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, b"cat cat cat\n", false);
        let mut ctx = Ctx::headless(800, 600, 96);
        // History, cursor history, a toggled lock and a search with a scope.
        type_text(&mut editor, &mut ctx, "x");
        editor.event(key(VK_Z, true, false), &mut ctx);
        editor.event(key(VK_D, true, false), &mut ctx);
        editor.event(key(VK_F, true, false), &mut ctx);
        type_text(&mut editor, &mut ctx, "cat");
        editor.find.as_mut().unwrap().scope = Some(0..3);
        editor.event(key(VK_ESCAPE, false, false), &mut ctx);
        assert!(!editor.doc.is_dirty());
        drop_file(&mut editor, &mut ctx, &file.0, 2);
        settle_open(&mut editor, &mut ctx);
        assert_eq!(text(&editor), "def f():\n    pass\n");
        assert_eq!(editor.doc.history.undo_len(), 0);
        assert_eq!(editor.doc.history.redo_len(), 0);
        assert!(editor.cursor_history.is_empty());
        let f = editor.find.as_ref().unwrap();
        assert!(!f.open && f.scope.is_none());
        assert!(f.find.as_str().starts_with("cat"), "the query stays");
        assert_eq!(editor.language.map(|l| l.id), Some("python"));
        assert!(editor.syntax.is_some());
        assert_eq!(
            editor.notice.as_deref(),
            Some("Only one file opens at a time; 2 more ignored")
        );
    }

    #[test]
    fn drops_onto_unsaved_changes_or_a_running_save_are_refused() {
        let file = TempFile::new("other.txt", b"other");
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, b"mine", false);
        let mut ctx = Ctx::headless(800, 600, 96);
        type_text(&mut editor, &mut ctx, "x");
        drop_file(&mut editor, &mut ctx, &file.0, 0);
        assert!(editor.pending.is_none());
        assert_eq!(
            editor.notice.as_deref(),
            Some("test.txt has unsaved changes; save or close it to open other.txt")
        );
        assert_eq!(text(&editor), "xmine");
        // During a save.
        let (open, gate) = mpsc::channel();
        *files.gate.lock().unwrap() = Some(gate);
        editor.event(key(VK_S, true, false), &mut ctx);
        drop_file(&mut editor, &mut ctx, &file.0, 0);
        assert!(editor.notice.as_deref().unwrap().contains("is being saved"));
        open.send(()).unwrap();
        settle(&mut editor, &mut ctx);
        assert_eq!(text(&editor), "xmine");
        // Edited while the dropped file loads: the file stays.
        drop_file(&mut editor, &mut ctx, &file.0, 0);
        assert!(editor.pending.is_some());
        type_text(&mut editor, &mut ctx, "y");
        settle_open(&mut editor, &mut ctx);
        assert_eq!(text(&editor), "xymine");
        assert!(
            editor
                .notice
                .as_deref()
                .unwrap()
                .contains("unsaved changes")
        );
    }

    #[test]
    fn folders_failed_drops_and_prompts_leave_the_file_as_it_is() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, &b"line\n".repeat(200), false);
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(key(VK_DOWN, true, false), &mut ctx);
        editor.event(key(VK_END, true, false), &mut ctx);
        let (scroll, sels) = (editor.scroll, editor.doc.selections.clone());
        drop_file(&mut editor, &mut ctx, &std::env::temp_dir(), 0);
        settle_open(&mut editor, &mut ctx);
        assert_eq!(editor.notice.as_deref(), Some("Folders cannot be opened"));
        assert_eq!(editor.path, Some(path()));
        assert_eq!((editor.scroll, &editor.doc.selections), (scroll, &sels));
        // A drop the platform could not read: one notice, nothing else.
        let error = "The dropped file name could not be read".to_owned();
        editor.event(Event::FilesDropped(Err(error.clone())), &mut ctx);
        assert_eq!(editor.notice.as_deref(), Some(error.as_str()));
        assert!(editor.pending.is_none());
        // While a question is shown, drops are ignored.
        type_text(&mut editor, &mut ctx, "x");
        editor.event(key(VK_W, true, false), &mut ctx);
        let file = TempFile::new("ignored.txt", b"");
        drop_file(&mut editor, &mut ctx, &file.0, 0);
        assert!(editor.pending.is_none());
        assert!(editor.prompt.is_some());
    }

    #[test]
    fn closing_returns_to_the_empty_state() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, b"abc", false);
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(key(VK_W, true, false), &mut ctx);
        assert!(editor.no_file() && !ctx.close_requested());
        assert_eq!(ctx.requested_title(), Some(APP_NAME));

        let mut editor = editor_on(&files, b"abc", false);
        type_text(&mut editor, &mut ctx, "x");
        editor.event(key(VK_F4, true, false), &mut ctx);
        assert_eq!(editor.prompt, Some(Prompt::DirtyClose(Exit::CloseFile)));
        editor.event(key(VK_ESCAPE, false, false), &mut ctx);
        assert!(!editor.no_file());
        editor.event(key(VK_W, true, false), &mut ctx);
        editor.event(key(VK_D, false, false), &mut ctx);
        assert!(editor.no_file() && !ctx.close_requested());
        assert_eq!(
            files.get(&path()).unwrap(),
            b"abc",
            "Don't Save keeps the file"
        );
        // Quit from the empty state.
        let alt_f4 = Event::Key(Key {
            vk: VK_F4,
            ctrl: false,
            shift: false,
            alt: true,
        });
        editor.event(alt_f4, &mut ctx);
        assert!(ctx.close_requested());
    }

    #[test]
    fn read_only_and_the_syntax_choice_follow_their_files() {
        let file = TempFile::new("next.py", b"x = 1\n");
        let files = Arc::new(FakeFiles::default());
        files.put(&path(), b"{}");
        let (tx, rx) = mpsc::channel();
        tx.send(done(b"{}")).unwrap();
        drop(tx);
        let mut settings = settings(true, false);
        settings.syntax = Some("json".to_owned());
        let mut editor = Editor::new(
            Some(&path()),
            None,
            Some(Load::new(rx)),
            renderer(),
            96,
            settings,
            Trace::new(false),
            files,
            Arc::new(|| {}),
        );
        let mut ctx = Ctx::headless(800, 600, 96);
        assert_eq!(editor.language.map(|l| l.id), Some("json"));
        // Toggled off for this file; the next one is read-only again.
        editor.perform(Action::ToggleReadOnly, &mut ctx);
        assert!(!editor.doc.locks.user);
        drop_file(&mut editor, &mut ctx, &file.0, 0);
        settle_open(&mut editor, &mut ctx);
        assert!(editor.doc.locks.user);
        assert_eq!(editor.language.map(|l| l.id), Some("python"));
    }

    #[test]
    fn a_syntax_choice_without_a_file_starts_no_syntax_work() {
        let file = TempFile::new("next.py", b"x = 1\n");
        let mut settings = settings(false, false);
        settings.syntax = Some("json".to_owned());
        let mut editor = Editor::new(
            None,
            None,
            None,
            renderer(),
            96,
            settings,
            Trace::new(false),
            Arc::new(FakeFiles::default()),
            Arc::new(|| {}),
        );
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(Event::Shown, &mut ctx);
        assert!(
            editor.language.is_none(),
            "--syntax waits for a command-line file"
        );
        for _ in 0..2 {
            editor.perform(Action::ToggleSyntax, &mut ctx);
            assert!(editor.no_file() && editor.syntax.is_none());
        }
        assert!(editor.highlighting, "the toggle stays a preference");
        drop_file(&mut editor, &mut ctx, &file.0, 0);
        settle_open(&mut editor, &mut ctx);
        assert_eq!(editor.language.map(|l| l.id), Some("python"));
        assert!(editor.syntax.is_some());
    }

    #[test]
    fn a_new_file_searches_after_a_busy_search_of_the_old_one() {
        let file = TempFile::new("b.txt", b"cat dog cat");
        let files = Arc::new(FakeFiles::default());
        let text_a = "cat ".repeat(50_000);
        let mut editor = editor_on(&files, text_a.as_bytes(), false);
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(key(VK_F, true, false), &mut ctx);
        // Many case-insensitive queries: generations climb on the search thread.
        for _ in 0..30 {
            type_text(&mut editor, &mut ctx, "c");
            editor.event(key(VK_BACK, false, false), &mut ctx);
        }
        type_text(&mut editor, &mut ctx, "cat");
        let old = editor.find.as_ref().unwrap().generation;
        drop_file(&mut editor, &mut ctx, &file.0, 0);
        settle_open(&mut editor, &mut ctx);
        assert!(editor.find.as_ref().unwrap().generation > old);
        std::thread::sleep(Duration::from_millis(50));
        assert!(
            !editor.search.as_ref().unwrap().holds_checkpoints(),
            "the old text is released"
        );
        editor.event(key(VK_F, true, false), &mut ctx);
        pump(&mut editor, &mut ctx, |e| counter(e).ends_with("of 2"));
        assert_eq!(text(&editor), "cat dog cat");
    }

    #[test]
    fn only_visible_changes_request_a_repaint() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, b"abc", false);
        let unchanged = [
            key(0x7B, false, false), // F12: not bound
            Event::Char('\u{1}'),
            key(VK_LEFT, false, false), // already at the start
            Event::Wheel {
                delta: 10,
                horizontal: false,
                shift: false,
                ctrl: false,
            },
        ];
        for event in unchanged {
            let mut ctx = Ctx::headless(800, 600, 96);
            editor.event(event.clone(), &mut ctx);
            assert!(!ctx.redraw_requested(), "{event:?}");
        }
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(Event::Char('x'), &mut ctx);
        assert!(ctx.redraw_requested());
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(key(VK_OEM_PLUS, true, false), &mut ctx);
        assert!(ctx.redraw_requested(), "zoom changes the cell size");
    }

    #[test]
    fn mouse_selects_characters_words_and_lines() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, b"alpha beta\ngamma", false);
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.size = ctx.size();
        let layout = editor.layout();
        let cell = editor.renderer.cell();
        let at = |col: i32, row: i32| {
            (
                layout.text.x + col * cell.width as i32,
                layout.text.y + row * cell.height as i32 + 2,
            )
        };
        let down = |(x, y): (i32, i32), clicks, shift| Event::MouseDown {
            x,
            y,
            clicks,
            shift,
            ctrl: false,
            alt: false,
            middle: false,
        };
        editor.event(down(at(7, 0), 2, false), &mut ctx);
        assert_eq!(editor.doc.selections.primary(), Selection::new(6, 10));
        let (x, y) = at(2, 1);
        editor.event(Event::MouseMove { x, y }, &mut ctx);
        assert_eq!(editor.doc.selections.primary(), Selection::new(6, 16));
        editor.event(Event::MouseUp, &mut ctx);
        editor.event(down(at(1, 0), 3, false), &mut ctx);
        assert_eq!(editor.doc.selections.primary(), Selection::new(0, 11));
        editor.event(Event::MouseUp, &mut ctx);
        editor.event(down(at(3, 0), 1, false), &mut ctx);
        editor.event(down(at(1, 1), 1, true), &mut ctx);
        assert_eq!(editor.doc.selections.primary(), Selection::new(3, 12));
        assert!(editor.is_text(at(1, 1).0, at(1, 1).1));
    }

    fn click(x: i32, y: i32) -> Event {
        Event::MouseDown {
            x,
            y,
            clicks: 1,
            shift: false,
            ctrl: false,
            alt: false,
            middle: false,
        }
    }

    fn palette(editor: &Editor) -> &Palette {
        match &editor.popup {
            Some(Popup::Palette(p)) => p,
            _ => panic!("the palette is not open"),
        }
    }

    fn panel(editor: &Editor) -> &Panel {
        match &editor.popup {
            Some(Popup::Panel(panel)) => panel,
            _ => panic!("no panel is open"),
        }
    }

    fn palette_labels(editor: &Editor) -> Vec<&str> {
        let p = palette(editor);
        p.items
            .iter()
            .map(|item| match item {
                Item::Header(h) => h,
                Item::Command { index, .. } => COMMANDS[*index].label,
                Item::Choice { index, .. } => p.choices.labels[*index].as_str(),
            })
            .collect()
    }

    #[test]
    fn palette_runs_commands_and_ctrl_p_lists_recent_and_favorites() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, b"one two", false);
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(key(VK_P, true, true), &mut ctx);
        assert!(ctx.redraw_requested());
        type_text(&mut editor, &mut ctx, "selection select all");
        assert_eq!(palette_labels(&editor)[0], "Selection: Select All");
        editor.event(key(VK_RETURN, false, false), &mut ctx);
        assert!(editor.popup.is_none());
        assert_eq!(editor.doc.selections.primary(), Selection::new(0, 7));
        assert_eq!(text(&editor), "one two", "typing went to the palette");

        // Ctrl+Space stars the selected command; its space character is dropped.
        editor.event(key(VK_F1, false, false), &mut ctx);
        type_text(&mut editor, &mut ctx, "undo");
        editor.event(key(VK_SPACE, true, false), &mut ctx);
        editor.event(Event::Char(' '), &mut ctx);
        assert_eq!(palette(&editor).query.as_str(), "undo");
        editor.event(key(VK_BACK, false, false), &mut ctx);
        assert_eq!(palette(&editor).query.as_str(), "und");
        editor.event(key(VK_ESCAPE, false, false), &mut ctx);
        assert!(editor.popup.is_none());

        editor.event(key(VK_P, true, false), &mut ctx);
        assert_eq!(
            palette_labels(&editor),
            vec![
                "recently used",
                "Selection: Select All",
                "favorites",
                "Edit: Undo"
            ]
        );
        // Unstarring in the favorites view removes the row and keeps a selection.
        editor.event(key(VK_DOWN, false, false), &mut ctx);
        editor.event(key(VK_SPACE, true, false), &mut ctx);
        assert_eq!(
            palette_labels(&editor),
            vec!["recently used", "Selection: Select All"]
        );
        assert_eq!(palette(&editor).selected, Some(1));
    }

    #[test]
    fn palette_mouse_stars_runs_and_closes() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, b"abc", true);
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(key(VK_P, true, true), &mut ctx);
        type_text(&mut editor, &mut ctx, "cut");
        let p = palette(&editor);
        let cut = p.selected_command().unwrap();
        assert_eq!(cut.id, "editor.action.clipboardCutAction");
        assert!(
            !enabled(cut, editor.doc.locks, true),
            "read-only disables mutating commands"
        );
        let g = editor.palette_geometry(p);
        let star = g.star_area(0, editor.ui_cell());
        assert!(!editor.is_text(star.x + 2, star.y + 2));
        editor.event(click(star.x + 2, star.y + 2), &mut ctx);
        let cut_index = commands::index_of(cut.id).unwrap();
        assert!(editor.store.as_ref().unwrap().is_favorite(cut_index));
        assert!(editor.popup.is_some(), "the star keeps the palette open");
        editor.event(click(g.list.x + 200, g.list.y + 2), &mut ctx);
        assert!(editor.popup.is_none());
        assert_eq!(editor.notice.as_deref(), Some("Not editable: read-only"));
        assert_eq!(text(&editor), "abc");

        editor.event(key(VK_P, true, false), &mut ctx);
        assert_eq!(palette_labels(&editor), vec!["recently used", "Edit: Cut"]);
        editor.event(click(1, 590), &mut ctx);
        assert!(editor.popup.is_none(), "a click outside closes the palette");

        // Toggle Read-Only from the palette makes the document editable.
        editor.event(key(VK_P, true, true), &mut ctx);
        type_text(&mut editor, &mut ctx, "toggle read");
        editor.event(key(VK_RETURN, false, false), &mut ctx);
        type_text(&mut editor, &mut ctx, "x");
        assert_eq!(text(&editor), "xabc");
    }

    /// An editor on `text` loaded from `name` (detection uses the name).
    fn editor_named(name: &str, text: &[u8]) -> Editor {
        let (tx, rx) = mpsc::channel();
        tx.send(done(text)).unwrap();
        drop(tx);
        Editor::new(
            Some(Path::new(name)),
            None,
            Some(Load::new(rx)),
            renderer(),
            96,
            settings(false, false),
            Trace::new(false),
            Arc::new(FakeFiles::default()),
            Arc::new(|| {}),
        )
    }

    /// The loader hook starts a syntax thread only for a code file within
    /// `syntax.maxFileMiB`.
    #[test]
    fn the_loader_hook_starts_syntax_for_code_within_the_limit() {
        let snapshot = Snapshot::from_bytes(b"fn main() {}\n");
        let run = |path: &str, max_mib: u64| {
            let slot: EarlySyntax = Default::default();
            let hook = crate::syntax::early_start(
                slot.clone(),
                PathBuf::from(path),
                None,
                max_mib,
                true,
                Arc::new(|| {}),
            );
            hook(&snapshot);
            slot.lock().unwrap().syntax.as_ref().map(|(l, _)| l.id)
        };
        assert_eq!(run(r"C:\a.rs", 32), Some("rust"));
        assert_eq!(run(r"C:\a.txt", 32), None);
        assert_eq!(run(r"C:\a.rs", 0), None);
    }

    /// The editor takes over a syntax thread started by the loader for the
    /// same language and text, and starts its own otherwise.
    #[test]
    fn an_early_syntax_thread_is_adopted_when_it_matches() {
        let snapshot = Snapshot::from_bytes(b"fn main() {}\n");
        for (lang, adopted) in [("rust", true), ("python", false)] {
            let lang = rte_syntax::by_id(lang).unwrap();
            let early = Syntax::start(lang, snapshot.clone(), true, Arc::new(|| {}));
            let id = early.id();
            let slot: EarlySyntax = Arc::new(std::sync::Mutex::new(crate::syntax::Early {
                syntax: Some((lang, early)),
                closed: false,
            }));
            let (tx, rx) = mpsc::channel();
            tx.send(LoadEvent::Done {
                snapshot: snapshot.clone(),
                info: info(),
                baseline: None,
                read_ms: 0.0,
            })
            .unwrap();
            drop(tx);
            let mut settings = settings(false, false);
            settings.early_syntax = Some(slot.clone());
            let editor = Editor::new(
                Some(Path::new(r"C:\src\main.rs")),
                None,
                Some(Load::new(rx)),
                renderer(),
                96,
                settings,
                Trace::new(false),
                Arc::new(FakeFiles::default()),
                Arc::new(|| {}),
            );
            assert!(slot.lock().unwrap().syntax.is_none(), "taken once");
            let syntax = editor.syntax.as_ref().unwrap();
            assert!(syntax.is_for(&editor.doc.snapshot));
            assert_eq!(syntax.id() == id, adopted, "{}", lang.id);
        }
    }

    /// Turning highlighting off or choosing Plain Text while loading stops
    /// the loader's syntax thread, whether it started before or after.
    #[test]
    fn an_early_syntax_thread_does_not_outlive_highlighting_off() {
        let text = b"fn main() {}\n";
        for (plain, hook_first) in [(false, false), (false, true), (true, false), (true, true)] {
            let slot: EarlySyntax = Default::default();
            let mut hook = Some(crate::syntax::early_start(
                slot.clone(),
                PathBuf::from(r"C:\src\main.rs"),
                None,
                32,
                true,
                Arc::new(|| {}),
            ));
            let (tx, rx) = mpsc::channel();
            tx.send(LoadEvent::Progress {
                snapshot: Snapshot::from_bytes(b"fn"),
                loaded: 2,
                total: text.len() as u64,
            })
            .unwrap();
            let first = rx.recv().ok();
            let mut settings = settings(false, false);
            settings.early_syntax = Some(slot.clone());
            let mut editor = Editor::new(
                Some(Path::new(r"C:\src\main.rs")),
                first,
                Some(Load::new(rx)),
                renderer(),
                96,
                settings,
                Trace::new(false),
                Arc::new(FakeFiles::default()),
                Arc::new(|| {}),
            );
            let mut ctx = Ctx::headless(800, 600, 96);
            let case = format!("plain {plain}, hook first {hook_first}");
            if hook_first {
                hook.take().unwrap()(&Snapshot::from_bytes(text));
                assert!(slot.lock().unwrap().syntax.is_some(), "{case}");
            }
            if plain {
                editor.set_language(0);
            } else {
                editor.perform(Action::ToggleSyntax, &mut ctx);
            }
            if let Some(hook) = hook {
                hook(&Snapshot::from_bytes(text));
            }
            assert!(slot.lock().unwrap().syntax.is_none(), "{case}");
            tx.send(done(text)).unwrap();
            editor.event(Event::Wake, &mut ctx);
            assert!(matches!(editor.status, LoadStatus::Ready), "{case}");
            assert!(editor.syntax.is_none(), "{case}");
        }
    }

    /// Delivers wake-ups until the syntax thread posted trees for the text.
    fn wait_for_syntax(editor: &mut Editor, ctx: &mut Ctx) {
        for _ in 0..5000 {
            editor.event(Event::Wake, ctx);
            if editor.syntax.as_ref().is_some_and(Syntax::settled) {
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("no syntax trees");
    }

    /// Color of the first drawn text glyph `ch`.
    fn glyph_color(editor: &mut Editor, ch: char) -> u32 {
        let mut pixels = vec![0u32; 800 * 600];
        editor.paint(&mut pixels, 800, 600);
        let clip = editor.layout().text;
        editor
            .list
            .glyphs
            .iter()
            .find(|g| g.clip == clip && g.ch == ch)
            .map(|g| g.color)
            .unwrap()
    }

    #[test]
    fn code_is_highlighted_by_the_syntax_thread() {
        let mut ctx = Ctx::headless(800, 600, 96);
        let mut editor = editor_named(r"C:\src\main.rs", b"fn main() {}\n");
        editor.event(
            Event::Resized {
                width: 800,
                height: 600,
            },
            &mut ctx,
        );
        assert_eq!(editor.language.map(|l| l.id), Some("rust"));
        assert!(editor.status_text().1.contains("Rust"));
        wait_for_syntax(&mut editor, &mut ctx);
        let keyword = editor.theme.syntax[rte_syntax::Style::Keyword as usize];
        assert_eq!(glyph_color(&mut editor, 'f'), keyword);
        // Edits move the highlights with the text, then undo restores them.
        editor.event(key(VK_HOME, true, false), &mut ctx);
        type_text(&mut editor, &mut ctx, "// c");
        editor.event(key(VK_RETURN, false, false), &mut ctx);
        let comment = editor.theme.syntax[rte_syntax::Style::Comment as usize];
        wait_for_syntax(&mut editor, &mut ctx);
        assert_eq!(glyph_color(&mut editor, 'c'), comment);
        assert_eq!(glyph_color(&mut editor, 'f'), keyword);
        while text(&editor) != "fn main() {}\n" {
            editor.event(key(VK_Z, true, false), &mut ctx);
        }
        wait_for_syntax(&mut editor, &mut ctx);
        assert_eq!(glyph_color(&mut editor, 'f'), keyword);
        // Turning highlighting off stops the thread and draws plain text.
        editor.perform(Action::ToggleSyntax, &mut ctx);
        assert!(editor.syntax.is_none() && editor.doc.journal.is_none());
        assert_eq!(glyph_color(&mut editor, 'f'), editor.theme.text);
        editor.perform(Action::ToggleSyntax, &mut ctx);
        wait_for_syntax(&mut editor, &mut ctx);
        assert_eq!(glyph_color(&mut editor, 'f'), keyword);
    }

    #[test]
    fn windows_batch_files_highlight_and_can_be_selected_manually() {
        for name in [r"C:\scripts\build.bat", r"C:\scripts\setup.cmd"] {
            let mut ctx = Ctx::headless(800, 600, 96);
            let mut editor = editor_named(name, b"@echo off\r\nREM build\r\n");
            editor.event(
                Event::Resized {
                    width: 800,
                    height: 600,
                },
                &mut ctx,
            );
            assert_eq!(editor.language.map(|l| l.id), Some("batch"));
            assert!(editor.status_text().1.contains("Batch"));
            wait_for_syntax(&mut editor, &mut ctx);
            let keyword = editor.theme.syntax[rte_syntax::Style::Keyword as usize];
            assert_eq!(glyph_color(&mut editor, '@'), keyword);
            editor.event(key(VK_OEM_2, true, false), &mut ctx);
            assert!(text(&editor).starts_with("REM @echo off\r\n"));
            wait_for_syntax(&mut editor, &mut ctx);
            let comment = editor.theme.syntax[rte_syntax::Style::Comment as usize];
            assert_eq!(glyph_color(&mut editor, 'R'), comment);
            editor.event(key(VK_Z, true, false), &mut ctx);
            wait_for_syntax(&mut editor, &mut ctx);
            assert_eq!(glyph_color(&mut editor, '@'), keyword);
        }
        let mut ctx = Ctx::headless(800, 600, 96);
        let mut editor = editor_named(r"C:\scripts\notes.txt", b"@echo off\r\n");
        chord_k(&mut editor, &mut ctx, VK_M);
        assert!(
            palette(&editor)
                .choices
                .labels
                .iter()
                .any(|label| label == "Batch")
        );
        type_text(&mut editor, &mut ctx, "batch");
        editor.event(key(VK_RETURN, false, false), &mut ctx);
        assert_eq!(editor.language.map(|l| l.id), Some("batch"));
        assert!(editor.syntax.is_some());
    }

    #[test]
    fn plain_text_runs_no_syntax_work() {
        let files = Arc::new(FakeFiles::default());
        let editor = editor_on(&files, b"fn main() {}\n", false);
        assert!(editor.language.is_none());
        assert!(editor.syntax.is_none() && editor.doc.journal.is_none());
        let mut ctx = Ctx::headless(800, 600, 96);
        let mut editor = editor_named(r"C:\big.json", &vec![b' '; 33 * 1024 * 1024]);
        editor.event(Event::Wake, &mut ctx);
        assert!(editor.syntax.is_none());
        assert!(editor.status_text().0.contains("larger than 32 MiB"));
    }

    fn chord_k(editor: &mut Editor, ctx: &mut Ctx, second: u16) {
        editor.event(key(VK_K, true, false), ctx);
        editor.event(key(second, false, false), ctx);
    }

    fn alt_shift(vk: u16) -> Event {
        Event::Key(Key {
            vk,
            ctrl: false,
            shift: true,
            alt: true,
        })
    }

    fn caret(editor: &Editor) -> u64 {
        editor.doc.selections.primary().head
    }

    #[test]
    fn change_language_mode_and_code_commands() {
        let mut ctx = Ctx::headless(800, 600, 96);
        let mut editor = editor_named(r"C:\notes.txt", b"");
        assert!(editor.language.is_none());
        chord_k(&mut editor, &mut ctx, VK_M);
        assert_eq!(palette(&editor).mode, Mode::Language);
        assert_eq!(
            palette(&editor)
                .selected
                .and_then(|i| palette(&editor).choice_at(i)),
            Some(0),
            "Plain Text is current"
        );
        type_text(&mut editor, &mut ctx, "rust");
        editor.event(key(VK_RETURN, false, false), &mut ctx);
        assert!(editor.popup.is_none());
        assert_eq!(editor.language.map(|l| l.id), Some("rust"));
        assert!(editor.syntax.is_some());

        // Auto-closing pairs, Enter between braces, Backspace in a pair.
        type_text(&mut editor, &mut ctx, "fn f(");
        assert_eq!(text(&editor), "fn f()");
        type_text(&mut editor, &mut ctx, ") {");
        assert_eq!(text(&editor), "fn f() {}");
        editor.event(key(VK_RETURN, false, false), &mut ctx);
        assert_eq!(text(&editor), "fn f() {\n    \n}");
        type_text(&mut editor, &mut ctx, "g(");
        editor.event(key(VK_BACK, false, false), &mut ctx);
        assert_eq!(text(&editor), "fn f() {\n    g\n}");
        type_text(&mut editor, &mut ctx, "();");

        // Toggle Line Comment and back.
        editor.event(key(VK_OEM_2, true, false), &mut ctx);
        assert_eq!(text(&editor), "fn f() {\n    // g();\n}");
        editor.event(key(VK_OEM_2, true, false), &mut ctx);
        assert_eq!(text(&editor), "fn f() {\n    g();\n}");

        // Go to Bracket from the opening brace and back.
        wait_for_syntax(&mut editor, &mut ctx);
        editor.select_single(Selection::caret(7));
        editor.event(key(VK_OEM_5, true, true), &mut ctx);
        assert_eq!(caret(&editor), text(&editor).rfind('}').unwrap() as u64);
        editor.event(key(VK_OEM_5, true, true), &mut ctx);
        assert_eq!(caret(&editor), 7);

        // Expand Selection grows through the syntax tree; Shrink returns.
        let g = text(&editor).find('g').unwrap() as u64;
        editor.select_single(Selection::caret(g));
        editor.event(alt_shift(VK_RIGHT), &mut ctx);
        assert_eq!(editor.doc.selections.primary().range(), g..g + 1);
        editor.event(alt_shift(VK_RIGHT), &mut ctx);
        editor.event(alt_shift(VK_RIGHT), &mut ctx);
        assert!(editor.doc.selections.primary().range().end - g >= 3);
        editor.event(alt_shift(VK_LEFT), &mut ctx);
        editor.event(alt_shift(VK_LEFT), &mut ctx);
        assert_eq!(editor.doc.selections.primary().range(), g..g + 1);

        // Go to Symbol lists `f` (from the syntax thread) and moves the
        // caret to it.
        editor.select_single(Selection::caret(text(&editor).len() as u64));
        editor.event(key(VK_O, true, true), &mut ctx);
        assert_eq!(palette(&editor).mode, Mode::Symbol);
        for _ in 0..5000 {
            if !editor.symbols_pending {
                break;
            }
            editor.event(Event::Wake, &mut ctx);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(palette(&editor).choices.labels, ["f"]);
        editor.event(key(VK_RETURN, false, false), &mut ctx);
        assert_eq!(caret(&editor), 3);
        // Unchanged text: the list shows at once.
        editor.event(key(VK_O, true, true), &mut ctx);
        assert!(!editor.symbols_pending);
        assert_eq!(palette(&editor).choices.labels, ["f"]);
        editor.event(key(VK_ESCAPE, false, false), &mut ctx);

        // The bracket pair at the caret is drawn.
        editor.select_single(Selection::caret(7));
        assert_eq!(editor.bracket_pair().map(|(a, _)| a), Some(7));
    }

    #[test]
    fn code_commands_follow_the_embedded_language() {
        let mut ctx = Ctx::headless(800, 600, 96);
        let html = "<p>a</p>\n<script>\nlet x = 1;\n</script>\n";
        let mut editor = editor_named(r"C:\page.html", html.as_bytes());
        editor.event(
            Event::Resized {
                width: 800,
                height: 600,
            },
            &mut ctx,
        );
        let mut pixels = vec![0u32; 800 * 600];
        for _ in 0..5000 {
            editor.event(Event::Wake, &mut ctx);
            editor.paint(&mut pixels, 800, 600);
            if editor.syntax.as_ref().is_some_and(Syntax::highlighted) {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let js = html.find("let").unwrap() as u64;
        assert_eq!(editor.lang_at(js).map(|l| l.id), Some("javascript"));
        // Line comments: `//` inside the script, `<!-- -->` outside.
        editor.select_single(Selection::caret(js));
        editor.event(key(VK_OEM_2, true, false), &mut ctx);
        assert!(text(&editor).contains("// let x = 1;"), "{}", text(&editor));
        editor.select_single(Selection::caret(0));
        editor.event(key(VK_OEM_2, true, false), &mut ctx);
        assert!(
            text(&editor).starts_with("<!-- <p>a</p> -->"),
            "{}",
            text(&editor)
        );
    }

    /// A fence below the view (no parsed layer yet) still comments in its
    /// own language, also in a selection that crosses into it.
    #[test]
    fn offscreen_fences_comment_in_their_language() {
        let mut ctx = Ctx::headless(800, 600, 96);
        let md = format!(
            "# Notes\n\n{}Text.\n```python\ndef hidden(): pass\n```\n",
            "Paragraph.\n\n".repeat(400)
        );
        let mut editor = editor_named(r"C:\notes.md", md.as_bytes());
        editor.event(
            Event::Resized {
                width: 800,
                height: 600,
            },
            &mut ctx,
        );
        wait_for_syntax(&mut editor, &mut ctx);
        let hidden = md.find("def hidden").unwrap() as u64;
        editor.select_single(Selection::caret(hidden));
        editor.event(key(VK_OEM_2, true, false), &mut ctx);
        assert!(
            text(&editor).contains("# def hidden(): pass"),
            "{}",
            text(&editor)
        );
        editor.event(key(VK_Z, true, false), &mut ctx);
        let text_line = md.find("Text.").unwrap() as u64;
        editor.select_single(Selection::new(text_line, hidden + 3));
        editor.event(key(VK_OEM_2, true, false), &mut ctx);
        let after = text(&editor);
        assert!(after.contains("<!-- Text. -->"), "{after}");
        assert!(after.contains("# def hidden(): pass"), "{after}");
    }

    /// After an edit far from the view the trees' positions there are
    /// approximate: comments wait for the next parse instead of guessing.
    #[test]
    fn far_edits_defer_comments_until_the_next_parse() {
        let mut ctx = Ctx::headless(800, 600, 96);
        let md = format!(
            "```js\nlet a = 1;\n```\n{}```js\nlet z = 1;\n```\n",
            "Paragraph.\n\n".repeat(2000)
        );
        let mut editor = editor_named(r"C:\notes.md", md.as_bytes());
        editor.event(
            Event::Resized {
                width: 800,
                height: 600,
            },
            &mut ctx,
        );
        wait_for_syntax(&mut editor, &mut ctx);
        let (a, z) = (
            md.find("let a").unwrap() as u64,
            md.find("let z").unwrap() as u64,
        );
        editor.doc.select(Selections::new(
            vec![Selection::caret(a), Selection::caret(z)],
            0,
        ));
        type_text(&mut editor, &mut ctx, &" ".repeat(100));
        let typed = text(&editor);
        let z = typed.find("let z").unwrap() as u64;
        editor.select_single(Selection::caret(z));
        editor.event(key(VK_OEM_2, true, false), &mut ctx);
        assert_eq!(text(&editor), typed, "nothing guessed");
        assert!(
            editor
                .notice
                .as_deref()
                .unwrap()
                .contains("still being parsed")
        );
        wait_for_syntax(&mut editor, &mut ctx);
        editor.event(key(VK_OEM_2, true, false), &mut ctx);
        let after = text(&editor);
        assert!(
            after.contains("// let z = 1;"),
            "{}",
            &after[after.len() - 40..]
        );
    }

    /// Replaced and final trees go back to the syntax thread to be freed;
    /// none are kept. (Lua: no other test here holds its highlighter.)
    #[test]
    fn replaced_trees_are_released_by_the_syntax_thread() {
        let hl = rte_syntax::highlighter(rte_syntax::by_id("lua").unwrap().grammar).unwrap();
        let base = Arc::strong_count(&hl);
        let held = || Arc::strong_count(&hl) - base;
        let until = |done: &dyn Fn() -> bool| {
            for _ in 0..5000 {
                if done() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            panic!("still held: {}", held());
        };
        let mut ctx = Ctx::headless(800, 600, 96);
        let mut editor = editor_named(r"C:\src\a.lua", b"local a = 1\n");
        editor.event(
            Event::Resized {
                width: 800,
                height: 600,
            },
            &mut ctx,
        );
        wait_for_syntax(&mut editor, &mut ctx);
        type_text(&mut editor, &mut ctx, "x");
        wait_for_syntax(&mut editor, &mut ctx);
        until(&|| held() == 1);
        drop(editor);
        until(&|| held() == 0);
    }

    /// Rows over the UI budget are highlighted by the thread again once the
    /// full parse replaces the window of a large file.
    #[test]
    fn long_rows_are_fully_highlighted_after_the_window() {
        let mut text = b"[".to_vec();
        for n in 0..20_000 {
            text.extend_from_slice(
                format!("{{\"id\":{n},\"ok\":true,\"tags\":[\"a\",null]}},").as_bytes(),
            );
        }
        text.extend_from_slice(b"0]");
        assert!(text.len() as u64 > rte_syntax::WINDOW_THRESHOLD);
        let mut ctx = Ctx::headless(1100, 900, 96);
        let mut editor = editor_named(r"C:\line.json", &text);
        editor.event(
            Event::Resized {
                width: 1100,
                height: 900,
            },
            &mut ctx,
        );
        let mut pixels = vec![0u32; 1100 * 900];
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            editor.event(Event::Wake, &mut ctx);
            editor.paint(&mut pixels, 1100, 900);
            if editor
                .syntax
                .as_ref()
                .is_some_and(Syntax::fully_highlighted)
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("never fully highlighted");
    }

    #[test]
    fn the_syntax_size_limit_follows_edits() {
        let mut ctx = Ctx::headless(800, 600, 96);
        let mut editor = editor_named(r"C:\src\main.rs", b"fn f() {}\n");
        editor.settings.syntax_max_mib = 1;
        editor.restart_syntax();
        assert!(editor.syntax.is_some());
        let grow = Command {
            tx: rte_text::Transaction::single(rte_text::Edit::insert(
                editor.doc.snapshot.len(),
                vec![b' '; 2 << 20],
            )),
            kind: edit::EditKind::Other,
            collapse: false,
        };
        editor.run(Some(grow), None);
        editor.sync_syntax();
        assert!(editor.syntax.is_none() && editor.doc.journal.is_none());
        assert!(editor.status_text().0.contains("larger than 1 MiB"));
        // Shrinking below the limit turns highlighting back on.
        editor.event(key(VK_Z, true, false), &mut ctx);
        assert!(editor.syntax.is_some() && editor.doc.journal.is_some());
    }

    #[test]
    fn highlighting_reads_only_the_drawn_part_of_long_lines() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, &vec![b'x'; 16 << 20], false);
        editor.size = (1100, 2160);
        let (visible, margin) = editor.visible_bytes();
        assert!(visible.end - visible.start <= ROW_SCAN_LIMIT as u64);
        let rows = u64::from(editor.layout().rows);
        assert!(margin.end - margin.start <= (2 * rows + 1) * ROW_SCAN_LIMIT as u64);
        assert_eq!(editor.visible_ranges(), [0..ROW_SCAN_LIMIT as u64]);
    }

    #[test]
    fn long_lines_are_highlighted_and_searched_past_the_scan_end() {
        let mut text = b"[".to_vec();
        while text.len() < 100_000 {
            text.extend_from_slice(b"\"ab\",1,");
        }
        text.extend_from_slice(b"\"end\"]");
        let len = text.len() as u64;
        let mut ctx = Ctx::headless(800, 600, 96);
        let mut editor = editor_named(r"C:\line.json", &text);
        editor.event(
            Event::Resized {
                width: 800,
                height: 600,
            },
            &mut ctx,
        );
        editor.event(key(VK_END, false, false), &mut ctx);
        let ranges = editor.visible_ranges();
        let last = ranges.last().cloned().unwrap();
        assert!(
            last.start > ROW_SCAN_LIMIT as u64 && last.end == len,
            "{ranges:?}"
        );
        assert!(editor.viewport_rows(&(0..len)).iter().any(|r| r.end == len));
        assert!(editor.status_text().1.contains("Col ~"));
        let mut pixels = vec![0u32; 800 * 600];
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !editor.syntax.as_ref().is_some_and(Syntax::highlighted) {
            assert!(std::time::Instant::now() < deadline, "never highlighted");
            editor.event(Event::Wake, &mut ctx);
            editor.paint(&mut pixels, 800, 600);
            std::thread::sleep(Duration::from_millis(1));
        }
        let snapshot = editor.doc.snapshot.clone();
        let spans = editor.syntax.as_mut().unwrap().spans(&snapshot, &ranges);
        assert!(
            spans.iter().any(|s| s.start > len - 100),
            "strings at the end are colored"
        );
        // Back at the line start the scan stops at the visible columns.
        editor.event(key(VK_HOME, false, false), &mut ctx);
        assert!(!editor.status_text().1.contains('~'));
        assert_eq!(editor.visible_ranges(), [0..ROW_SCAN_LIMIT as u64]);
    }

    #[test]
    fn alt_f4_quits_and_syntax_highlighting_toggles() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, b"", false);
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.perform(Action::ToggleSyntax, &mut ctx);
        assert!(editor.status_text().1.contains("Syntax off"));
        editor.perform(Action::ToggleSyntax, &mut ctx);
        assert!(!editor.status_text().1.contains("Syntax"));
        let alt = |vk| {
            Event::Key(Key {
                vk,
                ctrl: false,
                shift: false,
                alt: true,
            })
        };
        assert!(
            !editor.event(alt(VK_SPACE), &mut ctx),
            "the window menu stays"
        );
        assert!(!ctx.close_requested());
        editor.event(key(VK_F1, false, false), &mut ctx);
        assert!(editor.event(alt(VK_F4), &mut ctx));
        assert!(ctx.close_requested(), "Quit works with the palette open");
    }

    #[test]
    fn palette_query_supports_editing_keys_and_paste() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, b"text", false);
        let mut ctx = Ctx::headless(800, 600, 96);
        ctx.set_clipboard("redo\nsecond line", clip::FORMAT, &[]);
        editor.event(key(VK_P, true, true), &mut ctx);
        type_text(&mut editor, &mut ctx, "save");
        editor.event(key(VK_A, true, false), &mut ctx);
        type_text(&mut editor, &mut ctx, "undo");
        assert_eq!(palette(&editor).query.as_str(), "undo");
        editor.event(key(VK_A, true, false), &mut ctx);
        editor.event(key(VK_V, true, false), &mut ctx);
        assert_eq!(palette(&editor).query.as_str(), "redo");
        assert_eq!(palette_labels(&editor)[0], "Edit: Redo");
        editor.event(key(VK_HOME, false, false), &mut ctx);
        editor.event(key(VK_DELETE, false, false), &mut ctx);
        editor.event(key(VK_RIGHT, false, true), &mut ctx);
        type_text(&mut editor, &mut ctx, "X");
        assert_eq!(palette(&editor).query.as_str(), "Xdo");
        editor.event(key(VK_END, false, false), &mut ctx);
        editor.event(key(VK_LEFT, true, false), &mut ctx);
        editor.event(key(VK_BACK, true, false), &mut ctx);
        assert_eq!(
            palette(&editor).query.as_str(),
            "Xdo",
            "nothing before the word"
        );
        editor.event(key(VK_ESCAPE, false, false), &mut ctx);
        assert!(editor.popup.is_none());
        assert_eq!(text(&editor), "text", "the document is unchanged");
        assert_eq!(editor.doc.selections.primary(), Selection::caret(0));
    }

    #[test]
    fn popups_fit_small_zoomed_and_high_dpi_windows() {
        let files = Arc::new(FakeFiles::default());
        for (width, height, dpi) in [(400, 240, 96), (800, 480, 192), (600, 360, 144)] {
            let mut editor = editor_on(&files, b"text", false);
            editor.dpi = dpi;
            editor.set_zoom(16);
            let mut ctx = Ctx::headless(width, height, dpi);
            editor.event(key(VK_P, true, true), &mut ctx);
            assert!(
                editor.ui_cell().height < editor.renderer.cell().height,
                "popups do not zoom"
            );
            let mut pixels = vec![0u32; (width * height) as usize];
            for _ in 0..COMMANDS.len() {
                let p = palette(&editor);
                let g = editor.palette_geometry(p);
                assert!(g.panel.y >= 0 && g.panel.y + g.panel.h as i32 <= height as i32);
                assert!(g.input.y + g.input.h as i32 <= g.list.y);
                let row = p.selected.unwrap() - p.first;
                assert!(row < g.rows, "{width}x{height}: the selection is visible");
                editor.event(key(VK_DOWN, false, false), &mut ctx);
            }
            assert_eq!(
                palette(&editor).selected,
                Some(0),
                "every command was reached"
            );
            editor.paint(&mut pixels, width, height);
            assert!(!editor.list.popup.glyphs.is_empty());
            editor.event(key(VK_ESCAPE, false, false), &mut ctx);
            editor.perform(Action::About, &mut ctx);
            let g = editor.panel_geometry(panel(&editor));
            assert!(g.panel.y >= 0 && g.panel.y + g.panel.h as i32 <= height as i32);
            assert!(g.lines >= 1);
        }
    }

    #[test]
    fn unchanged_popup_events_request_no_repaint() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, b"text", false);
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(key(VK_P, true, true), &mut ctx);
        for event in [
            key(VK_LEFT, false, false),
            key(VK_HOME, false, false),
            key(VK_BACK, false, false),
            key(0x7B, false, false),
            Event::Char('\u{1}'),
            Event::Wake,
        ] {
            let mut ctx = Ctx::headless(800, 600, 96);
            editor.event(event.clone(), &mut ctx);
            assert!(!ctx.redraw_requested(), "{event:?}");
        }
        for event in [
            Event::Char('s'),
            key(VK_DOWN, false, false),
            key(VK_HOME, false, true),
        ] {
            let mut ctx = Ctx::headless(800, 600, 96);
            editor.event(event.clone(), &mut ctx);
            assert!(ctx.redraw_requested(), "{event:?}");
        }
    }

    #[test]
    fn palette_commands_run_before_their_state_is_saved() {
        let dir = std::env::temp_dir().join(format!("rte_editor_palette_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, b"one two", false);
        editor.settings.user_dir = Some(dir.clone());
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(key(VK_P, true, true), &mut ctx);
        type_text(&mut editor, &mut ctx, "selection select all");
        editor.event(key(VK_RETURN, false, false), &mut ctx);
        assert_eq!(editor.doc.selections.primary(), Selection::new(0, 7));
        assert_eq!(editor.store.as_mut().unwrap().settle(), None);
        let saved = std::fs::read_to_string(dir.join(favorites::FILE_NAME)).unwrap();
        assert!(saved.contains("editor.action.selectAll"), "{saved}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn about_links_open_and_shortcuts_list_every_command() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, b"", false);
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.size = ctx.size();
        editor.perform(Action::About, &mut ctx);
        let about = panel(&editor);
        assert_eq!(about.lines[1].spans[1].0, VERSION);
        let manifest_title = MANIFEST
            .lines()
            .next()
            .unwrap()
            .trim_start_matches('#')
            .trim();
        assert!(
            about
                .lines
                .iter()
                .any(|l| l.spans == [(manifest_title.to_owned(), Tone::Heading)]),
            "the manifest is part of About"
        );
        assert_eq!(about.links.len(), about.lines.len());
        let g = editor.panel_geometry(about);
        let line = about
            .links
            .iter()
            .position(|l| *l == Some(REPOSITORY_URL))
            .unwrap();
        editor.event(
            click(g.body.x + 5, g.body.y + (line as u32 * g.line_h) as i32 + 1),
            &mut ctx,
        );
        assert_eq!(ctx.requested_url(), Some(REPOSITORY_URL));
        let mut pixels = vec![0u32; 800 * 600];
        editor.paint(&mut pixels, 800, 600);
        let images = &editor.list.popup.images;
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].width, editor.icon_side());
        assert!(
            images[0].pixels.iter().any(|p| p >> 24 == 0xFF),
            "the icon is drawn"
        );
        editor.event(key(VK_ESCAPE, false, false), &mut ctx);
        assert!(editor.popup.is_none());

        let mut ctx = Ctx::headless(800, 600, 96);
        editor.perform(Action::Documentation, &mut ctx);
        assert_eq!(ctx.requested_url(), Some(DOCUMENTATION_URL));

        editor.perform(Action::KeyboardShortcuts, &mut ctx);
        let rows: Vec<String> = panel(&editor)
            .lines
            .iter()
            .map(|l| l.spans.iter().map(|(t, _)| t.as_str()).collect())
            .collect();
        for c in COMMANDS {
            assert!(rows.iter().any(|r| r.starts_with(c.label)), "{}", c.label);
        }
        assert!(rows.iter().any(|r| r.ends_with("Ctrl+Shift+P, F1")));
        editor.event(key(VK_END, false, false), &mut ctx);
        let shortcuts = panel(&editor);
        let visible = editor.panel_geometry(shortcuts).lines;
        assert!(shortcuts.first > 0, "the list is longer than the window");
        assert_eq!(shortcuts.first + visible, shortcuts.lines.len());
    }

    #[test]
    fn scripted_keys_open_and_close_the_palette_then_exit() {
        let files = Arc::new(FakeFiles::default());
        files.put(&path(), b"x");
        let mut settings = settings(false, false);
        settings.bench = Some(bench::parse("Ctrl+Shift+P,Escape", "2", "").unwrap());
        let mut editor = Editor::new(
            Some(&path()),
            Some(done(b"x")),
            None,
            renderer(),
            96,
            settings,
            Trace::new(false),
            files,
            Arc::new(|| {}),
        );
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(Event::Shown, &mut ctx);
        let mut posted = ctx.posted_key();
        let mut opened = 0;
        while let Some(key) = posted {
            let mut ctx = Ctx::headless(800, 600, 96);
            editor.event(Event::Key(key), &mut ctx);
            assert!(ctx.redraw_requested(), "{key:?} changes the frame");
            assert!(!ctx.close_requested());
            opened += usize::from(editor.popup.is_some());
            let mut ctx = Ctx::headless(800, 600, 96);
            editor.presented(&mut ctx);
            posted = ctx.posted_key();
            if posted.is_none() {
                assert!(ctx.close_requested(), "the script ends the process");
            }
        }
        assert_eq!(opened, 2);
    }

    #[test]
    fn scripted_text_types_at_every_selected_match() {
        let files = Arc::new(FakeFiles::default());
        let mut settings = settings(false, false);
        settings.bench =
            Some(bench::parse("'x'", "2", r"Ctrl+F,Alt+R,'a\d',Alt+Enter,Escape,Right").unwrap());
        let mut editor = Editor::new(
            Some(&path()),
            Some(done(b"a1 a2 b3 a4")),
            None,
            renderer(),
            96,
            settings,
            Trace::new(false),
            files,
            Arc::new(|| {}),
        );
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(Event::Shown, &mut ctx);
        for _ in 0..5000 {
            if ctx.close_requested() {
                break;
            }
            let (posted_key, posted_char) = (ctx.posted_key(), ctx.posted_char());
            ctx = Ctx::headless(800, 600, 96);
            if let Some(key) = posted_key {
                editor.event(Event::Key(key), &mut ctx);
            } else if let Some(ch) = posted_char {
                editor.event(Event::Char(ch), &mut ctx);
            } else {
                std::thread::sleep(Duration::from_millis(1));
                editor.event(Event::Wake, &mut ctx);
            }
            if ctx.redraw_requested() {
                editor.presented(&mut ctx);
            }
        }
        assert!(ctx.close_requested(), "the script ends the process");
        assert_eq!(text(&editor), "a1xx a2xx b3 a4xx");
    }

    fn mods(vk: u16, ctrl: bool, shift: bool, alt: bool) -> Event {
        Event::Key(Key {
            vk,
            ctrl,
            shift,
            alt,
        })
    }

    /// Delivers wake-ups until `done` holds.
    fn pump(editor: &mut Editor, ctx: &mut Ctx, done: impl Fn(&Editor) -> bool) {
        for _ in 0..5000 {
            if done(editor) {
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
            editor.event(Event::Wake, ctx);
        }
        panic!("the search thread did not answer");
    }

    fn counter(editor: &Editor) -> String {
        let f = editor.find.as_ref().unwrap();
        f.counter(editor.doc.selections.primary().range()).0
    }

    fn ranges(editor: &Editor) -> Vec<std::ops::Range<u64>> {
        editor
            .doc
            .selections
            .as_slice()
            .iter()
            .map(Selection::range)
            .collect()
    }

    #[test]
    fn search_starts_nothing_until_used() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, b"  cat", false);
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(Event::Wake, &mut ctx);
        assert!(editor.find.is_none() && editor.search.is_none());
        editor.event(key(VK_F, true, false), &mut ctx);
        assert!(editor.find_focused());
        assert!(editor.search.is_none(), "an empty query starts no thread");
        type_text(&mut editor, &mut ctx, "c");
        assert!(editor.search.is_some());
    }

    #[test]
    fn search_box_finds_counts_navigates_and_closes() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, b"Cat dog cat\ncat", false);
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(key(VK_F, true, false), &mut ctx);
        type_text(&mut editor, &mut ctx, "cat");
        pump(&mut editor, &mut ctx, |e| counter(e) == "1 of 3");
        assert_eq!(ranges(&editor), [0..3]);
        assert_eq!(text(&editor), "Cat dog cat\ncat", "typing went to the box");
        editor.event(key(VK_RETURN, false, false), &mut ctx);
        pump(&mut editor, &mut ctx, |e| counter(e) == "2 of 3");
        assert_eq!(ranges(&editor), [8..11]);
        editor.event(key(VK_RETURN, false, true), &mut ctx);
        pump(&mut editor, &mut ctx, |e| ranges(e) == [0..3]);
        editor.event(key(VK_F3, false, true), &mut ctx);
        pump(&mut editor, &mut ctx, |e| ranges(e) == [12..15]);
        assert!(
            !find::highlights(editor.find.as_deref(), &editor.doc.snapshot)
                .0
                .is_empty()
        );

        // Match Case uses the UI scan: no wait for the next match.
        editor.event(mods(VK_C, false, false, true), &mut ctx);
        editor.event(key(VK_F3, false, false), &mut ctx);
        assert_eq!(ranges(&editor), [8..11]);
        pump(&mut editor, &mut ctx, |e| counter(e) == "1 of 2");

        editor.event(key(VK_ESCAPE, false, false), &mut ctx);
        assert!(!editor.find_open());
        assert_eq!(ranges(&editor), [8..11], "the match stays selected");
        editor.event(key(VK_F3, false, false), &mut ctx);
        assert_eq!(ranges(&editor), [12..15], "F3 works with the box closed");
    }

    #[test]
    fn modes_are_exclusive_and_errors_are_reported() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, b"a1 b22 a333", false);
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(key(VK_F, true, false), &mut ctx);
        editor.event(mods(VK_R, false, false, true), &mut ctx);
        type_text(&mut editor, &mut ctx, "(");
        pump(&mut editor, &mut ctx, |e| {
            e.find.as_ref().unwrap().error.is_some()
        });
        assert_eq!(counter(&editor), "No results");
        assert!(!editor.search_busy(), "an invalid pattern ends the search");
        editor.event(key(VK_BACK, false, false), &mut ctx);
        type_text(&mut editor, &mut ctx, r"a\d+");
        pump(&mut editor, &mut ctx, |e| counter(e) == "1 of 2");
        editor.event(mods(VK_G, false, false, true), &mut ctx);
        let f = editor.find.as_ref().unwrap();
        assert_eq!(f.mode, rte_search::Mode::Glob);
        editor.event(key(VK_A, true, false), &mut ctx);
        type_text(&mut editor, &mut ctx, "b?2");
        pump(&mut editor, &mut ctx, |e| counter(e) == "1 of 1");
        assert_eq!(ranges(&editor), [3..6]);
        editor.event(mods(VK_R, false, false, true), &mut ctx);
        assert_eq!(editor.find.as_ref().unwrap().mode, rte_search::Mode::Regex);
    }

    #[test]
    fn replace_one_replace_all_and_undo() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, b"cat dog cat\ncat", false);
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(key(VK_H, true, false), &mut ctx);
        editor.event(key(VK_TAB, false, false), &mut ctx);
        type_text(&mut editor, &mut ctx, "cat");
        editor.event(key(VK_TAB, false, false), &mut ctx);
        type_text(&mut editor, &mut ctx, "cow");
        pump(&mut editor, &mut ctx, |e| ranges(e) == [0..3]);
        editor.event(key(VK_RETURN, false, false), &mut ctx);
        pump(&mut editor, &mut ctx, |e| ranges(e) == [8..11]);
        assert_eq!(text(&editor), "cow dog cat\ncat");
        editor.event(mods(VK_RETURN, true, false, true), &mut ctx);
        pump(&mut editor, &mut ctx, |e| text(e) == "cow dog cow\ncow");
        assert_eq!(editor.notice.as_deref(), Some("Replaced 2 occurrences"));
        editor.event(key(VK_ESCAPE, false, false), &mut ctx);
        editor.event(key(VK_Z, true, false), &mut ctx);
        assert_eq!(
            text(&editor),
            "cow dog cat\ncat",
            "Replace All is one undo step"
        );

        editor.doc.locks.user = true;
        editor.event(mods(VK_RETURN, true, false, true), &mut ctx);
        assert!(
            editor
                .notice
                .as_deref()
                .unwrap()
                .starts_with("Not editable")
        );
        assert!(editor.pending_op.is_none());
    }

    #[test]
    fn stale_replace_all_reruns_on_the_current_text() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, b"cat cat", false);
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(Event::Wake, &mut ctx);
        editor.event(key(VK_H, true, false), &mut ctx);
        type_text(&mut editor, &mut ctx, "dog");
        editor.event(mods(VK_RETURN, true, false, true), &mut ctx);
        // The text changes before the reply arrives.
        let tx = rte_text::Transaction::single(rte_text::Edit::insert(7, b" cat".to_vec()));
        editor.doc.snapshot = editor.doc.snapshot.apply(&tx).unwrap().0;
        pump(&mut editor, &mut ctx, |e| text(e) == "dog dog dog");
    }

    #[test]
    fn occurrences_cursor_undo_and_select_all() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, b"foo bar foo foobar foo", false);
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(Event::Wake, &mut ctx);
        editor.event(key(VK_D, true, false), &mut ctx);
        assert_eq!(
            ranges(&editor),
            [0..3],
            "an empty selection selects its word"
        );
        editor.event(key(VK_D, true, false), &mut ctx);
        assert_eq!(ranges(&editor), [0..3, 8..11], "whole words only");
        editor.event(key(VK_K, true, false), &mut ctx);
        assert!(editor.notice.as_deref().unwrap().contains("Ctrl+K"));
        editor.event(key(VK_D, true, false), &mut ctx);
        assert_eq!(ranges(&editor), [0..3, 19..22]);
        editor.event(key(VK_U, true, false), &mut ctx);
        assert_eq!(ranges(&editor), [0..3, 8..11]);
        editor.event(key(VK_L, true, true), &mut ctx);
        pump(&mut editor, &mut ctx, |e| e.doc.selections.len() == 3);
        type_text(&mut editor, &mut ctx, "x");
        assert_eq!(text(&editor), "x bar x foobar x");
        editor.event(key(VK_K, true, false), &mut ctx);
        editor.event(key(VK_X, true, false), &mut ctx);
        assert!(
            editor
                .notice
                .as_deref()
                .unwrap()
                .contains("is not a command")
        );
    }

    #[test]
    fn select_all_above_the_limit_changes_nothing() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, b"a a a", false);
        editor.settings.multi_cursor_limit = 2;
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(Event::Wake, &mut ctx);
        editor.event(key(VK_D, true, false), &mut ctx);
        editor.event(key(VK_L, true, true), &mut ctx);
        pump(&mut editor, &mut ctx, |e| e.notice.is_some());
        assert!(editor.notice.as_deref().unwrap().starts_with("More than 2"));
        assert_eq!(ranges(&editor), [0..1]);
    }

    /// Lets the search thread answer, then delivers its replies.
    fn let_search_answer(editor: &mut Editor, ctx: &mut Ctx) {
        for _ in 0..5 {
            std::thread::sleep(Duration::from_millis(20));
            editor.event(Event::Wake, ctx);
        }
    }

    /// `cat cat` with the replace box open: find `cat`, replace with `dog`.
    fn replace_box(text: &[u8]) -> (Editor, Ctx) {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, text, false);
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(key(VK_H, true, false), &mut ctx);
        editor.event(key(VK_TAB, false, false), &mut ctx);
        type_text(&mut editor, &mut ctx, "cat");
        editor.event(key(VK_TAB, false, false), &mut ctx);
        type_text(&mut editor, &mut ctx, "dog");
        pump(&mut editor, &mut ctx, |e| ranges(e) == [0..3]);
        (editor, ctx)
    }

    #[test]
    fn changed_search_input_revokes_pending_replacements_and_selections() {
        // Replace All, then the query changes before the reply.
        let (mut editor, mut ctx) = replace_box(b"cat cat");
        editor.event(mods(VK_RETURN, true, false, true), &mut ctx);
        editor.event(key(VK_TAB, false, false), &mut ctx);
        type_text(&mut editor, &mut ctx, "x");
        let_search_answer(&mut editor, &mut ctx);
        assert_eq!(text(&editor), "cat cat");
        assert!(editor.pending_op.is_none());

        // Replace One, then Match Case changes.
        let (mut editor, mut ctx) = replace_box(b"cat cat");
        editor.event(key(VK_RETURN, false, false), &mut ctx);
        editor.event(mods(VK_C, false, false, true), &mut ctx);
        let_search_answer(&mut editor, &mut ctx);
        assert_eq!(text(&editor), "cat cat");

        // Replace All, then the replacement changes.
        let (mut editor, mut ctx) = replace_box(b"cat cat");
        editor.event(mods(VK_RETURN, true, false, true), &mut ctx);
        type_text(&mut editor, &mut ctx, "s");
        let_search_answer(&mut editor, &mut ctx);
        assert_eq!(text(&editor), "cat cat");

        // Select all matches, then Find in Selection changes the scope.
        let (mut editor, mut ctx) = replace_box(b"cat cat");
        editor.event(mods(VK_RETURN, false, false, true), &mut ctx);
        editor.event(mods(VK_L, false, false, true), &mut ctx);
        let_search_answer(&mut editor, &mut ctx);
        assert_eq!(ranges(&editor), [0..3]);
        assert!(
            editor
                .notice
                .as_deref()
                .unwrap()
                .contains("nothing was selected")
        );

        // Unchanged input still applies.
        let (mut editor, mut ctx) = replace_box(b"cat cat");
        editor.event(mods(VK_RETURN, true, false, true), &mut ctx);
        pump(&mut editor, &mut ctx, |e| text(e) == "dog dog");
    }

    #[test]
    fn cursor_undo_never_restores_offsets_of_an_older_text() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, b"ab ab", false);
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(Event::Wake, &mut ctx);
        editor.event(key(VK_D, true, false), &mut ctx);
        editor.event(key(VK_D, true, false), &mut ctx);
        assert_eq!(ranges(&editor), [0..2, 3..5]);
        editor.event(Event::Char('\u{e9}'), &mut ctx);
        let after = ranges(&editor);
        editor.event(key(VK_U, true, false), &mut ctx);
        assert_eq!(ranges(&editor), after, "states of the old text are dropped");
        type_text(&mut editor, &mut ctx, "X");
        assert_eq!(text(&editor), "\u{e9}X \u{e9}X");
        // States recorded on the current text still restore.
        editor.event(key(VK_HOME, false, false), &mut ctx);
        editor.event(mods(VK_I, false, true, true), &mut ctx);
        let before = ranges(&editor);
        editor.event(key(VK_L, true, false), &mut ctx);
        editor.event(key(VK_U, true, false), &mut ctx);
        assert_eq!(ranges(&editor), before);
    }

    #[test]
    fn dense_long_line_highlights_are_bounded_and_released_on_close() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, &vec![b'a'; 1 << 20], false);
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(Event::Wake, &mut ctx);
        editor.event(key(VK_F, true, false), &mut ctx);
        editor.event(mods(VK_C, false, false, true), &mut ctx);
        let started = std::time::Instant::now();
        type_text(&mut editor, &mut ctx, "a");
        let ui = started.elapsed();
        let shown = find::highlights(editor.find.as_deref(), &editor.doc.snapshot)
            .0
            .len();
        assert_eq!(
            shown,
            rte_view::ROW_SCAN_LIMIT,
            "one row as the view lays it out"
        );
        assert!(ui < Duration::from_millis(500), "{ui:?}");
        editor.event(key(VK_ESCAPE, false, false), &mut ctx);
        assert_eq!(editor.find.as_ref().unwrap().match_capacity(), 0);
    }

    #[test]
    fn next_and_previous_step_over_an_empty_current_match() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, b"a\nb", false);
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(key(VK_F, true, false), &mut ctx);
        editor.event(mods(VK_R, false, false, true), &mut ctx);
        type_text(&mut editor, &mut ctx, "^");
        pump(&mut editor, &mut ctx, |e| counter(e) == "1 of 2");
        assert_eq!(ranges(&editor), [0..0]);
        editor.event(key(VK_F3, false, false), &mut ctx);
        pump(&mut editor, &mut ctx, |e| ranges(e) == [2..2]);
        editor.event(key(VK_RETURN, false, false), &mut ctx);
        pump(&mut editor, &mut ctx, |e| ranges(e) == [0..0]);
        editor.event(key(VK_F3, false, true), &mut ctx);
        pump(&mut editor, &mut ctx, |e| ranges(e) == [2..2]);
        editor.event(key(VK_RETURN, false, true), &mut ctx);
        pump(&mut editor, &mut ctx, |e| ranges(e) == [0..0]);
    }

    #[test]
    fn an_old_index_reply_does_not_label_a_new_selection() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, b"cat cat cat", false);
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(key(VK_F, true, false), &mut ctx);
        type_text(&mut editor, &mut ctx, "cat");
        pump(&mut editor, &mut ctx, |e| counter(e) == "1 of 3");
        editor.event(key(VK_F3, false, false), &mut ctx);
        pump(&mut editor, &mut ctx, |e| ranges(e) == [4..7]);
        editor.event(key(VK_F3, false, false), &mut ctx);
        pump(&mut editor, &mut ctx, |e| ranges(e) == [8..11]);
        let f = editor.find.as_ref().unwrap();
        let reply = crate::search::Reply {
            lane: crate::search::Lane::Find,
            generation: f.generation,
            snapshot: editor.doc.snapshot.clone(),
            outcome: crate::search::Outcome::Index {
                at: 0,
                index: Some(0),
            },
        };
        editor.find_reply(reply);
        assert_ne!(counter(&editor), "1 of 3");
        pump(&mut editor, &mut ctx, |e| counter(e) == "3 of 3");
    }

    #[test]
    fn add_next_occurrence_passes_already_selected_matches() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, b"cat cat cat", false);
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(Event::Wake, &mut ctx);
        editor.doc.select(Selections::new(
            vec![Selection::new(0, 3), Selection::new(4, 7)],
            0,
        ));
        editor.event(key(VK_D, true, false), &mut ctx);
        assert_eq!(ranges(&editor), [0..3, 4..7, 8..11]);
        editor.event(key(VK_D, true, false), &mut ctx);
        assert_eq!(ranges(&editor), [0..3, 4..7, 8..11]);
        assert_eq!(editor.notice.as_deref(), Some("No more occurrences"));
    }

    #[test]
    fn highlights_stay_inside_find_in_selection() {
        for match_case in [false, true] {
            let files = Arc::new(FakeFiles::default());
            let mut editor = editor_on(&files, b"cat cat", false);
            let mut ctx = Ctx::headless(800, 600, 96);
            editor.event(Event::Wake, &mut ctx);
            editor.event(key(VK_D, true, false), &mut ctx);
            editor.event(key(VK_F, true, false), &mut ctx);
            editor.event(mods(VK_L, false, false, true), &mut ctx);
            if match_case {
                editor.event(mods(VK_C, false, false, true), &mut ctx);
            }
            pump(&mut editor, &mut ctx, |e| !e.search_busy());
            let shown = find::highlights(editor.find.as_deref(), &editor.doc.snapshot).0;
            assert_eq!(shown, [0..3], "match case {match_case}");
        }
    }

    #[test]
    fn line_ends_and_column_drags_refuse_above_the_limit_early() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, "x\n".repeat(50_000).as_bytes(), false);
        editor.settings.multi_cursor_limit = 2;
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(Event::Wake, &mut ctx);
        editor.event(key(VK_A, true, false), &mut ctx);
        let started = std::time::Instant::now();
        editor.event(mods(VK_I, false, true, true), &mut ctx);
        assert!(started.elapsed() < Duration::from_millis(100));
        assert!(editor.notice.as_deref().unwrap().starts_with("At most 2"));
        assert_eq!(editor.doc.selections.len(), 1);
        editor.event(key(VK_HOME, true, false), &mut ctx);
        for _ in 0..3 {
            editor.event(mods(VK_DOWN, true, true, true), &mut ctx);
        }
        assert_eq!(editor.doc.selections.len(), 2, "the third line is refused");
    }

    #[test]
    fn cursors_above_below_line_ends_and_columns() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, b"abc\nabc\nabc", false);
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(Event::Wake, &mut ctx);
        editor.event(key(VK_RIGHT, false, false), &mut ctx);
        editor.event(mods(VK_DOWN, true, false, true), &mut ctx);
        editor.event(mods(VK_DOWN, true, false, true), &mut ctx);
        assert_eq!(ranges(&editor), [1..1, 5..5, 9..9]);
        type_text(&mut editor, &mut ctx, "-");
        assert_eq!(text(&editor), "a-bc\na-bc\na-bc");
        editor.event(key(VK_ESCAPE, false, false), &mut ctx);
        assert_eq!(editor.doc.selections.len(), 1);

        editor.event(key(VK_HOME, true, false), &mut ctx);
        editor.event(mods(VK_DOWN, true, true, true), &mut ctx);
        editor.event(mods(VK_RIGHT, true, true, true), &mut ctx);
        editor.event(mods(VK_RIGHT, true, true, true), &mut ctx);
        assert_eq!(ranges(&editor), [0..2, 5..7]);
        editor.event(key(VK_A, true, false), &mut ctx);
        editor.event(mods(VK_I, false, true, true), &mut ctx);
        assert_eq!(ranges(&editor), [4..4, 9..9, 14..14]);
        editor.event(key(VK_L, true, false), &mut ctx);
        assert_eq!(ranges(&editor), [0..5, 5..10, 10..14]);
    }

    #[test]
    fn alt_click_adds_and_removes_carets_and_middle_drag_selects_columns() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, b"one two\nthree four\nfive", false);
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(Event::Shown, &mut ctx);
        let layout = editor.layout();
        let cell = editor.renderer.cell();
        let at = |col: i32, row: i32| {
            (
                layout.text.x + col * cell.width as i32,
                layout.text.y + row * cell.height as i32 + 2,
            )
        };
        let down = |(x, y): (i32, i32), alt: bool, middle: bool| Event::MouseDown {
            x,
            y,
            clicks: 1,
            shift: false,
            ctrl: false,
            alt,
            middle,
        };
        editor.event(down(at(1, 0), false, false), &mut ctx);
        editor.event(Event::MouseUp, &mut ctx);
        editor.event(down(at(2, 1), true, false), &mut ctx);
        editor.event(Event::MouseUp, &mut ctx);
        assert_eq!(ranges(&editor), [1..1, 10..10]);
        assert_eq!(editor.doc.selections.primary().head, 10);
        editor.event(down(at(2, 1), true, false), &mut ctx);
        editor.event(Event::MouseUp, &mut ctx);
        assert_eq!(ranges(&editor), [1..1]);

        editor.event(down(at(1, 0), false, true), &mut ctx);
        let (x, y) = at(3, 2);
        editor.event(Event::MouseMove { x, y }, &mut ctx);
        editor.event(Event::MouseUp, &mut ctx);
        assert_eq!(ranges(&editor), [1..3, 9..11, 20..22]);
        editor.event(key(VK_U, true, false), &mut ctx);
        assert_eq!(ranges(&editor), [1..1], "a drag is one cursor-undo step");
    }

    #[test]
    fn icon_scaling_averages_premultiplied_pixels() {
        let source = [0xFF00_0000, 0, 0xFFFF_FFFF, 0];
        assert_eq!(scale_image(&source, 2, 1), vec![0x8040_4040]);
        let up = scale_image(&source, 2, 4);
        assert_eq!(up.len(), 16);
        assert_eq!(
            (up[0], up[1], up[2], up[8]),
            (source[0], source[0], 0, source[2])
        );
    }

    /// `Ctrl+K` then `Ctrl+second`.
    fn chord_k_ctrl(editor: &mut Editor, ctx: &mut Ctx, second: u16) {
        editor.event(key(VK_K, true, false), ctx);
        editor.event(key(second, true, false), ctx);
    }

    #[test]
    fn line_commands_run_from_their_keys() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, b"one\ntwo\nthree", false);
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(Event::Wake, &mut ctx);
        let alt = |vk, shift| mods(vk, false, shift, true);
        editor.event(alt(VK_DOWN, false), &mut ctx);
        assert_eq!(text(&editor), "two\none\nthree");
        editor.event(alt(VK_UP, false), &mut ctx);
        assert_eq!(text(&editor), "one\ntwo\nthree");
        editor.event(alt(VK_DOWN, true), &mut ctx);
        assert_eq!(
            (text(&editor).as_str(), caret(&editor)),
            ("one\none\ntwo\nthree", 4)
        );
        editor.event(key(VK_K, true, true), &mut ctx);
        assert_eq!(
            (text(&editor).as_str(), caret(&editor)),
            ("one\ntwo\nthree", 4)
        );
        editor.event(key(VK_RETURN, true, false), &mut ctx);
        assert_eq!(
            (text(&editor).as_str(), caret(&editor)),
            ("one\ntwo\n\nthree", 8)
        );
        editor.event(key(VK_RETURN, true, true), &mut ctx);
        assert_eq!(text(&editor), "one\ntwo\n\n\nthree");
        editor.event(key(VK_Z, true, false), &mut ctx);
        editor.event(key(VK_Z, true, false), &mut ctx);
        assert_eq!(text(&editor), "one\ntwo\nthree");
        editor.event(key(VK_OEM_6, true, false), &mut ctx);
        assert_eq!(text(&editor), "one\n    two\nthree");
        editor.event(key(VK_OEM_4, true, false), &mut ctx);
        assert_eq!(text(&editor), "one\ntwo\nthree");
        editor.event(key(VK_F, true, false), &mut ctx);
        editor.event(alt(VK_DOWN, false), &mut ctx);
        assert_eq!(text(&editor), "one\ntwo\nthree", "not from the find box");
        // Read-only refuses the edits.
        let mut locked = editor_on(&files, b"one\ntwo", true);
        locked.event(Event::Wake, &mut ctx);
        locked.event(alt(VK_DOWN, false), &mut ctx);
        assert_eq!(text(&locked), "one\ntwo");
    }

    #[test]
    fn add_and_remove_line_comments_use_the_language() {
        let mut ctx = Ctx::headless(800, 600, 96);
        let mut editor = editor_named(r"C:\src\main.rs", b"a();\n// b();");
        editor.event(Event::Wake, &mut ctx);
        editor.event(key(VK_A, true, false), &mut ctx);
        chord_k_ctrl(&mut editor, &mut ctx, VK_C);
        assert_eq!(text(&editor), "// a();\n// // b();");
        editor.event(key(VK_Z, true, false), &mut ctx);
        chord_k_ctrl(&mut editor, &mut ctx, VK_U);
        assert_eq!(text(&editor), "a();\nb();");
    }

    #[test]
    fn scroll_page_shortcuts_panel_and_the_empty_state() {
        let files = Arc::new(FakeFiles::default());
        let lines = "x\n".repeat(200);
        let mut editor = editor_on(&files, lines.as_bytes(), false);
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(Event::Wake, &mut ctx);
        editor.event(mods(VK_NEXT, false, false, true), &mut ctx);
        assert_eq!(editor.scroll.top_line, editor.page());
        editor.event(mods(VK_PRIOR, false, false, true), &mut ctx);
        assert_eq!((editor.scroll.top_line, caret(&editor)), (0, 0));
        chord_k_ctrl(&mut editor, &mut ctx, VK_S);
        assert!(matches!(editor.popup, Some(Popup::Panel(_))));

        let mut empty = empty_editor(false);
        empty.event(mods(VK_DOWN, false, false, true), &mut ctx);
        assert_eq!(empty.notice.as_deref(), Some("No file is open"));
        chord_k_ctrl(&mut empty, &mut ctx, VK_S);
        assert!(matches!(empty.popup, Some(Popup::Panel(_))));
        empty.popup = None;
        let mut ctx = Ctx::headless(800, 600, 96);
        empty.event(key(VK_F11, false, false), &mut ctx);
        assert_eq!(ctx.requested_full_screen(), Some(true));
        assert_eq!(empty.layout().scrollbar.w, 0, "no strip without a file");
    }

    #[test]
    fn go_to_line_previews_and_places_the_caret() {
        let files = Arc::new(FakeFiles::default());
        let lines: String = (1..=200).map(|i| format!("line {i}\n")).collect();
        let mut editor = editor_on(&files, lines.as_bytes(), false);
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(Event::Wake, &mut ctx);
        let s = editor.doc.snapshot.clone();
        let label = |editor: &Editor| palette(editor).choices.labels[0].clone();
        editor.event(key(VK_G, true, false), &mut ctx);
        assert_eq!(palette(&editor).mode, Mode::Line);
        assert!(label(&editor).starts_with("Current line 1, column 1"));
        type_text(&mut editor, &mut ctx, "150");
        assert_eq!(label(&editor), "Go to line 150");
        let top = editor.scroll.top_line;
        assert!(top <= 149 && 149 < top + editor.page(), "previewed");
        editor.event(key(VK_ESCAPE, false, false), &mut ctx);
        assert_eq!(
            (editor.scroll.top_line, caret(&editor)),
            (0, 0),
            "Escape restores"
        );

        editor.event(key(VK_G, true, false), &mut ctx);
        type_text(&mut editor, &mut ctx, "20:3");
        assert_eq!(label(&editor), "Go to line 20, column 3");
        editor.event(key(VK_RETURN, false, false), &mut ctx);
        assert!(editor.popup.is_none());
        assert_eq!(caret(&editor), s.line_start(19).unwrap() + 2);

        for (query, line) in [("-1", 200), ("999", 200), ("0", 0)] {
            editor.event(key(VK_G, true, false), &mut ctx);
            type_text(&mut editor, &mut ctx, query);
            editor.event(key(VK_RETURN, false, false), &mut ctx);
            assert_eq!(caret(&editor), s.line_start(line).unwrap(), "{query}");
        }
        // Not a line: Enter changes nothing.
        let before = (caret(&editor), editor.scroll);
        editor.event(key(VK_G, true, false), &mut ctx);
        type_text(&mut editor, &mut ctx, "abc");
        editor.event(key(VK_RETURN, false, false), &mut ctx);
        assert_eq!((caret(&editor), editor.scroll), before);
    }

    #[test]
    fn full_screen_follows_the_platform() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, b"text", false);
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(Event::Wake, &mut ctx);
        let title = editor.layout().title;
        assert!(title.h > 0 && editor.is_caption(title.x + 5, title.y + 2));
        editor.event(key(VK_F11, false, false), &mut ctx);
        assert_eq!(ctx.requested_full_screen(), Some(true));
        assert_eq!(
            editor.layout().title.h,
            title.h,
            "unchanged until the platform answers"
        );
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(Event::FullScreen(Ok(true)), &mut ctx);
        assert!(ctx.redraw_requested());
        assert_eq!(editor.layout().title.h, 0);
        assert!(!editor.is_caption(5, 2));
        editor.event(key(VK_F11, false, false), &mut ctx);
        assert_eq!(ctx.requested_full_screen(), Some(false));
        editor.event(Event::FullScreen(Ok(false)), &mut ctx);
        assert_eq!(editor.layout().title, title);
        editor.event(Event::FullScreen(Err("no monitor")), &mut ctx);
        assert_eq!(
            editor.notice.as_deref(),
            Some("Full screen failed: no monitor")
        );
    }

    #[test]
    fn the_scroll_slider_drags_without_moving_the_caret() {
        let files = Arc::new(FakeFiles::default());
        let lines = "x\n".repeat(999);
        let mut editor = editor_on(&files, lines.as_bytes(), false);
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(Event::Wake, &mut ctx);
        editor.select_single(Selection::caret(4));
        let track = editor.layout().scrollbar;
        assert_eq!(track.w, 6);
        assert!(!editor.is_text(track.x + 1, track.y + 5));
        let slider = editor.slider().unwrap();
        assert_eq!(slider.y, track.y);
        let press = |y| Event::MouseDown {
            x: track.x + 2,
            y,
            clicks: 1,
            shift: false,
            ctrl: false,
            alt: false,
            middle: false,
        };
        // Pressing and releasing repaint once each (the slider's color).
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(press(slider.y + 2), &mut ctx);
        assert!(ctx.redraw_requested());
        assert_eq!(editor.scroll.top_line, 0);
        let bottom = track.y + track.h as i32 + 50;
        editor.event(
            Event::MouseMove {
                x: track.x,
                y: bottom,
            },
            &mut ctx,
        );
        assert_eq!(editor.scroll.top_line, 999);
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(Event::MouseUp, &mut ctx);
        assert!(ctx.redraw_requested());
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(Event::MouseUp, &mut ctx);
        assert!(!ctx.redraw_requested(), "nothing changed");
        // A press on the strip centers the slider there.
        let middle = track.y + track.h as i32 / 2;
        editor.event(press(middle), &mut ctx);
        editor.event(Event::MouseUp, &mut ctx);
        let s = editor.slider().unwrap();
        assert!((s.y + s.h as i32 / 2 - middle).abs() <= 2, "{s:?}");
        assert_eq!(caret(&editor), 4);
    }

    #[test]
    fn closing_or_replacing_the_file_drops_long_line_and_preview_state() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, &vec![b'x'; 128 * 1024], false);
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(Event::Wake, &mut ctx);
        editor.event(key(VK_END, false, false), &mut ctx);
        editor.visible_ranges();
        assert!(
            editor.seams.borrow().0.is_some(),
            "the long row was scanned"
        );
        editor.event(key(VK_W, true, false), &mut ctx);
        assert!(editor.no_file());
        assert!(
            editor.seams.borrow().0.is_none(),
            "the closed text is released"
        );

        // A file opened during a Go to Line preview starts at its top.
        let lines: String = (1..=300).map(|i| format!("{i}\n")).collect();
        let mut editor = editor_on(&files, lines.as_bytes(), false);
        editor.event(Event::Wake, &mut ctx);
        let file = TempFile::new("short.txt", b"one\ntwo");
        editor.event(key(VK_G, true, false), &mut ctx);
        type_text(&mut editor, &mut ctx, "200");
        assert!(editor.scroll.top_line > 100);
        drop_file(&mut editor, &mut ctx, &file.0, 0);
        settle_open(&mut editor, &mut ctx);
        assert_eq!(text(&editor), "one\ntwo");
        assert_eq!(editor.scroll.top_line, 0);
        assert!(editor.line_preview.is_none() && editor.popup.is_none());
    }

    #[test]
    fn alt_line_keys_need_the_text_focus() {
        let files = Arc::new(FakeFiles::default());
        let mut editor = editor_on(&files, b"one\ntwo", false);
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(Event::Wake, &mut ctx);
        let alt = |vk, shift| mods(vk, false, shift, true);
        for open in [key(VK_P, true, true), key(VK_K, true, false)] {
            editor.event(open, &mut ctx);
            if editor.chord.is_some() {
                editor.event(key(VK_S, true, false), &mut ctx);
            }
            assert!(editor.popup.is_some());
            for event in [alt(VK_DOWN, false), alt(VK_DOWN, true), alt(VK_NEXT, false)] {
                editor.event(event, &mut ctx);
            }
            assert_eq!(
                (text(&editor).as_str(), editor.scroll.top_line),
                ("one\ntwo", 0)
            );
            editor.event(key(VK_ESCAPE, false, false), &mut ctx);
        }
        // The palette still runs the command by name.
        editor.event(key(VK_P, true, true), &mut ctx);
        type_text(&mut editor, &mut ctx, "move line down");
        editor.event(key(VK_RETURN, false, false), &mut ctx);
        assert_eq!(text(&editor), "two\none");
    }

    #[test]
    fn go_to_line_clamps_huge_columns_to_the_line_end() {
        let files = Arc::new(FakeFiles::default());
        let text = "\u{e9}\u{e9}\n\u{4e2d}x\ne\u{301}\nab";
        let mut editor = editor_on(&files, text.as_bytes(), false);
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(Event::Wake, &mut ctx);
        let s = editor.doc.snapshot.clone();
        for line in 0..4u64 {
            editor.event(key(VK_G, true, false), &mut ctx);
            type_text(&mut editor, &mut ctx, &format!("{}:{}", line + 1, u64::MAX));
            editor.event(key(VK_RETURN, false, false), &mut ctx);
            assert_eq!(
                caret(&editor),
                edit::content_range(&s, line).end,
                "line {line}"
            );
        }
    }
}
