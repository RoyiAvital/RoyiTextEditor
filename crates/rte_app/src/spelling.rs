//! The spelling thread, started the first time spelling is on for a plain
//! text or Markdown file. It owns the engine, the user dictionary and the
//! cache of checked words. Requests come in three lanes served in priority
//! order (Suggest, Marks, Navigate), each holding at most one request: a
//! newer one replaces it. Replies carry the request's identity (lane, id,
//! generation, snapshot); the UI accepts only current ones. Markdown
//! requests first ask the syntax worker which text is not prose. The thread
//! blocks on its channel when idle.

use std::ops::Range;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use rte_spell::{
    Builtin, Checker, Hunspell, Speller, USER_MAX_BYTES, Word, encode_user, merge_user, parse_user,
    words,
};
use rte_text::Snapshot;

use crate::editor::Wake;
use crate::favorites::Storage;
use crate::syntax::{MaskAnswer, MaskPort, MaskRequest};

/// Bytes, words and marks of one Marks request; beyond them the reply
/// covers a prefix and the UI asks for the rest.
pub const MAX_REQUEST_BYTES: u64 = 256 * 1024;
pub const MAX_REQUEST_WORDS: usize = 16_384;
pub const MAX_MARKS: usize = 4_096;
/// A navigation slice: at most this many bytes or this long.
const SLICE_BYTES: u64 = 256 * 1024;
const SLICE_TIME: Duration = Duration::from_millis(2);
/// Words checked between deadline checks of a slice.
const SLICE_CHECK: usize = 64;
/// Navigation reports its progress this often.
const PROGRESS_EVERY: Duration = Duration::from_millis(200);
/// Cooperative budget of the suggestions of one word.
const SUGGEST_TIME: Duration = Duration::from_millis(20);
pub const MAX_SUGGESTIONS: usize = 8;
/// Bytes searched for whitespace when a range is widened to whole tokens.
const BOUNDARY_SCAN: u64 = 256;
pub const USER_FILE: &str = "en_US.txt";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engine {
    Builtin,
    Hunspell,
    Windows,
}

impl Engine {
    pub fn parse(text: &str) -> Option<Engine> {
        match text {
            "builtin" => Some(Engine::Builtin),
            "hunspell" => Some(Engine::Hunspell),
            "windows" => Some(Engine::Windows),
            _ => None,
        }
    }
}

pub struct Config {
    pub engine: Engine,
    /// Base path of the Hunspell `.aff` and `.dic` files.
    pub hunspell: PathBuf,
    /// The user dictionary file; `None` keeps added words in memory.
    pub storage: Option<Box<dyn Storage>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lane {
    Suggest = 0,
    Marks = 1,
    Navigate = 2,
}

/// A misspelled word and its bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mark {
    pub range: Range<u64>,
    pub word: String,
}

pub struct Request {
    pub id: u64,
    pub generation: u64,
    pub snapshot: Snapshot,
    /// Markdown: the syntax worker answers which text is not prose.
    pub mask: Option<MaskPort>,
    pub work: Work,
}

pub enum Work {
    /// Misspelled words of sorted, disjoint `ranges` (at most
    /// [`MAX_REQUEST_BYTES`] in total).
    Marks(Vec<Range<u64>>),
    /// Suggestions for a misspelled word.
    Suggest(Mark),
    /// The next (previous) misspelling from `origin`, wrapping once; the
    /// word at `origin` comes last.
    Navigate { origin: u64, backwards: bool },
}

