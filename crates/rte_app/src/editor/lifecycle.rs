//! The file of the session: the empty state, opening a dropped file,
//! closing back to the empty state. One file at a time; nothing here
//! starts, closes or contacts another session.

use super::*;

impl Editor {
    /// True in the empty state (no file open).
    pub(super) fn no_file(&self) -> bool {
        self.path.is_none()
    }

    /// Resets every per-file state to that of a new editor on `path` (the
    /// empty state for `None`). Session settings (zoom, highlighting on or
    /// off, the search query, favorites) stay.
    pub(super) fn reset_document(&mut self, path: Option<PathBuf>) {
        debug_assert!(self.saving.is_none(), "a save is running");
        // An early syntax thread of the command-line file must never start late.
        if let Some(slot) = self.settings.early_syntax.take()
            && let Ok(mut slot) = slot.lock()
        {
            drop(slot.close());
        }
        // Dropping the load stops its loader.
        self.loads = None;
        let mut doc = Document::new(EditOptions::default());
        doc.locks.no_file = path.is_none();
        doc.locks.user = self.settings.read_only;
        self.doc = doc;
        self.title = path
            .as_deref()
            .map_or_else(String::new, |p| p.display().to_string());
        self.path = path;
        self.info = None;
        self.baseline = None;
        self.status = LoadStatus::Ready;
        self.scroll = Scroll::default();
        self.notice = None;
        self.prompt = None;
        self.save_queued = false;
        self.save_refused = false;
        self.closing = None;
        self.drag = None;
        *self.caret_column.get_mut() = None;
        // The seam cache holds a snapshot: it must not keep the old text alive.
        *self.seams.get_mut() = (None, 0, Vec::new());
        // A Go to Line preview belongs to the old file; nothing is restored.
        self.line_preview = None;
        self.language = None;
        self.language_chosen = false;
        // Dropping the syntax thread retires its trees on that thread.
        self.syntax = None;
        self.syntax_over_limit = false;
        self.symbols_pending = false;
        *self.bracket_cache.get_mut() = None;
        self.popup = None;
        self.chord = None;
        self.cursor_history.clear();
        self.column = None;
        self.occurrence_word = None;
        self.expansions.clear();
        self.expanded = None;
        self.symbol_offsets.clear();
        self.marked = (false, false, false);
        self.reset_search();
    }

    /// Why the current file may not be replaced by `dropped` now.
    fn open_refusal(&self, dropped: &Path) -> Option<String> {
        let path = self.path.as_deref()?;
        let (name, other) = (display_name(path), display_name(dropped));
        if self.saving.is_some() {
            Some(format!("{name} is being saved; {other} was not opened"))
        } else if self.doc.is_dirty() || self.prompt.is_some() || self.closing.is_some() {
            Some(format!(
                "{name} has unsaved changes; save or close it to open {other}"
            ))
        } else {
            None
        }
    }

    /// Files dropped on the window: the first opens in place of a file
    /// without unsaved changes (or of the empty state).
    pub(super) fn files_dropped(&mut self, dropped: Result<Dropped, String>) {
        if self.prompt.is_some() {
            return;
        }
        self.popup = None;
        let Dropped { path, others } = match dropped {
            Ok(dropped) => dropped,
            Err(error) => {
                self.notice = Some(error);
                return;
            }
        };
        if let Some(reason) = self.open_refusal(&path) {
            self.notice = Some(reason);
            return;
        }
        if self.highlighting
            && let Some(lang) = rte_syntax::detect(&path, b"")
        {
            rte_syntax::warm(lang.grammar);
        }
        let wake = self.wake.clone();
        let load = loader::spawn(path.clone(), move || wake(), None, false);
        self.notice = Some(format!("Opening {}\u{2026}", display_name(&path)));
        // A newer drop replaces (and stops) a pending one.
        self.pending = Some(PendingOpen {
            notice: (others > 0)
                .then(|| format!("Only one file opens at a time; {others} more ignored")),
            path,
            load,
        });
    }

    /// The first answer of a pending open's loader: a folder or missing
    /// file leaves everything as it was; a file replaces the current one,
    /// unless it gained unsaved changes meanwhile.
    pub(super) fn drain_pending(&mut self) {
        let Some(pending) = &self.pending else {
            return;
        };
        let first = match pending.load.events.try_recv() {
            Ok(event) => event,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                let name = display_name(&pending.path);
                self.pending = None;
                self.notice = Some(format!("{name} could not be opened"));
                return;
            }
        };
        let pending = self.pending.take().expect("checked above");
        if let LoadEvent::Rejected(reason) = first {
            self.notice = Some(reason);
            return;
        }
        if let Some(reason) = self.open_refusal(&pending.path) {
            self.notice = Some(reason);
            return;
        }
        self.reset_document(Some(pending.path));
        self.doc.locks.loading = true;
        self.status = LoadStatus::Loading {
            loaded: 0,
            total: 0,
        };
        self.loads = Some(pending.load);
        self.apply_load(first);
        self.drain_loads();
        if pending.notice.is_some() {
            self.notice = pending.notice;
        }
    }

    /// `File: Close`: back to the empty state, asking first about unsaved
    /// changes; waits for a running save.
    pub(super) fn close_file(&mut self) {
        if self.no_file() {
            self.pending = None;
            self.notice = Some("No file is open; Alt+F4 quits".to_owned());
            return;
        }
        self.pending = None;
        if self.saving.is_some() {
            self.closing = Some(Exit::CloseFile);
        } else if self.doc.is_dirty() {
            self.prompt = Some(Prompt::DirtyClose(Exit::CloseFile));
        } else {
            self.reset_document(None);
        }
    }

    /// Ends what a settled close asked for.
    pub(super) fn finish_exit(&mut self, exit: Exit, ctx: &mut Ctx) {
        match exit {
            Exit::Quit => ctx.close(),
            Exit::CloseFile => self.reset_document(None),
        }
    }
}
