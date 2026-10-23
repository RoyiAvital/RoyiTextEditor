//! The editor window: a read-only document view (M2).

use std::path::Path;
use std::sync::mpsc::{Receiver, TryRecvError};

use rte_platform::{App, Ctx, Event, Key};
use rte_render::{Renderer, Target};
use rte_text::{Encoding, LineEnding, Snapshot};
use rte_view::{Chrome, DisplayList, Frame, Layout, Options, Scroll, Theme, ViewBuilder};

use crate::loader::{FileInfo, LoadEvent};
use crate::trace::Trace;

const VK_PRIOR: u16 = 0x21;
const VK_NEXT: u16 = 0x22;
const VK_END: u16 = 0x23;
const VK_HOME: u16 = 0x24;
const VK_LEFT: u16 = 0x25;
const VK_UP: u16 = 0x26;
const VK_RIGHT: u16 = 0x27;
const VK_DOWN: u16 = 0x28;
const VK_0: u16 = 0x30;
const VK_W: u16 = 0x57;
const VK_ADD: u16 = 0x6B;
const VK_SUBTRACT: u16 = 0x6D;
const VK_OEM_PLUS: u16 = 0xBB;
const VK_OEM_MINUS: u16 = 0xBD;

/// Font size in pixels at 96 DPI and zoom 0.
pub const BASE_PX: f32 = 14.0;
const WHEEL_NOTCH: i32 = 120;
const WHEEL_LINES: i32 = 3;
const WHEEL_COLUMNS: i32 = 6;

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

pub struct Editor {
    snapshot: Snapshot,
    renderer: Renderer,
    view: ViewBuilder,
    list: DisplayList,
    options: Options,
    theme: Theme,
    scroll: Scroll,
    layout: Option<Layout>,
    title: String,
    info: Option<FileInfo>,
    status: LoadStatus,
    loads: Option<Receiver<LoadEvent>>,
    settings: Settings,
    dpi: u32,
    zoom: i32,
    wheel: (i32, i32),
    trace: Trace,
}