impl Work {
    fn lane(&self) -> Lane {
        match self {
            Work::Suggest(_) => Lane::Suggest,
            Work::Marks(_) => Lane::Marks,
            Work::Navigate { .. } => Lane::Navigate,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Misspelled words of `covered`: the requested ranges, or a prefix of
    /// them when the bounds were reached.
    Marks {
        covered: Vec<Range<u64>>,
        marks: Vec<Mark>,
    },
    Suggestions(Vec<String>),
    Found(Mark),
    /// No other misspelling; `only` is the one at the origin, if any.
    Exhausted(Option<Mark>),
    /// Still scanning: bytes scanned of `total`.
    Progress {
        scanned: u64,
        total: u64,
    },
    /// The Markdown structure is not available (the syntax thread stopped).
    Unavailable,
}

#[derive(Debug)]
pub struct Reply {
    pub lane: Lane,
    pub id: u64,
    pub generation: u64,
    pub snapshot: Snapshot,
    pub outcome: Outcome,
}

#[derive(Debug)]
pub enum Event {
    Reply(Reply),
    /// A report for the status line; `reset`: verdicts changed (another
    /// engine), so earlier replies are out of date.
    Notice {
        text: String,
        reset: bool,
    },
}

enum Msg {
    Request(Request),
    Cancel(Lane),
    Add(String),
    Ignore(String),
    /// The syntax worker's answer for the request `id` of `lane`; `None`
    /// when the worker stopped without answering.
    Mask {
        lane: Lane,
        id: u64,
        answer: Option<MaskAnswer>,
    },
    /// The UI dropped its handle.
    Stop,
}

/// The UI's handle; dropping it stops the thread.
pub struct SpellThread {
    tx: Sender<Msg>,
    rx: Receiver<Event>,
}

impl SpellThread {
    /// Starts the thread; it creates the engine and reads the user
    /// dictionary before the first request. `wake` runs after every event.
    pub fn spawn(config: Config, wake: Wake) -> Result<SpellThread, String> {
        let (tx, jobs) = mpsc::channel();
        let (events, rx) = mpsc::channel();
        let to_self = tx.clone();
        thread::Builder::new()
            .name("spelling".to_owned())
            .spawn(move || Worker::start(config, events, wake, to_self).run(&jobs))
            .map_err(|e| format!("spelling thread: {e}"))?;
        Ok(SpellThread { tx, rx })
    }

    /// Queues `request`, replacing the pending one of its lane.
    pub fn send(&self, request: Request) {
        let _ = self.tx.send(Msg::Request(request));
    }

    /// Drops the pending request of `lane`.
    pub fn cancel(&self, lane: Lane) {
        let _ = self.tx.send(Msg::Cancel(lane));
    }

    /// Adds `word` to the user dictionary (and its file).
    pub fn add(&self, word: &str) {
        let _ = self.tx.send(Msg::Add(word.to_owned()));
    }

    /// Ignores `word` for the session.
    pub fn ignore(&self, word: &str) {
        let _ = self.tx.send(Msg::Ignore(word.to_owned()));
    }

    pub fn try_recv(&self) -> Option<Event> {
        self.rx.try_recv().ok()
    }
}

impl Drop for SpellThread {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Stop);
    }
}

/// The Windows Spell Checking API as an engine.
struct WindowsEngine(rte_platform::spell::WindowsSpeller);

impl Speller for WindowsEngine {
    fn name(&self) -> &'static str {
        "windows"
    }

    fn check(&mut self, word: &str) -> Result<bool, String> {
        self.0.check(word)
    }

    fn suggest(
        &mut self,
        word: &str,
        max: usize,
        _deadline: Instant,
    ) -> Result<Vec<String>, String> {
        self.0.suggest(word, max)
    }
}

/// The configured engine, or the built-in one and why.
fn create_engine(engine: Engine, hunspell: &std::path::Path) -> (Box<dyn Speller>, Option<String>) {
    let failed = |name: &str, e: String| {
        let text = format!(
            "Spelling: the {name} engine is not available ({e}); using the built-in word list"
        );
        (Box::new(Builtin::new()) as Box<dyn Speller>, Some(text))
    };
    match engine {
        Engine::Builtin => (Box::new(Builtin::new()), None),
        Engine::Hunspell => match Hunspell::load(hunspell) {
            Ok(h) => (Box::new(h), None),
            Err(e) => failed("Hunspell", e),
        },
        Engine::Windows => match rte_platform::spell::WindowsSpeller::new("en-US") {
            Ok(w) => (Box::new(WindowsEngine(w)), None),
            Err(e) => failed("Windows", e),
        },
    }
}

/// The words of the user dictionary file (none when it does not exist).
fn read_user(storage: &mut dyn Storage) -> Result<Vec<String>, String> {
    match storage.read(USER_MAX_BYTES as u64) {
        Ok(None) => Ok(Vec::new()),
        Ok(Some(bytes)) => parse_user(&bytes).ok_or_else(|| {
            format!("{USER_FILE} is not a valid word list of at most 64 KiB and 10,000 words")
        }),
        Err(e) => Err(format!("{USER_FILE} cannot be read ({e})")),
    }
}

