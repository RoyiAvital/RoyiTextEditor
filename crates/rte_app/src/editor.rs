//! The editor window: input, view state, prompts and saving around a
//! [`Document`], plus the command palette and panels drawn above it.

use std::borrow::Cow;
use std::cell::RefCell;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

use rte_edit::{self as edit, Command, EditOptions, Selection, Selections};
use rte_platform::{App, Ctx, Event, Key};
use rte_render::{Renderer, Target};
use rte_text::{Encoding, LineEnding, Snapshot};
use rte_view::{
    CaretStyle, Cell, Chrome, DisplayList, Frame, Layout, Options, PaletteFrame, PaletteGeometry,
    PaletteRow, PanelFrame, PanelGeometry, PanelLine, Scroll, Theme, Tone, ViewBuilder,
    draw_palette, draw_panel, hit_test,
};
use unicode_width::UnicodeWidthStr;

use crate::bench;
use crate::clip;
use crate::commands::{self, Action, COMMANDS, CommandDef, EDITING_KEYS, vk::*};
use crate::document::{Document, Locks};
use crate::favorites::{self, FileStorage, MAX_FAVORITES, Store};
use crate::loader::{FileInfo, LoadEvent};
use crate::palette::{Item, Mode, Motion, Palette};
use crate::save::{self, Baseline, FileOps, SaveOutcome, SaveRequest};
use crate::trace::Trace;

/// Font size in pixels at 96 DPI and zoom 0.
pub const BASE_PX: f32 = 14.0;
const WHEEL_NOTCH: i32 = 120;
const WHEEL_LINES: i32 = 3;
const WHEEL_COLUMNS: i32 = 6;
/// Columns kept visible beside the caret when scrolling horizontally.
const REVEAL_MARGIN: u64 = 4;
/// Selections up to this size show a character count.
const COUNT_LIMIT: u64 = 64 * 1024;

/// Wakes the window thread from worker threads.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

enum LoadStatus {
    Loading { loaded: u64, total: u64 },
    Ready,
    Failed(String),
}

pub struct Settings {
    pub read_only: bool,
    pub exit_after_first_frame: bool,
    pub exit_after_load: bool,
    /// Recent commands listed by `Ctrl+P` (configuration).
    pub recent_count: usize,
    /// Folder of the palette state file; `None` keeps it in memory.
    pub user_dir: Option<PathBuf>,
    /// Shown on the status line at startup (configuration problems).
    pub notice: Option<String>,
    /// `false` after `--syntax none` / `--syntax off`.
    pub highlighting: bool,
    /// Scripted keys of an interaction benchmark.
    pub bench: Option<bench::Script>,
}

/// Drawn above the text; takes the keyboard while open.
enum Popup {
    Palette(Palette),
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
    DirtyClose,
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
}

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
    loads: Option<Receiver<LoadEvent>>,
    settings: Settings,
    dpi: u32,
    zoom: i32,
    wheel: (i32, i32),
    trace: Trace,
    notice: Option<String>,
    prompt: Option<Prompt>,
    saving: Option<Receiver<SaveOutcome>>,
    save_queued: bool,
    /// Close once the running save finishes.
    closing: bool,
    drag: Option<Drag>,
    files: Arc<dyn FileOps>,
    wake: Wake,
    shown_dirty: bool,
    /// Column of the last queried caret: snapshot, offset, tab width, column.
    caret_column: RefCell<Option<(Snapshot, u64, u32, u64)>>,
    /// A frame was drawn (traced as `first_paint`).
    painted: bool,
    /// Syntax highlighting is wanted (set per process; applies once it ships).
    highlighting: bool,
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
    locks: Locks,
    /// Load state and shown percentage.
    status: (u8, u64),
    new_file: bool,
    /// Fingerprint of what the popup shows.
    popup: u64,
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
            && self.locks == other.locks
            && self.status == other.status
            && self.new_file == other.new_file
            && self.popup == other.popup
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

/// Window title for `path`, marked when the document has unsaved changes.
pub fn window_title(path: Option<&Path>, dirty: bool) -> String {
    let name = path.map_or_else(
        || "Untitled".to_owned(),
        |p| {
            p.file_name().map_or_else(
                || p.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            )
        },
    );
    let mark = if dirty { "\u{25CF} " } else { "" };
    format!("{mark}{name} - {APP_NAME}")
}