impl Editor {
    pub fn new(
        path: Option<&Path>,
        first: Option<LoadEvent>,
        loads: Option<Receiver<LoadEvent>>,
        renderer: Renderer,
        dpi: u32,
        settings: Settings,
        trace: Trace,
    ) -> Editor {
        let title = path.map_or_else(|| "Untitled".to_owned(), |p| p.display().to_string());
        let mut editor = Editor {
            snapshot: Snapshot::empty(),
            renderer,
            view: ViewBuilder::default(),
            list: DisplayList::default(),
            options: Options::default(),
            theme: Theme::DARK,
            scroll: Scroll::default(),
            layout: None,
            title,
            info: None,
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
                    snapshot: self.snapshot.clone(),
                    error: "loading stopped unexpectedly".to_owned(),
                });
            }
        }
    }

    fn apply_load(&mut self, event: LoadEvent) {
        if !matches!(event, LoadEvent::Progress { .. }) {
            self.trace.mark("load_done");
        }
        match event {
            LoadEvent::Progress {
                snapshot,
                loaded,
                total,
            } => {
                self.snapshot = snapshot;
                self.status = LoadStatus::Loading { loaded, total };
            }
            LoadEvent::Done { snapshot, info } => {
                self.snapshot = snapshot;
                self.info = Some(info);
                self.status = LoadStatus::Ready;
            }
            LoadEvent::Failed { snapshot, error } => {
                self.snapshot = snapshot;
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

    fn rows(&self) -> u64 {
        self.layout.map_or(1, |l| u64::from(l.rows.max(2) - 1))
    }

    fn scroll_lines(&mut self, delta: i64) {
        self.scroll.top_line = self.scroll.top_line.saturating_add_signed(delta);
        self.clamp_scroll();
    }

    fn scroll_cols(&mut self, delta: i64) {
        self.scroll.first_col = self.scroll.first_col.saturating_add_signed(delta);
    }

    fn clamp_scroll(&mut self) {
        let last = self.snapshot.line_count() - 1;
        self.scroll.top_line = self.scroll.top_line.min(last);
    }

    fn set_zoom(&mut self, zoom: i32) {
        self.zoom = zoom.clamp(-8, 16);
        let px = self.px();
        self.renderer.set_size(px);
    }

    fn key(&mut self, key: Key, ctx: &mut Ctx) -> bool {
        let page = self.rows() as i64;
        match (key.vk, key.ctrl, key.alt) {
            (VK_W, true, false) => ctx.close(),
            (VK_UP, _, false) => self.scroll_lines(-1),
            (VK_DOWN, _, false) => self.scroll_lines(1),
            (VK_PRIOR, _, _) => self.scroll_lines(-page),
            (VK_NEXT, _, _) => self.scroll_lines(page),
            (VK_HOME, true, false) => self.scroll = Scroll::default(),
            (VK_END, true, false) => self.scroll_lines(i64::MAX),
            (VK_HOME, false, false) => self.scroll.first_col = 0,
            (VK_LEFT, false, false) => self.scroll_cols(-1),
            (VK_RIGHT, false, false) => self.scroll_cols(1),
            (VK_OEM_PLUS | VK_ADD, true, false) => self.set_zoom(self.zoom + 1),
            (VK_OEM_MINUS | VK_SUBTRACT, true, false) => self.set_zoom(self.zoom - 1),
            (VK_0, true, false) => self.set_zoom(0),
            _ => return false,
        }
        ctx.redraw();
        true
    }

    fn status_text(&self) -> (String, String) {
        let left = match &self.status {
            LoadStatus::Loading { loaded, total } => {
                format!("Loading {}%", loaded * 100 / (*total).max(1))
            }
            LoadStatus::Failed(error) => format!("Read error: {error}"),
            LoadStatus::Ready => {
                let lines = self.snapshot.line_count();
                match self.info {
                    Some(info) if info.new_file => "New file".to_owned(),
                    Some(info) if info.malformed => {
                        format!("{lines} lines · malformed UTF-16, read-only")
                    }
                    _ => format!("{lines} lines"),
                }
            }
        };
        let mut right = Vec::new();
        if let Some(info) = self.info {
            right.push(match info.encoding {
                Encoding::Utf8 => "UTF-8",
                Encoding::Utf8Bom => "UTF-8 BOM",
                Encoding::Utf16Le => "UTF-16 LE",
                Encoding::Utf16Be => "UTF-16 BE",
            });
            right.push(match info.line_ending {
                LineEnding::Lf => "LF",
                LineEnding::CrLf => "CRLF",
            });
        }
        if self.settings.read_only {
            right.push("RO");
        }
        (left, right.join(" · "))
    }
}

impl App for Editor {
    fn event(&mut self, event: Event, ctx: &mut Ctx) -> bool {
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
            Event::Resized { .. } => ctx.redraw(),
            Event::DpiChanged(dpi) => {
                self.dpi = dpi;
                let px = self.px();
                self.renderer.set_size(px);
                ctx.redraw();
            }
            Event::Key(key) => return self.key(key, ctx),
            Event::Char(_) => return false,
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
                ctx.redraw();
            }
            Event::CloseRequested => ctx.close(),
            Event::Wake => {
                self.drain_loads();
                self.exit_if_loaded(ctx);
                ctx.redraw();
            }
        }
        true
    }

    fn paint(&mut self, pixels: &mut [u32], width: u32, height: u32) {
        let cell = self.renderer.cell();
        let layout = Layout::new(
            width,
            height,
            cell,
            self.snapshot.line_count(),
            &self.options,
        );
        self.layout = Some(layout);
        let (left, right) = self.status_text();
        let frame = Frame {
            snapshot: &self.snapshot,
            layout: &layout,
            scroll: self.scroll,
            cell,
            theme: &self.theme,
            chrome: Chrome {
                title: &self.title,
                status_left: &left,
                status_right: &right,
            },
            options: &self.options,
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
    }

    fn is_caption(&self, x: i32, y: i32) -> bool {
        self.layout.is_some_and(|l| l.title.contains(x, y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;
    use std::sync::mpsc;

    use crate::loader::FileInfo;

    fn editor(first: Option<LoadEvent>, loads: Receiver<LoadEvent>) -> Editor {
        let renderer = Renderer::new(Cow::Borrowed(rte_assets::DEFAULT_FONT), 14.0).unwrap();
        Editor::new(
            Some(Path::new("test.txt")),
            first,
            Some(loads),
            renderer,
            96,
            Settings {
                read_only: false,
                exit_after_first_frame: false,
                exit_after_load: true,
            },
            Trace::new(false),
        )
    }

    fn done(text: &[u8]) -> LoadEvent {
        LoadEvent::Done {
            snapshot: Snapshot::from_bytes(text),
            info: FileInfo {
                encoding: Encoding::Utf8,
                line_ending: LineEnding::Lf,
                malformed: false,
                new_file: false,
            },
        }
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
        let mut editor = editor(first, rx);
        assert!(matches!(editor.status, LoadStatus::Ready));
        assert_eq!(editor.snapshot.to_vec(0..6), b"prefix");
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
        let editor = editor(first, rx);
        assert!(matches!(editor.status, LoadStatus::Failed(_)));
        assert_eq!(editor.snapshot.len(), 4);
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
        let mut editor = editor(first, rx);
        assert!(matches!(editor.status, LoadStatus::Loading { .. }));
        tx.send(done(b"ab")).unwrap();
        let mut ctx = Ctx::headless(800, 600, 96);
        editor.event(Event::Wake, &mut ctx);
        assert!(matches!(editor.status, LoadStatus::Ready));
        assert!(ctx.close_requested());
    }
}