/// Merges `added` into the user dictionary file: read, merge, write. Words
/// added by other sessions are kept; the file is never written when it is
/// invalid or the result would pass the limits.
pub fn persist(storage: &mut dyn Storage, added: &[String]) -> Result<(), String> {
    let merged = merge_user(read_user(storage)?, added);
    let bytes = encode_user(&merged).ok_or_else(|| {
        format!("{USER_FILE} would pass 64 KiB or 10,000 words; the word lasts for this session")
    })?;
    storage.write(&bytes).map_err(|e| {
        format!("{USER_FILE} cannot be written ({e}); the word lasts for this session")
    })
}

/// Widens `range` to whole tokens (whitespace boundaries), looking at most
/// [`BOUNDARY_SCAN`] bytes outward on each side; inside a longer token it
/// stops at a byte that cannot be part of a word.
fn widen(snapshot: &Snapshot, range: Range<u64>) -> Range<u64> {
    let len = snapshot.len();
    let start = range.start.min(len);
    let end = range.end.min(len).max(start);
    let split = |b: &u8| !b.is_ascii_alphanumeric() && *b != b'_' && *b != b'\'' && b.is_ascii();
    let before = snapshot.to_vec(start.saturating_sub(BOUNDARY_SCAN)..start);
    let back = |i: usize| start - (before.len() - i - 1) as u64;
    let start = match before.iter().rposition(u8::is_ascii_whitespace) {
        Some(i) => back(i),
        None if start <= BOUNDARY_SCAN => 0,
        None => before.iter().rposition(split).map_or(start, back),
    };
    let after = snapshot.to_vec(end..(end + BOUNDARY_SCAN).min(len));
    let end = match after.iter().position(u8::is_ascii_whitespace) {
        Some(i) => end + i as u64,
        None if end + BOUNDARY_SCAN >= len => len,
        None => after.iter().position(split).map_or(end, |i| end + i as u64),
    };
    start..end
}

/// The words of `range` (widened to whole tokens) outside the sorted
/// `excluded` ranges, which count as spaces (so `[word](url)` keeps
/// `word` once its destination is excluded). `bytes` receives the text
/// from the returned offset.
fn prose_words(
    snapshot: &Snapshot,
    range: Range<u64>,
    excluded: &[Range<u64>],
    bytes: &mut Vec<u8>,
    out: &mut Vec<Word>,
) -> u64 {
    let range = widen(snapshot, range);
    bytes.clear();
    for chunk in snapshot.chunks(range.clone()) {
        bytes.extend_from_slice(chunk);
    }
    let first = excluded.partition_point(|r| r.end <= range.start);
    for r in excluded[first..].iter().take_while(|r| r.start < range.end) {
        let from = r.start.max(range.start) - range.start;
        let to = r.end.min(range.end) - range.start;
        bytes[from as usize..to as usize].fill(b' ');
    }
    out.clear();
    words(bytes, range.start, out);
    range.start
}

/// Text of `word` in `bytes` (which start at offset `base`).
fn text_of<'a>(bytes: &'a [u8], base: u64, word: &Word) -> std::borrow::Cow<'a, str> {
    String::from_utf8_lossy(&bytes[(word.start - base) as usize..(word.end - base) as usize])
}

/// A request being served, and what it waits for.
struct Job {
    request: Request,
    /// Markdown: the range whose exclusions were asked for, and the answer.
    asked: Option<Range<u64>>,
    excluded: Option<Vec<Range<u64>>>,
    scan: Option<Scan>,
}

impl Job {
    fn new(request: Request) -> Job {
        let scan = match request.work {
            Work::Navigate { origin, backwards } => {
                Some(Scan::new(&request.snapshot, origin, backwards))
            }
            _ => None,
        };
        Job {
            request,
            asked: None,
            excluded: None,
            scan,
        }
    }

    /// Waits for the syntax worker's answer.
    fn waiting(&self) -> bool {
        self.asked.is_some() && self.excluded.is_none()
    }

    /// Exclusions of `range` when known: none for plain text.
    fn exclusions(&self, range: &Range<u64>) -> Option<&[Range<u64>]> {
        match (&self.request.mask, &self.asked, &self.excluded) {
            (None, ..) => Some(&[]),
            (Some(_), Some(asked), Some(excluded)) if asked == range => Some(excluded),
            _ => None,
        }
    }
}

/// Navigation progress: a cursor over the text, in slices.
struct Scan {
    origin: u64,
    backwards: bool,
    /// Forward: the next slice starts here; backwards: it ends here.
    pos: u64,
    wrapped: bool,
    /// Where the scan after the wrap stops.
    stop: u64,
    scanned: u64,
    started: Instant,
    reported: Instant,
}

