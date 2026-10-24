//! The syntax thread of a document and the highlight spans of the visible
//! rows.
//!
//! The thread owns the parser ([`Session`]) and posts trees ([`View`]). The
//! UI edits each received view forward with the edits made since, so its
//! positions match the current text until the next parse arrives. Visible
//! rows are highlighted on the UI thread within [`BUDGET`]; rows left over
//! are highlighted by the thread for the next frame. Go to Symbol queries
//! run on the thread too.

use std::collections::VecDeque;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use rte_syntax::{
    InputEdit, Lang, Painter, Session, Span, Symbol, View, Work, changes_near, input_edits,
    input_edits_near,
};
use rte_text::{ChangeSet, Snapshot};
use rte_view::SyntaxSpan;

use crate::document::Document;
use crate::editor::Wake;
use crate::loader::OnDone;

/// UI time for highlighting the visible rows of one frame.
pub const BUDGET: Duration = Duration::from_millis(1);
/// Most symbols Go to Symbol lists.
pub const SYMBOL_LIMIT: usize = 20_000;

/// A syntax thread started by the loader once the text is read, before the
/// editor exists; the editor adopts it when language and text match.
pub type EarlySyntax = Arc<Mutex<Early>>;

#[derive(Default)]
pub struct Early {
    pub(crate) syntax: Option<(&'static Lang, Syntax)>,
    /// The editor decided; a later thread would never be adopted.
    pub(crate) closed: bool,
}

impl Early {
    /// Takes the started thread, if any, and refuses later ones.
    pub fn close(&mut self) -> Option<(&'static Lang, Syntax)> {
        self.closed = true;
        self.syntax.take()
    }
}

/// The language of `path` from its name and first 64 KiB.
pub fn detect(path: &Path, s: &Snapshot) -> Option<&'static Lang> {
    rte_syntax::detect(path, &s.to_vec(0..s.len().min(64 * 1024)))
}

/// A loader hook that starts the syntax thread of `path` into `slot` when
/// its text is read: the `chosen` or detected language, within `max_mib`.
pub fn early_start(
    slot: EarlySyntax,
    path: PathBuf,
    chosen: Option<&'static Lang>,
    max_mib: u64,
    injections: bool,
    wake: Wake,
) -> OnDone {
    Box::new(move |snapshot: &Snapshot| {
        let Some(lang) = chosen.or_else(|| detect(&path, snapshot)) else {
            return;
        };
        if snapshot.len() > max_mib * 1024 * 1024 {
            return;
        }
        if let Ok(mut slot) = slot.lock()
            && !slot.closed
        {
            slot.syntax = Some((
                lang,
                Syntax::start(lang, snapshot.clone(), injections, wake),
            ));
        }
    })
}

enum Msg {
    /// Text changes since the last message: (before, after, changes).
    Edit {
        seq: u64,
        steps: Vec<(Snapshot, Snapshot, ChangeSet)>,
        snapshot: Snapshot,
    },
    Interest(Range<u64>),
    /// Spans of `ranges` for the text of `seq` (over the UI budget). A newer
    /// request replaces an unanswered one.
    Spans {
        seq: u64,
        ranges: Vec<Range<u64>>,
    },
    /// Definitions for Go to Symbol, for the text of `seq`.
    Symbols {
        seq: u64,
    },
    /// The whole-file parse finished (sent by the worker's parse thread).
    Parsed,
    /// A view the UI replaced: releasing the last tree handles can take
    /// milliseconds, so the worker drops it.
    Retire(View),
    /// The UI dropped its [`Syntax`].
    Stop,
}

enum Reply {
    View {
        seq: u64,
        view: View,
        /// State of the thread when it posted the view.
        state: State,
    },
    Spans {
        seq: u64,
        ranges: Vec<Range<u64>>,
        spans: Vec<Span>,
    },
    Symbols {
        seq: u64,
        symbols: Symbols,
    },
    Failed(String),
}

