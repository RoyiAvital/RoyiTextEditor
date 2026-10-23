//! The editor window: input, view state, prompts and saving around a
//! [`Document`].

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

use rte_edit::{self as edit, Command, EditOptions, Selection, Selections};
use rte_platform::{App, Ctx, Event, Key};
use rte_render::{Renderer, Target};
use rte_text::{Encoding, LineEnding, Snapshot};
use rte_view::{
    CaretStyle, Chrome, DisplayList, Frame, Layout, Options, Scroll, Theme, ViewBuilder, hit_test,
};

use crate::clip;
use crate::document::{Document, Locks};
use crate::loader::{FileInfo, LoadEvent};
use crate::save::{self, Baseline, FileOps, SaveOutcome, SaveRequest};
use crate::trace::Trace;

const VK_BACK: u16 = 0x08;
const VK_TAB: u16 = 0x09;
const VK_RETURN: u16 = 0x0D;
const VK_ESCAPE: u16 = 0x1B;
const VK_PRIOR: u16 = 0x21;
const VK_NEXT: u16 = 0x22;
const VK_END: u16 = 0x23;
const VK_HOME: u16 = 0x24;
const VK_LEFT: u16 = 0x25;
const VK_UP: u16 = 0x26;
const VK_RIGHT: u16 = 0x27;
const VK_DOWN: u16 = 0x28;
const VK_INSERT: u16 = 0x2D;
const VK_DELETE: u16 = 0x2E;
const VK_0: u16 = 0x30;
const VK_A: u16 = 0x41;
const VK_C: u16 = 0x43;
const VK_D: u16 = 0x44;
const VK_N: u16 = 0x4E;
const VK_O: u16 = 0x4F;
const VK_S: u16 = 0x53;
const VK_V: u16 = 0x56;
const VK_W: u16 = 0x57;
const VK_X: u16 = 0x58;
const VK_Y: u16 = 0x59;
const VK_Z: u16 = 0x5A;
const VK_ADD: u16 = 0x6B;
const VK_SUBTRACT: u16 = 0x6D;
const VK_F4: u16 = 0x73;
const VK_OEM_PLUS: u16 = 0xBB;
const VK_OEM_MINUS: u16 = 0xBD;

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
    locks: Locks,
    /// Load state and shown percentage.
    status: (u8, u64),
    new_file: bool,
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
            && self.locks == other.locks
            && self.status == other.status
            && self.new_file == other.new_file
    }
}

/// Application name from `Meta/AppMeta.json` (exported by the build script).
pub const APP_NAME: &str = env!("RTE_APP_NAME");

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
        settings: Settings,
        trace: Trace,
        files: Arc<dyn FileOps>,
        wake: Wake,
    ) -> Editor {
        let title = path.map_or_else(|| "Untitled".to_owned(), |p| p.display().to_string());
        let mut doc = Document::new(EditOptions::default());
        doc.locks.user = settings.read_only;
        doc.locks.loading = loads.is_some();
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
            notice: None,
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
            locks: self.doc.locks,
            status,
            new_file: self.info.is_some_and(|i| i.new_file),
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
        let Key {
            vk,
            ctrl,
            shift,
            alt,
        } = key;
        if alt {
            return false;
        }
        self.notice = None;
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
            (VK_A, true) => self.select_single(Selection::new(0, self.doc.snapshot.len())),
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
                let (s, sels) = (&self.doc.snapshot, &self.doc.selections);
                let cmd = if shift {
                    edit::outdent(s, sels, &opts)
                } else {
                    Some(edit::tab(s, sels, &opts))
                };
                self.run(cmd, None);
            }
            (VK_INSERT, false) if !shift => {
                self.doc.options.overtype = !self.doc.options.overtype;
            }
            (VK_Z, true) => self.undo_redo(shift),
            (VK_Y, true) => self.undo_redo(true),
            (VK_C, true) => self.copy(ctx, false),
            (VK_X, true) => self.copy(ctx, true),
            (VK_V, true) => self.paste(ctx),
            (VK_S, true) => {
                self.request_save();
            }
            (VK_W | VK_F4, true) => self.request_close(ctx),
            (VK_OEM_PLUS | VK_ADD, true) => self.set_zoom(self.zoom + 1),
            (VK_OEM_MINUS | VK_SUBTRACT, true) => self.set_zoom(self.zoom - 1),
            (VK_0, true) => self.set_zoom(0),
            _ => return false,
        }
        true
    }

    fn typed(&mut self, ch: char) {
        if self.prompt.is_some() || self.closing || ch < ' ' || ch == '\u{7F}' {
            return;
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
                self.exit_if_loaded(ctx);
            }
            Event::Resized { .. } => {}
            Event::DpiChanged(dpi) => {
                self.dpi = dpi;
                let px = self.px();
                self.renderer.set_size(px);
            }
            Event::Key(key) => handled = self.key(key, ctx),
            Event::Char(ch) => self.typed(ch),
            Event::Wheel {
                delta,
                horizontal,
                shift,
                ctrl,
            } => {
                if ctrl && !horizontal {
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
            } => self.mouse_down(x, y, clicks, shift),
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
                self.exit_if_loaded(ctx);
            }
        }
        let dirty = self.doc.is_dirty();
        if dirty != self.shown_dirty {
            self.shown_dirty = dirty;
            ctx.set_title(&window_title(self.path.as_deref(), dirty));
        }
        let layout_changed = matches!(event, Event::Shown | Event::Resized { .. });
        if layout_changed || !self.visible().same(&before) {
            ctx.redraw();
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
        self.renderer.draw(
            &self.list,
            &mut Target {
                pixels,
                width,
                height,
            },
        );
        if !self.painted {
            self.painted = true;
            self.trace.mark("first_paint");
        }
    }

    fn is_caption(&self, x: i32, y: i32) -> bool {
        self.layout().title.contains(x, y)
    }

    fn is_text(&self, x: i32, y: i32) -> bool {
        self.layout().text.contains(x, y)
    }
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
            Some("\u{25CF} test.txt - RoyiTextEditor")
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
        assert_eq!(ctx.requested_title(), Some("test.txt - RoyiTextEditor"));
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
}