impl Scan {
    fn new(snapshot: &Snapshot, origin: u64, backwards: bool) -> Scan {
        let origin = origin.min(snapshot.len());
        let caret = widen(snapshot, origin..origin);
        let now = Instant::now();
        Scan {
            origin,
            backwards,
            pos: if backwards { caret.end } else { caret.start },
            wrapped: false,
            stop: if backwards { caret.start } else { caret.end },
            scanned: 0,
            started: now,
            reported: now,
        }
    }

    /// The next slice; an empty one at the end of the text (backwards: at
    /// its start) means wrap there; `None` once the scan is complete.
    fn slice(&self, snapshot: &Snapshot) -> Option<Range<u64>> {
        let len = snapshot.len();
        if self.backwards {
            let limit = if self.wrapped { self.stop } else { 0 };
            if self.pos <= limit {
                return (!self.wrapped && len > 0).then_some(len..len);
            }
            let from = self.pos.saturating_sub(SLICE_BYTES).max(limit);
            Some(widen(snapshot, from..self.pos).start..self.pos)
        } else {
            let limit = if self.wrapped { self.stop } else { len };
            if self.pos >= limit {
                return (!self.wrapped && len > 0).then_some(0..0);
            }
            let to = (self.pos + SLICE_BYTES).min(limit);
            Some(self.pos..widen(snapshot, self.pos..to).end)
        }
    }

    /// Whether `word` is a candidate in the current phase.
    fn eligible(&self, word: &Word) -> bool {
        match (self.backwards, self.wrapped) {
            (false, false) => word.start > self.origin,
            (false, true) => word.start <= self.origin,
            (true, false) => word.end < self.origin,
            (true, true) => word.end >= self.origin,
        }
    }

    fn at_origin(&self, word: &Word) -> bool {
        word.start <= self.origin && self.origin <= word.end
    }
}

struct Worker {
    checker: Checker,
    events: Sender<Event>,
    wake: Wake,
    to_self: Sender<Msg>,
    storage: Option<Box<dyn Storage>>,
    /// The file was read and is valid: additions may be written.
    writable: bool,
    /// Words added this session.
    added: Vec<String>,
    lanes: [Option<Job>; 3],
    words: Vec<Word>,
    /// Text of the words (reused).
    text: Vec<u8>,
}

impl Worker {
    fn start(config: Config, events: Sender<Event>, wake: Wake, to_self: Sender<Msg>) -> Worker {
        let (engine, notice) = create_engine(config.engine, &config.hunspell);
        let mut worker = Worker {
            checker: Checker::new(engine),
            events,
            wake,
            to_self,
            storage: config.storage,
            writable: false,
            added: Vec::new(),
            lanes: [None, None, None],
            words: Vec::new(),
            text: Vec::new(),
        };
        if let Some(text) = notice {
            worker.notice(text, false);
        }
        if let Some(storage) = &mut worker.storage {
            match read_user(storage.as_mut()) {
                Ok(list) => {
                    worker.writable = true;
                    for word in &list {
                        worker.checker.add_user(word);
                    }
                }
                Err(e) => {
                    let text = format!(
                        "Spelling: {e}; it is left unchanged and added words last for this session"
                    );
                    worker.notice(text, false);
                }
            }
        }
        worker
    }

    fn notice(&self, text: String, reset: bool) {
        if self.events.send(Event::Notice { text, reset }).is_ok() {
            (self.wake)();
        }
    }

    fn reply(&self, job: &Job, outcome: Outcome) {
        let r = &job.request;
        let reply = Reply {
            lane: r.work.lane(),
            id: r.id,
            generation: r.generation,
            snapshot: r.snapshot.clone(),
            outcome,
        };
        if self.events.send(Event::Reply(reply)).is_ok() {
            (self.wake)();
        }
    }