/// Definitions of the document.
#[derive(Clone, Debug, Default)]
pub struct Symbols {
    pub list: Vec<Symbol>,
    /// More than [`SYMBOL_LIMIT`] exist.
    pub truncated: bool,
    /// Only part of a large file is parsed yet.
    pub partial: bool,
}

/// Spans of the last highlighted ranges and what they were computed from.
struct Cache {
    key: Option<(u64, u64, Vec<Range<u64>>)>,
    spans: Vec<SyntaxSpan>,
    /// No row was left over the budget.
    complete: bool,
}

/// How far the syntax thread got when it posted a view.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct State {
    /// Bytes whose highlighting is final for the text of the view.
    current: Range<u64>,
    /// The whole file is parsed and current.
    complete: bool,
    interest: Range<u64>,
}

pub struct Syntax {
    tx: Sender<Msg>,
    rx: Receiver<Reply>,
    /// Set with every message: the thread stops its current work to read it.
    pending: Arc<AtomicBool>,
    /// Text the thread was last told about.
    snapshot: Snapshot,
    seq: u64,
    view: Option<View>,
    /// Text of `view`'s parse.
    view_seq: u64,
    /// Last text whose edits reached `view` only near the visible rows.
    inexact_seq: u64,
    state: State,
    /// Edits sent but not yet in `view`'s parse: (seq, edits).
    log: VecDeque<(u64, Vec<InputEdit>)>,
    interest: Range<u64>,
    /// Increments when a view or worker spans arrive.
    generation: u64,
    /// Thread spans for the current text and view.
    worker_spans: Option<(u64, Vec<Range<u64>>, Vec<Span>)>,
    requested: Option<(u64, Vec<Range<u64>>)>,
    symbols: Option<(u64, Arc<Symbols>)>,
    /// Go to Symbol waits for `symbols`.
    symbols_wanted: bool,
    painter: Painter,
    found: Vec<Span>,
    cache: Cache,
    failed: Option<String>,
}

impl Syntax {
    /// Starts the syntax thread for `snapshot`.
    pub fn start(lang: &'static Lang, snapshot: Snapshot, injections: bool, wake: Wake) -> Syntax {
        let (tx, worker_rx) = mpsc::channel();
        let (worker_tx, rx) = mpsc::channel();
        let pending = Arc::new(AtomicBool::new(false));
        let flag = pending.clone();
        let text = snapshot.clone();
        let to_self = tx.clone();
        let spawned = thread::Builder::new()
            .name("syntax".to_owned())
            .spawn(move || {
                run(
                    lang, text, injections, worker_rx, to_self, worker_tx, flag, wake,
                )
            });
        let failed = spawned.err().map(|e| format!("syntax thread: {e}"));
        Syntax {
            tx,
            rx,
            pending,
            snapshot,
            seq: 0,
            view: None,
            view_seq: 0,
            inexact_seq: 0,
            state: State::default(),
            log: VecDeque::new(),
            interest: 0..0,
            generation: 0,
            worker_spans: None,
            requested: None,
            symbols: None,
            symbols_wanted: false,
            painter: Painter::default(),
            found: Vec::new(),
            cache: Cache {
                key: None,
                spans: Vec::new(),
                complete: false,
            },
            failed,
        }
    }

    /// Why highlighting stopped, if it did.
    pub fn failed(&self) -> Option<&str> {
        self.failed.as_deref()
    }

    /// Started for `snapshot` and given no edits since.
    pub fn is_for(&self, snapshot: &Snapshot) -> bool {
        self.seq == 0 && self.snapshot.same_tree(snapshot)
    }