impl Editor {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        path: Option<&Path>,
        first: Option<LoadEvent>,
        loads: Option<Receiver<LoadEvent>>,
        renderer: Renderer,
        dpi: u32,
        mut settings: Settings,
        trace: Trace,
        files: Arc<dyn FileOps>,
        wake: Wake,
    ) -> Editor {
        let title = path.map_or_else(|| "Untitled".to_owned(), |p| p.display().to_string());
        let mut doc = Document::new(EditOptions::default());
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
            settings,
            dpi,
            zoom: 0,
            wheel: (0, 0),
            trace,
            notice,
            prompt: None,
            saving: None,
            save_queued: false,
            closing: false,
            drag: None,
            files,
            wake,
            shown_dirty: false,
            caret_column: RefCell::new(None),
            painted: false,
            highlighting,
            bench,
            shown: false,
            popup: None,
            store: None,
            skip_space: false,
            ui_renderer: None,
            about_icon: None,
        };
        if let Some(event) = first {
            editor.apply_load(event);
        }
        // Events queued before the waker existed produced no wake-up.
        editor.drain_loads();
        editor
    }

    /// Applies queued load events. A loader that disconnects before its final
    /// event becomes a visible failure instead of an endless "Loading".
    fn drain_loads(&mut self) {
        let Some(rx) = &self.loads else {
            return;
        };
        let mut events = Vec::new();
        let disconnected = loop {
            match rx.try_recv() {
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
            }
            LoadEvent::Failed { snapshot, error } => {
                self.doc.load(snapshot);
                self.doc.locks.load_error = true;
                self.status = LoadStatus::Failed(error);
            }
        }
        self.clamp_scroll();
    }

    /// `--exit-after-load`: report the trace and close once loading ended.
    fn exit_if_loaded(&mut self, ctx: &mut Ctx) {
        if self.settings.exit_after_load && !matches!(self.status, LoadStatus::Loading { .. }) {
            self.trace.report();
            ctx.close();
        }
    }

    fn px(&self) -> f32 {
        let zoom = 1.1f32.powi(self.zoom);
        (BASE_PX * self.dpi as f32 / 96.0 * zoom).round()
    }

    fn layout(&self) -> Layout {
        Layout::new(
            self.size.0,
            self.size.1,
            self.renderer.cell(),
            self.doc.snapshot.line_count(),
            &self.options,
        )
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
            locks: self.doc.locks,
            status,
            new_file: self.info.is_some_and(|i| i.new_file),
            popup: self.popup_view(),
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
        if self.path.is_none() {
            self.notice = Some(
                "Untitled documents cannot be saved; start the editor with a file path".to_owned(),
            );
            return false;
        }
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
        } else if self.closing && self.prompt.is_none() {
            self.closing = false;
            if self.doc.is_dirty() {
                self.prompt = Some(Prompt::DirtyClose);
            } else {
                ctx.close();
            }
        }
    }

    fn request_close(&mut self, ctx: &mut Ctx) {
        if self.saving.is_some() {
            self.closing = true;
        } else if self.doc.is_dirty() {
            self.prompt = Some(Prompt::DirtyClose);
        } else {
            ctx.close();
        }
    }

    fn prompt_key(&mut self, prompt: Prompt, key: Key, ctx: &mut Ctx) {
        let plain = !key.ctrl && !key.alt;
        match (prompt, key.vk) {
            (_, VK_ESCAPE) => {
                self.prompt = None;
                self.closing = false;
            }
            (Prompt::DirtyClose, VK_RETURN | VK_S) if plain && self.path.is_some() => {
                self.prompt = None;
                self.closing = true;
                if !self.request_save() {
                    self.closing = false;
                    self.prompt = Some(Prompt::DirtyClose);
                }
            }
            (Prompt::DirtyClose, VK_D | VK_N) if plain => ctx.close(),
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
        if self.closing {
            return true;
        }
        if key.alt {
            // Unbound Alt keys keep their system behavior (window menu).
            let Some(def) = commands::for_key(key) else {
                return false;
            };
            self.perform(def.action, ctx);
            return true;
        }
        self.skip_space = false;
        match self.popup {
            Some(Popup::Palette(_)) => return self.palette_key(key, ctx),
            Some(Popup::Panel(_)) => return self.panel_key(key, ctx),
            None => {}
        }
        self.notice = None;
        if let Some(def) = commands::for_key(key) {
            self.perform(def.action, ctx);
            return true;
        }
        self.edit_key(key)
    }

    /// Runs a registry command (from a key or the palette).
    fn perform(&mut self, action: Action, ctx: &mut Ctx) {
        let (s, sels) = (&self.doc.snapshot, &self.doc.selections);
        match action {
            Action::Save => {
                self.request_save();
            }
            Action::Close | Action::Quit => self.request_close(ctx),
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
            }
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
        if self.prompt.is_some() || self.closing || ch < ' ' || ch == '\u{7F}' {
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
        self.notice = None;
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
            _ => self.popup = Some(Popup::Palette(Palette::new(mode, store, recent))),
        }
    }

    fn refresh_palette(&mut self) {
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
        } else if self.popup.is_some() {
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
                (p.mode == Mode::All).hash(&mut h);
                p.query.hash(&mut h);
                (p.selected, p.first, p.items.len()).hash(&mut h);
                self.store.as_ref().map(Store::rev).hash(&mut h);
            }
            Some(Popup::Panel(panel)) => {
                2u8.hash(&mut h);
                (panel.title, panel.first).hash(&mut h);
            }
        }
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
    /// command runs before the state file is written.
    fn run_palette_item(&mut self, item: usize, ctx: &mut Ctx) {
        let Some(Popup::Palette(p)) = &self.popup else {
            return;
        };
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
                });
                let placeholder = match p.mode {
                    Mode::All => "Type a command name",
                    Mode::Favorites => "Search recent and favorite commands",
                };
                let empty = if p.mode == Mode::Favorites && p.query.as_str().is_empty() {
                    "Star commands in Ctrl+Shift+P to add them here"
                } else {
                    "No matching commands"
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
            None => {}
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

    fn mouse_down(&mut self, x: i32, y: i32, clicks: u32, shift: bool) {
        let layout = self.layout();
        let body = layout.text.contains(x, y) || layout.gutter.contains(x, y);
        if !body || self.prompt.is_some() || self.closing {
            return;
        }
        self.notice = None;
        let pos = self.hit(x, y);
        let (unit, origin) = match clicks {
            1 if shift => {
                let anchor = self.doc.selections.primary().anchor;
                (Unit::Char, (anchor, anchor))
            }
            1 => (Unit::Char, (pos, pos)),
            2 => (Unit::Word, self.word_span(pos)),
            _ => (Unit::Line, self.line_span(pos)),
        };
        self.drag = Some(Drag { unit, origin });
        self.drag_to(pos);
    }

    fn drag_to(&mut self, pos: u64) {
        let Some(Drag { unit, origin }) = self.drag else {
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
        self.select_single(selection);
        self.reveal();
    }

    fn status_text(&self) -> (String, String) {
        let name = self.path.as_deref().map_or_else(
            || "Untitled".to_owned(),
            |p| {
                p.file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
            },
        );
        let left = match (self.prompt, self.path.is_some()) {
            (Some(Prompt::DirtyClose), true) => {
                format!("Save changes to {name}?   Enter: Save   D: Don't Save   Esc: Cancel")
            }
            (Some(Prompt::DirtyClose), false) => {
                "Discard the untitled document?   D: Discard   Esc: Cancel".to_owned()
            }
            (Some(Prompt::Conflict(_)), _) => {
                format!("{name} changed on disk.   O: Overwrite   Esc: Cancel")
            }
            (None, _) if self.saving.is_some() => "Saving\u{2026}".to_owned(),
            (None, _) => match (&self.notice, &self.status) {
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
        let mut right = vec![format!(
            "Ln {}, Col {}",
            s.line_of(primary.head) + 1,
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
        let mut handled = true;
        match event {
            Event::Shown => {
                self.trace.mark("first_frame");
                if !self.settings.exit_after_load {
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
                    bench.key_arrived(key);
                }
                handled = self.key(key, ctx);
            }
            Event::Char(ch) => self.typed(ch),
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
                ..
            } => {
                if self.popup.is_some() {
                    self.popup_click(x, y, ctx);
                } else {
                    self.mouse_down(x, y, clicks, shift);
                }
            }
            Event::MouseMove { x, y } => {
                if self.drag.is_some() {
                    let pos = self.hit(x, y);
                    self.drag_to(pos);
                }
            }
            Event::MouseUp => self.drag = None,
            Event::CloseRequested => self.request_close(ctx),
            Event::Wake => {
                self.drain_loads();
                self.drain_saves(ctx);
                self.drain_store();
                self.exit_if_loaded(ctx);
            }
        }
        let dirty = self.doc.is_dirty();
        if dirty != self.shown_dirty {
            self.shown_dirty = dirty;
            ctx.set_title(&window_title(self.path.as_deref(), dirty));
        }
        if self.prompt.is_some() {
            self.popup = None;
        }
        self.sync_ui_renderer();
        let layout_changed = matches!(event, Event::Shown | Event::Resized { .. });
        let redraw = layout_changed || !self.visible().same(&before);
        if redraw {
            ctx.redraw();
        }
        if let Some(bench) = &mut self.bench {
            bench.check_frame(redraw, ctx);
            if self.shown && !matches!(self.status, LoadStatus::Loading { .. }) {
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
        let title_mark = self.doc.is_dirty().then(|| {
            if self.path.is_some() {
                self.theme.unsaved
            } else {
                self.theme.at_risk
            }
        });
        let caret = if self.prompt.is_some() {
            CaretStyle::Hidden
        } else if self.doc.options.overtype {
            CaretStyle::Underline
        } else {
            CaretStyle::Bar
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
        if let Some(bench) = &mut self.bench {
            bench.presented(ctx);
        }
    }

    fn is_caption(&self, x: i32, y: i32) -> bool {
        self.layout().title.contains(x, y)
    }

    fn is_text(&self, x: i32, y: i32) -> bool {
        self.layout().text.contains(x, y) && !self.popup_area().is_some_and(|r| r.contains(x, y))
    }
}

/// Whether `def` can run under `locks`; Save also needs a path.
fn enabled(def: &CommandDef, locks: Locks, has_path: bool) -> bool {
    match def.action {
        Action::Save => has_path && locks.save_refusal().is_none(),
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
        Renderer::new(Cow::Borrowed(rte_assets::DEFAULT_FONT), 14.0).unwrap()
    }

    fn settings(read_only: bool, exit_after_load: bool) -> Settings {
        Settings {
            read_only,
            exit_after_first_frame: false,
            exit_after_load,
            recent_count: 3,
            user_dir: None,
            notice: None,
            highlighting: true,
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
            Some(loads),
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
            Some(rx),
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
    fn untitled_documents_are_never_saved() {
        let mut editor = Editor::new(
            None,
            None,
            None,
            renderer(),
            96,
            settings(false, false),
            Trace::new(false),
            Arc::new(FakeFiles::default()),
            Arc::new(|| {}),
        );
        let mut ctx = Ctx::headless(800, 600, 96);
        type_text(&mut editor, &mut ctx, "x");
        editor.event(key(VK_S, true, false), &mut ctx);
        assert!(editor.saving.is_none());
        assert!(editor.notice.as_deref().unwrap().starts_with("Untitled"));
        editor.event(Event::CloseRequested, &mut ctx);
        assert!(editor.status_text().0.starts_with("Discard"));
        editor.event(key(VK_S, false, false), &mut ctx);
        assert!(!ctx.close_requested());
        editor.event(key(VK_D, false, false), &mut ctx);
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
        assert_eq!(editor.prompt, Some(Prompt::DirtyClose));
        editor.event(key(VK_ESCAPE, false, false), &mut ctx);
        assert_eq!(editor.prompt, None);
        assert!(!ctx.close_requested());
        editor.event(key(VK_W, true, false), &mut ctx);
        editor.event(key(VK_RETURN, false, false), &mut ctx);
        settle(&mut editor, &mut ctx);
        assert!(ctx.close_requested());
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

    #[test]
    fn a_pristine_untitled_document_closes_without_asking() {
        let untitled = || {
            Editor::new(
                None,
                None,
                None,
                renderer(),
                96,
                settings(false, false),
                Trace::new(false),
                Arc::new(FakeFiles::default()),
                Arc::new(|| {}),
            )
        };
        let mut editor = untitled();
        let mut ctx = Ctx::headless(800, 600, 96);
        assert!(editor.status_text().1.starts_with("Ln 1"));
        editor.event(Event::CloseRequested, &mut ctx);
        assert!(ctx.close_requested());
        let mut editor = untitled();
        let mut ctx = Ctx::headless(800, 600, 96);
        type_text(&mut editor, &mut ctx, "x");
        editor.event(key(VK_Z, true, false), &mut ctx);
        editor.event(Event::CloseRequested, &mut ctx);
        assert!(ctx.close_requested());
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
            editor.event(event, &mut ctx);
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

    fn palette_labels(editor: &Editor) -> Vec<&'static str> {
        palette(editor)
            .items
            .iter()
            .map(|item| match item {
                Item::Header(h) => h,
                Item::Command { index, .. } => COMMANDS[*index].label,
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
        type_text(&mut editor, &mut ctx, "select all");
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
            editor.event(event, &mut ctx);
            assert!(!ctx.redraw_requested(), "{event:?}");
        }
        for event in [
            Event::Char('s'),
            key(VK_DOWN, false, false),
            key(VK_HOME, false, true),
        ] {
            let mut ctx = Ctx::headless(800, 600, 96);
            editor.event(event, &mut ctx);
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
        type_text(&mut editor, &mut ctx, "select all");
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
        settings.bench = Some(bench::parse("Ctrl+Shift+P,Escape", "2").unwrap());
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
}