    fn run(mut self, rx: &Receiver<Msg>) {
        loop {
            let ready = (0..3).find(|&l| self.lanes[l].as_ref().is_some_and(|j| !j.waiting()));
            let msg = if ready.is_some() {
                match rx.try_recv() {
                    Ok(msg) => Some(msg),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => return,
                }
            } else {
                match rx.recv() {
                    Ok(msg) => Some(msg),
                    Err(_) => return,
                }
            };
            match (msg, ready) {
                (Some(Msg::Stop), _) => return,
                (Some(msg), _) => self.handle(msg),
                (None, Some(lane)) => self.step(lane),
                (None, None) => {}
            }
        }
    }

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Request(request) => {
                let lane = request.work.lane() as usize;
                self.lanes[lane] = Some(Job::new(request));
            }
            Msg::Cancel(lane) => self.lanes[lane as usize] = None,
            Msg::Add(word) => {
                self.checker.add_user(&word);
                if !self.added.contains(&word) {
                    self.added.push(word);
                }
                if let (Some(storage), true) = (&mut self.storage, self.writable)
                    && let Err(e) = persist(storage.as_mut(), &self.added)
                {
                    self.notice(format!("Spelling: {e}"), false);
                }
            }
            Msg::Ignore(word) => self.checker.ignore(&word),
            Msg::Stop => {}
            Msg::Mask { lane, id, answer } => {
                let Some(job) = &mut self.lanes[lane as usize] else {
                    return;
                };
                if job.request.id != id || !job.waiting() {
                    return;
                }
                match answer {
                    Some(MaskAnswer::Verified(excluded)) => job.excluded = Some(excluded),
                    // Text unknown to the syntax worker or no worker: no
                    // verified structure, no answer.
                    Some(MaskAnswer::Stale) | None => {
                        let job = self.lanes[lane as usize].take().expect("job");
                        self.reply(&job, Outcome::Unavailable);
                    }
                }
            }
        }
    }

    /// Asks the syntax worker for the exclusions of `range`; false when it
    /// stopped (the job is answered `Unavailable`).
    fn ask_mask(&mut self, lane: usize, range: Range<u64>) -> bool {
        let job = self.lanes[lane].as_mut().expect("job");
        let port = job.request.mask.clone().expect("Markdown job");
        job.asked = Some(range.clone());
        job.excluded = None;
        let guard = MaskGuard {
            tx: self.to_self.clone(),
            lane: job.request.work.lane(),
            id: job.request.id,
            sent: false,
        };
        let sent = port.request(MaskRequest {
            snapshot: job.request.snapshot.clone(),
            ranges: vec![range],
            reply: Box::new(move |answer| guard.answer(answer)),
        });
        if !sent {
            let job = self.lanes[lane].take().expect("job");
            self.reply(&job, Outcome::Unavailable);
        }
        sent
    }

    /// Serves `lane` one step; an engine error switches to the built-in
    /// engine and drops all pending work (the UI asks again).
    fn step(&mut self, lane: usize) {
        let result = match self.lanes[lane].as_ref().map(|j| j.request.work.lane()) {
            Some(Lane::Suggest) => self.suggest(),
            Some(Lane::Marks) => self.marks(),
            Some(Lane::Navigate) => self.navigate(),
            None => Ok(()),
        };
        if let Err(e) = result {
            let name = self.checker.engine();
            self.checker.set_engine(Box::new(Builtin::new()));
            self.lanes = [None, None, None];
            let text =
                format!("Spelling: the {name} engine failed ({e}); using the built-in word list");
            self.notice(text, true);
        }
    }

    fn suggest(&mut self) -> Result<(), String> {
        let job = self.lanes[Lane::Suggest as usize].take().expect("job");
        let Work::Suggest(mark) = &job.request.work else {
            return Ok(());
        };
        let deadline = Instant::now() + SUGGEST_TIME;
        let list = self
            .checker
            .suggest(&mark.word, MAX_SUGGESTIONS, deadline)?;
        self.reply(&job, Outcome::Suggestions(list));
        Ok(())
    }

    fn marks(&mut self) -> Result<(), String> {
        let lane = Lane::Marks as usize;
        let job = self.lanes[lane].as_ref().expect("job");
        let Work::Marks(ranges) = &job.request.work else {
            return Ok(());
        };
        let hull = match (ranges.first(), ranges.last()) {
            (Some(first), Some(last)) => widen(&job.request.snapshot, first.start..last.end),
            _ => 0..0,
        };
        if job.request.mask.is_some() && job.exclusions(&hull).is_none() {
            self.ask_mask(lane, hull);
            return Ok(());
        }
        let job = self.lanes[lane].take().expect("job");
        let Work::Marks(ranges) = &job.request.work else {
            return Ok(());
        };
        let excluded = job.exclusions(&hull).unwrap_or_default();
        let snapshot = &job.request.snapshot;
        let mut covered = Vec::new();
        let mut marks = Vec::new();
        let mut checked = 0;
        'ranges: for range in ranges {
            let base = prose_words(
                snapshot,
                range.clone(),
                excluded,
                &mut self.text,
                &mut self.words,
            );
            for word in &self.words {
                if checked == MAX_REQUEST_WORDS || marks.len() == MAX_MARKS {
                    let end = word.start.max(range.start);
                    if end > range.start {
                        covered.push(range.start..end);
                    }
                    break 'ranges;
                }
                checked += 1;
                let text = text_of(&self.text, base, word);
                if !self.checker.is_correct(&text)? {
                    marks.push(Mark {
                        range: word.start..word.end,
                        word: text.into_owned(),
                    });
                }
            }
            covered.push(range.clone());
        }
        self.reply(&job, Outcome::Marks { covered, marks });
        Ok(())
    }

    fn navigate(&mut self) -> Result<(), String> {
        let lane = Lane::Navigate as usize;
        let job = self.lanes[lane].as_ref().expect("job");
        let snapshot = job.request.snapshot.clone();
        let scan = job.scan.as_ref().expect("scan");
        let Some(slice) = scan.slice(&snapshot) else {
            let job = self.lanes[lane].take().expect("job");
            self.reply(&job, Outcome::Exhausted(None));
            return Ok(());
        };
        if slice.is_empty() {
            // The end of the text: wrap.
            let scan = self.lanes[lane]
                .as_mut()
                .and_then(|j| j.scan.as_mut())
                .expect("scan");
            scan.wrapped = true;
            scan.pos = slice.start;
            return Ok(());
        }
        if job.request.mask.is_some() && job.exclusions(&slice).is_none() {
            self.ask_mask(lane, slice);
            return Ok(());
        }
        let excluded = job.exclusions(&slice).unwrap_or_default().to_vec();
        let base = prose_words(
            &snapshot,
            slice.clone(),
            &excluded,
            &mut self.text,
            &mut self.words,
        );
        let deadline = Instant::now() + SLICE_TIME;
        let job = self.lanes[lane].as_mut().expect("job");
        let scan = job.scan.as_mut().expect("scan");
        let backwards = scan.backwards;
        let n = self.words.len();
        let mut found = None;
        let mut resume = None;
        for k in 0..n {
            let word = self.words[if backwards { n - 1 - k } else { k }];
            if k > 0 && k % SLICE_CHECK == 0 && Instant::now() > deadline {
                resume = Some(if backwards { word.end } else { word.start });
                break;
            }
            if !scan.eligible(&word) {
                continue;
            }
            let text = text_of(&self.text, base, &word);
            if !self.checker.is_correct(&text)? {
                found = Some((word, text.into_owned()));
                break;
            }
        }
        if let Some((word, text)) = found {
            let at_origin = scan.wrapped && scan.at_origin(&word);
            let job = self.lanes[lane].take().expect("job");
            let mark = Mark {
                range: word.start..word.end,
                word: text,
            };
            let outcome = if at_origin {
                Outcome::Exhausted(Some(mark))
            } else {
                Outcome::Found(mark)
            };
            self.reply(&job, outcome);
            return Ok(());
        }
        let next = match resume {
            Some(pos) => pos,
            None if backwards => slice.start,
            None => slice.end,
        };
        scan.scanned += next.abs_diff(scan.pos);
        scan.pos = next;
        let now = Instant::now();
        let report = now.duration_since(scan.started) >= PROGRESS_EVERY
            && now.duration_since(scan.reported) >= PROGRESS_EVERY;
        if report {
            scan.reported = now;
            let outcome = Outcome::Progress {
                scanned: scan.scanned,
                total: snapshot.len(),
            };
            let job = self.lanes[lane].as_ref().expect("job");
            self.reply(job, outcome);
        }
        Ok(())
    }
}

/// Sends the syntax worker's answer back to the spelling thread; dropped
/// unanswered (the worker stopped), it reports that.
struct MaskGuard {
    tx: Sender<Msg>,
    lane: Lane,
    id: u64,
    sent: bool,
}

impl MaskGuard {
    fn answer(mut self, answer: MaskAnswer) {
        self.sent = true;
        let _ = self.tx.send(Msg::Mask {
            lane: self.lane,
            id: self.id,
            answer: Some(answer),
        });
    }
}

impl Drop for MaskGuard {
    fn drop(&mut self) {
        if !self.sent {
            let _ = self.tx.send(Msg::Mask {
                lane: self.lane,
                id: self.id,
                answer: None,
            });
        }
    }
}

#[cfg(test)]
#[allow(clippy::single_range_in_vec_init)]
mod tests;