    /// Changes when the drawn spans may change.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    fn send(&self, msg: Msg) {
        if self.tx.send(msg).is_ok() {
            self.pending.store(true, Ordering::Release);
        }
    }

    /// Hands `view` to the worker to drop, without interrupting its work.
    fn retire(&self, view: View) {
        let _ = self.tx.send(Msg::Retire(view));
    }

    /// Sends the document's changes since the last call to the thread. The
    /// UI's trees get exact edits only `near` the visible rows (positions in
    /// the new text); the thread computes all of them.
    pub fn sync(&mut self, doc: &mut Document, near: Range<u64>) {
        let journal = doc.journal.as_mut().map(std::mem::take).unwrap_or_default();
        if doc.snapshot.same_tree(&self.snapshot) {
            return;
        }
        let mut steps = Vec::new();
        let mut known = self.snapshot.clone();
        for step in journal {
            if !step.before.same_tree(&known) {
                let changes = known.diff(&step.before);
                steps.push((known, step.before.clone(), changes));
            }
            known = step.after.clone();
            steps.push((step.before, step.after, step.changes));
        }
        if !known.same_tree(&doc.snapshot) {
            let changes = known.diff(&doc.snapshot);
            steps.push((known, doc.snapshot.clone(), changes));
        }
        let mut edits = Vec::new();
        let mut exact = true;
        for (i, (old, new, changes)) in steps.iter().enumerate() {
            // Only the last step's positions are those of the visible range.
            let near = if i + 1 == steps.len() {
                near.clone()
            } else {
                0..0
            };
            exact &= changes_near(changes, near.clone());
            edits.extend(input_edits_near(old, new, changes, near));
        }
        self.seq += 1;
        if !exact {
            self.inexact_seq = self.seq;
        }
        if let Some(view) = &mut self.view {
            view.edit(&edits);
        }
        self.log.push_back((self.seq, edits));
        self.snapshot = doc.snapshot.clone();
        self.send(Msg::Edit {
            seq: self.seq,
            steps,
            snapshot: self.snapshot.clone(),
        });
    }

    /// Tells the thread which bytes are visible; `margin` more are parsed
    /// ahead (injections, windows of large files).
    pub fn set_visible(&mut self, visible: Range<u64>, margin: Range<u64>) {
        let inside = self.interest.start <= visible.start && visible.end <= self.interest.end;
        if !inside || self.interest.is_empty() {
            self.interest = margin.clone();
            self.send(Msg::Interest(margin));
        }
    }

    /// Reads the thread's replies; true when the spans may have changed.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        loop {
            match self.rx.try_recv() {
                Ok(Reply::View {
                    seq,
                    mut view,
                    state,
                }) => {
                    for (s, edits) in &self.log {
                        if *s > seq {
                            view.edit(edits);
                        }
                    }
                    while self.log.front().is_some_and(|(s, _)| *s <= seq) {
                        self.log.pop_front();
                    }
                    if let Some(old) = self.view.replace(view) {
                        self.retire(old);
                    }
                    self.view_seq = seq;
                    self.state = state;
                    // Spans of an older tree.
                    self.worker_spans = None;
                    self.requested = None;
                    changed = true;
                }
                Ok(Reply::Spans { seq, ranges, spans }) => {
                    if seq == self.seq {
                        self.worker_spans = Some((seq, ranges, spans));
                        changed = true;
                    }
                }
                Ok(Reply::Symbols { seq, symbols }) => {
                    if seq == self.seq {
                        self.symbols = Some((seq, Arc::new(symbols)));
                    }
                }
                Ok(Reply::Failed(e)) => {
                    self.failed = Some(e);
                    changed = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if self.failed.is_none() {
                        self.failed = Some("the syntax thread stopped".to_owned());
                        changed = true;
                    }
                    break;
                }
            }
        }
        if changed {
            self.generation += 1;
        }
        changed
    }

    /// Spans of the sorted, disjoint `ranges` of the current text.
    pub fn spans(&mut self, snapshot: &Snapshot, ranges: &[Range<u64>]) -> &[SyntaxSpan] {
        if let Some((generation, seq, cached)) = &self.cache.key
            && (*generation, *seq) == (self.generation, self.seq)
            && cached == ranges
        {
            return &self.cache.spans;
        }
        self.found.clear();
        // Thread spans for these ranges of the current view need no UI query.
        let ready = self
            .worker_spans
            .as_ref()
            .filter(|(seq, r, _)| *seq == self.seq && r == ranges);
        let (source, complete) = if let Some((_, _, spans)) = ready {
            (spans, true)
        } else {
            let deadline = Instant::now() + BUDGET;
            let complete = match &self.view {
                Some(view) => self.painter.paint_ranges(
                    view,
                    snapshot,
                    ranges,
                    &|| Instant::now() >= deadline,
                    &mut self.found,
                ),
                None => true,
            };
            let request = (self.seq, ranges.to_vec());
            if !complete && self.requested.as_ref() != Some(&request) {
                self.send(Msg::Spans {
                    seq: self.seq,
                    ranges: ranges.to_vec(),
                });
                self.requested = Some(request);
            }
            (&self.found, complete)
        };
        self.cache.spans.clear();
        self.cache.spans.extend(source.iter().map(|s| SyntaxSpan {
            start: s.start,
            end: s.end,
            style: s.style as u8,
        }));
        self.cache.key = Some((self.generation, self.seq, ranges.to_vec()));
        self.cache.complete = complete;
        &self.cache.spans
    }

    /// The last drawn spans come from trees of the current text that are
    /// final over the drawn ranges, without rows left over the budget.
    pub fn highlighted(&self) -> bool {
        let Some((_, _, drawn)) = &self.cache.key else {
            return false;
        };
        self.view.is_some()
            && self.log.is_empty()
            && drawn
                .iter()
                .all(|r| self.state.current.start <= r.start && r.end <= self.state.current.end)
            && self.cache.complete
    }

    /// Highlighted from a parse of the whole file.
    pub fn fully_highlighted(&self) -> bool {
        self.highlighted() && self.state.complete
    }

    /// The trees, when they match `snapshot` and are final over `range`
    /// (code-aware commands; a window of a large file without its context is
    /// not used).
    pub fn final_view(&self, snapshot: &Snapshot, range: Range<u64>) -> Option<&View> {
        let current = &self.state.current;
        (self.log.is_empty()
            && self.snapshot.same_tree(snapshot)
            && current.start <= range.start
            && range.end <= current.end)
            .then_some(self.view.as_ref())
            .flatten()
    }

    /// The trees, edited to the current text (positions are valid; the
    /// structure near recent edits may be older).
    pub fn view(&self) -> Option<&View> {
        self.view.as_ref()
    }

    /// The view's positions are exact everywhere, not only near the
    /// visible rows.
    pub fn exact(&self) -> bool {
        self.view.is_some() && self.inexact_seq <= self.view_seq
    }

    /// Asks for the definitions of the current text; those of a complete
    /// parse are kept until the text changes.
    pub fn request_symbols(&mut self) {
        self.symbols_wanted = true;
        if !self
            .symbols
            .as_ref()
            .is_some_and(|(seq, _)| *seq == self.seq)
        {
            self.symbols = None;
            self.send(Msg::Symbols { seq: self.seq });
        }
    }

    /// The requested definitions, once they are there.
    pub fn take_symbols(&mut self) -> Option<Arc<Symbols>> {
        if !self.symbols_wanted {
            return None;
        }
        let (seq, symbols) = self.symbols.as_ref()?;
        if *seq != self.seq {
            return None;
        }
        let symbols = symbols.clone();
        self.symbols_wanted = false;
        if symbols.partial {
            self.symbols = None;
        }
        Some(symbols)
    }

    /// The view includes every edit sent.
    #[cfg(test)]
    pub fn settled(&self) -> bool {
        self.view.is_some() && self.log.is_empty()
    }

    /// Identifies this syntax thread.
    #[cfg(test)]
    pub fn id(&self) -> usize {
        Arc::as_ptr(&self.pending) as usize
    }
}

impl Drop for Syntax {
    fn drop(&mut self) {
        if let Some(view) = self.view.take() {
            self.retire(view);
        }
        // Stops a running parse, then the thread.
        self.send(Msg::Stop);
    }
}

/// Requests of the UI waiting for current trees.
#[derive(Default)]
struct Requests {
    seq: u64,
    spans: Option<Vec<Range<u64>>>,
    symbols: bool,
}

#[allow(clippy::too_many_arguments)]
fn run(
    lang: &'static Lang,
    snapshot: Snapshot,
    injections: bool,
    rx: Receiver<Msg>,
    to_self: Sender<Msg>,
    tx: Sender<Reply>,
    pending: Arc<AtomicBool>,
    wake: Wake,
) {
    let reply = |reply: Reply| {
        let sent = tx.send(reply).is_ok();
        wake();
        sent
    };
    let mut session = match Session::new(lang, snapshot, injections) {
        Ok(session) => session,
        Err(e) => {
            reply(Reply::Failed(e));
            return;
        }
    };
    session.set_notify(Arc::new(move || {
        let _ = to_self.send(Msg::Parsed);
    }));
    let mut requests = Requests::default();
    let mut painter = Painter::default();
    let cancel = || pending.load(Ordering::Acquire);
    // State of the last posted view.
    let mut posted: Option<State> = None;
    loop {
        pending.store(false, Ordering::Release);
        loop {
            match rx.try_recv() {
                Ok(msg) => {
                    if !handle(msg, &mut session, &mut requests) {
                        return;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        let seq = requests.seq;
        if requests.spans.is_some() && session.is_current() {
            let ranges = requests.spans.take().unwrap_or_default();
            let mut out = Vec::new();
            if painter.paint_ranges(
                &session.view(),
                session.snapshot(),
                &ranges,
                &cancel,
                &mut out,
            ) {
                if !reply(Reply::Spans {
                    seq,
                    ranges,
                    spans: out,
                }) {
                    return;
                }
            } else {
                // Interrupted: answer after the new message.
                requests.spans = Some(ranges);
            }
        }
        if requests.symbols && session.is_current() {
            let view = session.view();
            if let Some((list, truncated)) =
                rte_syntax::symbols(&view, session.snapshot(), SYMBOL_LIMIT, &cancel)
            {
                requests.symbols = false;
                let symbols = Symbols {
                    list,
                    truncated,
                    partial: !session.is_complete(),
                };
                if !reply(Reply::Symbols { seq, symbols }) {
                    return;
                }
            }
        }
        match session.work(&cancel) {
            Work::Updated => {
                if let Err(e) = rte_syntax::highlighter(lang.grammar) {
                    reply(Reply::Failed(e));
                    return;
                }
                let state = state_of(&session);
                posted = Some(state.clone());
                if !reply(Reply::View {
                    seq,
                    view: session.view(),
                    state,
                }) {
                    return;
                }
            }
            Work::Cancelled => {}
            Work::Idle | Work::Waiting if requests.spans.is_none() && !requests.symbols => {
                // The last step may have changed only the state (an interest
                // range needing no work): post it once.
                let state = state_of(&session);
                if posted.as_ref() != Some(&state) {
                    posted = Some(state.clone());
                    if !reply(Reply::View {
                        seq,
                        view: session.view(),
                        state,
                    }) {
                        return;
                    }
                    continue;
                }
                match rx.recv() {
                    Ok(msg) => {
                        if !handle(msg, &mut session, &mut requests) {
                            return;
                        }
                    }
                    Err(_) => return,
                }
            }
            Work::Idle | Work::Waiting => {}
        }
    }
}

fn state_of(session: &Session) -> State {
    State {
        current: session.current_range(),
        complete: session.is_complete(),
        interest: session.interest().clone(),
    }
}

/// Applies `msg`; false for [`Msg::Stop`].
fn handle(msg: Msg, session: &mut Session, requests: &mut Requests) -> bool {
    match msg {
        Msg::Edit {
            seq,
            steps,
            snapshot,
        } => {
            let edits: Vec<InputEdit> = steps
                .iter()
                .flat_map(|(old, new, changes)| input_edits(old, new, changes))
                .collect();
            session.edit(&edits, snapshot);
            requests.seq = seq;
            requests.spans = None;
        }
        Msg::Interest(range) => session.set_interest(range),
        Msg::Spans { seq, ranges } => {
            if seq == requests.seq {
                requests.spans = Some(ranges);
            }
        }
        Msg::Symbols { seq } => requests.symbols = seq == requests.seq,
        Msg::Parsed => {}
        Msg::Retire(view) => drop(view),
        Msg::Stop => return false,
    }
    true
}
