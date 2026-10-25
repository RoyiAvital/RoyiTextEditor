//! The search threads, started on first use. The search thread compiles
//! patterns and runs every search except bounded literal scans on the UI
//! thread. Counts run on their own thread as one exact enumeration, so
//! viewport and navigation requests are answered while a count runs. Each
//! request belongs to a lane with a generation; a newer generation cancels
//! the running scan and drops queued work.

use std::collections::VecDeque;
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use rte_search::{Expansion, MatchIter, Matcher, Query, Replacement, Searcher};
use rte_text::{Edit, Snapshot};

/// Least time between count progress reports.
const REPORT_EVERY: Duration = Duration::from_millis(50);
/// Matches counted between progress checks.
const REPORT_CHECK: u64 = 1024;
/// Index checkpoints are recorded every this many matches or bytes at
/// first; both spacings double to keep at most [`MAX_CHECKPOINTS`].
const CHECKPOINT_EVERY: u64 = 1024;
const CHECKPOINT_BYTES: u64 = 4 * 1024 * 1024;
const MAX_CHECKPOINTS: usize = 4096;
/// Heap bytes of one Replace All text allocation besides its bytes.
const ALLOC_OVERHEAD: u64 = 16;
/// Highlight ranges a viewport request returns at most.
pub const MAX_VIEW_MATCHES: usize = 64 * 1024;

/// Independent request streams: the search box, and one-off operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lane {
    Find = 0,
    Op = 1,
}

pub struct Job {
    pub lane: Lane,
    pub generation: u64,
    pub snapshot: Snapshot,
    pub query: Query,
    pub work: Work,
}

pub enum Work {
    Count {
        scope: Range<u64>,
    },
    Viewport {
        /// Sorted text ranges of the visible rows.
        rows: Vec<Range<u64>>,
    },
    /// Next match from `from` (previous with `backwards`), wrapping in `scope`.
    Find {
        from: u64,
        scope: Range<u64>,
        backwards: bool,
    },
    /// Position of the match starting at `at` among all matches of `scope`.
    Index {
        at: u64,
        scope: Range<u64>,
    },
    SelectAll {
        scope: Range<u64>,
        limit: usize,
    },
    ReplaceOne {
        at: Range<u64>,
        /// The search scope: the match is verified inside it.
        scope: Range<u64>,
        replacement: Replacement,
        preserve: bool,
    },
    ReplaceAll {
        scope: Range<u64>,
        replacement: Replacement,
        preserve: bool,
        cap: u64,
    },
}

#[derive(Debug)]
pub struct Reply {
    pub lane: Lane,
    pub generation: u64,
    /// The text the outcome refers to.
    pub snapshot: Snapshot,
    pub outcome: Outcome,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The query does not compile.
    Invalid(String),
    Count {
        count: u64,
        done: bool,
        /// Lines the backtracking engine skipped or found too complex.
        skipped: u64,
        complex: u64,
    },
    Viewport {
        rows: Vec<Range<u64>>,
        matches: Vec<Range<u64>>,
    },
    /// Zero-based index when known.
    Found {
        found: Option<Range<u64>>,
        index: Option<u64>,
    },
    /// Zero-based index of the match starting at `at`, when known.
    Index {
        at: u64,
        index: Option<u64>,
    },
    Selected(Result<Vec<Range<u64>>, String>),
    /// Replacement text of `at`, or `None` when `at` is not a match.
    Replaced {
        at: Range<u64>,
        text: Option<Vec<u8>>,
    },
    ReplacedAll(Result<Vec<Edit>, String>),
}

pub struct SearchThread {
    tx: Sender<Job>,
    counts: Sender<Job>,
    rx: Receiver<Reply>,
    gens: Arc<[AtomicU64; 2]>,
    checkpoints: SharedCheckpoints,
}

impl SearchThread {
    /// Starts the search and count threads; `wake` runs after every reply.
    pub fn spawn(wake: impl Fn() + Send + Sync + 'static) -> SearchThread {
        let (tx, jobs) = mpsc::channel();
        let (counts, count_jobs) = mpsc::channel();
        let (replies, rx) = mpsc::channel();
        let gens: Arc<[AtomicU64; 2]> = Arc::new([AtomicU64::new(0), AtomicU64::new(0)]);
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
        let checkpoints = Arc::new(Mutex::new(None));
        let worker = Worker {
            gens: gens.clone(),
            replies: replies.clone(),
            wake: wake.clone(),
            compiled: None,
            checkpoints: checkpoints.clone(),
        };
        thread::Builder::new()
            .name("search".to_owned())
            .spawn(move || worker.run(&jobs))
            .expect("search thread");
        let counter = Worker {
            gens: gens.clone(),
            replies,
            wake,
            compiled: None,
            checkpoints: checkpoints.clone(),
        };
        thread::Builder::new()
            .name("search-count".to_owned())
            .spawn(move || counter.run_counts(&count_jobs))
            .expect("count thread");
        SearchThread {
            tx,
            counts,
            rx,
            gens,
            checkpoints,
        }
    }

    /// Frees the count checkpoints (and the text they hold). Call after
    /// cancelling the find lane: a count of an older generation then never
    /// records new ones.
    pub fn release(&self) {
        *self.checkpoints.lock().expect("checkpoints") = None;
    }

    /// The count checkpoints hold a text.
    #[cfg(test)]
    pub fn holds_checkpoints(&self) -> bool {
        self.checkpoints.lock().expect("checkpoints").is_some()
    }

    /// Queues `job`; it cancels older generations of its lane.
    pub fn send(&self, job: Job) {
        self.gens[job.lane as usize].fetch_max(job.generation, Ordering::Relaxed);
        let _ = match job.work {
            Work::Count { .. } => self.counts.send(job),
            _ => self.tx.send(job),
        };
    }

    /// Cancels the running and queued work of `lane` older than `generation`.
    pub fn cancel(&self, lane: Lane, generation: u64) {
        self.gens[lane as usize].fetch_max(generation, Ordering::Relaxed);
    }

    pub fn try_recv(&self) -> Option<Reply> {
        self.rx.try_recv().ok()
    }
}

/// Index checkpoints of the latest finished or running count: canonical
/// match starts, so a forward search from one continues the count's order.
struct Checkpoints {
    generation: u64,
    snapshot: Snapshot,
    query: Query,
    scope_start: u64,
    /// (match start, matches before it).
    points: Vec<(u64, u64)>,
    every: u64,
    bytes: u64,
    /// Counted up to here.
    pos: u64,
}

impl Checkpoints {
    fn new(generation: u64, snapshot: &Snapshot, query: &Query, scope_start: u64) -> Self {
        Checkpoints {
            generation,
            snapshot: snapshot.clone(),
            query: query.clone(),
            scope_start,
            points: Vec::new(),
            every: CHECKPOINT_EVERY,
            bytes: CHECKPOINT_BYTES,
            pos: scope_start,
        }
    }

    fn of(&self, generation: u64, snapshot: &Snapshot, query: &Query) -> bool {
        self.generation == generation && self.snapshot.same_tree(snapshot) && self.query == *query
    }

    /// Records the match at `start` with `before` matches before it when it
    /// is far enough from the last point.
    fn push(&mut self, start: u64, before: u64) {
        let (last_start, last_n) = self.points.last().copied().unwrap_or((self.scope_start, 0));
        if before - last_n < self.every && start - last_start < self.bytes {
            return;
        }
        if self.points.len() == MAX_CHECKPOINTS {
            self.every *= 2;
            self.bytes *= 2;
            let mut i = 0;
            self.points.retain(|_| {
                i += 1;
                i % 2 == 0
            });
        }
        self.points.push((start, before));
    }

    /// The last point at or before `pos`, else the scope start.
    fn before(&self, pos: u64) -> (u64, u64) {
        let i = self.points.partition_point(|&(start, _)| start <= pos);
        if i == 0 {
            (self.scope_start, 0)
        } else {
            self.points[i - 1]
        }
    }
}

type SharedCheckpoints = Arc<Mutex<Option<Checkpoints>>>;

/// A compiled query: `None` for an empty query, or the compile error.
type Compiled = Result<Option<Arc<Matcher>>, String>;

/// The search thread or the count thread.
struct Worker {
    gens: Arc<[AtomicU64; 2]>,
    replies: Sender<Reply>,
    wake: Arc<dyn Fn() + Send + Sync>,
    compiled: Option<(Query, Compiled)>,
    checkpoints: SharedCheckpoints,
}

impl Worker {
    fn current(&self, lane: Lane, generation: u64) -> bool {
        self.gens[lane as usize].load(Ordering::Relaxed) == generation
    }

    fn run(mut self, jobs: &Receiver<Job>) {
        let mut queue: VecDeque<Job> = VecDeque::new();
        loop {
            if queue.is_empty() {
                match jobs.recv() {
                    Ok(job) => queue.push_back(job),
                    Err(_) => return,
                }
            }
            loop {
                match jobs.try_recv() {
                    Ok(job) => queue.push_back(job),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return,
                }
            }
            queue.retain(|j| self.current(j.lane, j.generation));
            if let Some(job) = queue.pop_front() {
                self.run_job(job);
            }
        }
    }

    /// Runs the newest current count, one at a time.
    fn run_counts(mut self, jobs: &Receiver<Job>) {
        while let Ok(mut job) = jobs.recv() {
            while let Ok(newer) = jobs.try_recv() {
                job = newer;
            }
            if self.current(job.lane, job.generation) {
                self.count(job);
            }
        }
    }

    fn matcher(&mut self, query: &Query) -> Compiled {
        if let Some((q, result)) = &self.compiled
            && q == query
        {
            return result.clone();
        }
        let result = Matcher::new(query).map(|m| m.map(Arc::new));
        self.compiled = Some((query.clone(), result.clone()));
        result
    }

    fn reply(&self, lane: Lane, generation: u64, snapshot: &Snapshot, outcome: Outcome) {
        let _ = self.replies.send(Reply {
            lane,
            generation,
            snapshot: snapshot.clone(),
            outcome,
        });
        (self.wake)();
    }

    /// Runs `job` and replies unless it became stale.
    fn run_job(&mut self, job: Job) {
        let Job {
            lane,
            generation,
            snapshot,
            query,
            work,
        } = job;
        let matcher = match self.matcher(&query) {
            Ok(m) => m,
            Err(error) => {
                self.reply(lane, generation, &snapshot, Outcome::Invalid(error));
                return;
            }
        };
        let gens = self.gens.clone();
        let stop = move || gens[lane as usize].load(Ordering::Relaxed) != generation;
        let outcome = match matcher {
            None => empty_outcome(&work),
            Some(m) => {
                let mut s = Searcher::new(&m, &snapshot, &stop);
                let outcome = self.work(&mut s, &snapshot, &query, generation, work);
                if s.stopped() {
                    return;
                }
                outcome
            }
        };
        self.reply(lane, generation, &snapshot, outcome);
    }

    fn work(
        &self,
        s: &mut Searcher<'_>,
        snapshot: &Snapshot,
        query: &Query,
        generation: u64,
        work: Work,
    ) -> Outcome {
        match work {
            Work::Count { .. } => unreachable!("counts run on the count thread"),
            Work::Viewport { rows } => {
                let matches = s.matches_in(&rows, MAX_VIEW_MATCHES);
                Outcome::Viewport { rows, matches }
            }
            Work::Find {
                from,
                scope,
                backwards,
            } => {
                let found = if from > scope.end && !backwards {
                    s.find(scope.start, scope.end)
                } else if backwards {
                    let from = from.clamp(scope.start, scope.end);
                    let anchor = self.anchor(snapshot, query, generation, from, &scope);
                    s.find_prev(from, anchor).or_else(|| {
                        let anchor = self.anchor(snapshot, query, generation, scope.end, &scope);
                        s.find_prev(scope.end, anchor)
                    })
                } else {
                    let from = from.max(scope.start);
                    s.find(from, scope.end)
                        .or_else(|| s.find(scope.start, scope.end))
                };
                let index = found.as_ref().and_then(|m| {
                    self.index_from_checkpoints(s, snapshot, query, generation, m.start)
                });
                Outcome::Found { found, index }
            }
            Work::Index { at, scope } => Outcome::Index {
                at,
                index: self
                    .index_from_checkpoints(s, snapshot, query, generation, at)
                    .or_else(|| count_before(s, scope.start, at)),
            },
            Work::SelectAll { scope, limit } => {
                let mut found = Vec::new();
                let mut over = false;
                s.for_each(scope, |m| {
                    over = found.len() >= limit;
                    if !over {
                        found.push(m);
                    }
                    !over
                });
                Outcome::Selected(if over {
                    Err(format!("More than {limit} matches; nothing was selected"))
                } else {
                    Ok(found)
                })
            }
            Work::ReplaceOne {
                at,
                scope,
                replacement,
                preserve,
            } => {
                s.set_end(scope.end);
                let text = (at.start >= scope.start)
                    .then(|| s.expand(&replacement, at.clone(), preserve))
                    .flatten();
                Outcome::Replaced { at, text }
            }
            Work::ReplaceAll {
                scope,
                replacement,
                preserve,
                cap,
            } => {
                s.set_end(scope.end);
                Outcome::ReplacedAll(replace_all(s, scope, &replacement, preserve, cap))
            }
        }
    }

    /// A canonical start for a backward search to `pos`: the last checkpoint
    /// of the same count at or before it, else the scope start.
    fn anchor(
        &self,
        snapshot: &Snapshot,
        query: &Query,
        generation: u64,
        pos: u64,
        scope: &Range<u64>,
    ) -> u64 {
        let cp = self.checkpoints.lock().expect("checkpoints");
        match cp.as_ref() {
            Some(cp) if cp.of(generation, snapshot, query) && cp.scope_start == scope.start => {
                cp.before(pos).0
            }
            _ => scope.start,
        }
    }

    /// Index of the match at `at` from the checkpoints of the same count.
    fn index_from_checkpoints(
        &self,
        s: &mut Searcher<'_>,
        snapshot: &Snapshot,
        query: &Query,
        generation: u64,
        at: u64,
    ) -> Option<u64> {
        let (start, before) = {
            let cp = self.checkpoints.lock().expect("checkpoints");
            let cp = cp.as_ref()?;
            if !cp.of(generation, snapshot, query) || at >= cp.pos {
                return None;
            }
            cp.before(at)
        };
        count_before(s, start, at).map(|n| before + n)
    }

    /// Counts all matches of the job's scope in one forward enumeration,
    /// reporting progress; stops silently when the job becomes stale.
    fn count(&mut self, job: Job) {
        let Job {
            lane,
            generation,
            snapshot,
            query,
            work: Work::Count { scope },
        } = job
        else {
            unreachable!("only counts reach the count thread");
        };
        let matcher = match self.matcher(&query) {
            Ok(Some(m)) => m,
            Ok(None) => {
                self.reply(lane, generation, &snapshot, count_outcome(0, true, 0, 0));
                return;
            }
            Err(error) => {
                self.reply(lane, generation, &snapshot, Outcome::Invalid(error));
                return;
            }
        };
        *self.checkpoints.lock().expect("checkpoints") =
            Some(Checkpoints::new(generation, &snapshot, &query, scope.start));
        // Cancelled meanwhile: `release` may have run before the line above.
        if !self.current(lane, generation) {
            let mut cp = self.checkpoints.lock().expect("checkpoints");
            if cp.as_ref().is_some_and(|c| c.generation == generation) {
                *cp = None;
            }
            return;
        }
        let gens = self.gens.clone();
        let stop = move || gens[lane as usize].load(Ordering::Relaxed) != generation;
        let mut s = Searcher::new(&matcher, &snapshot, &stop);
        s.set_end(scope.end);
        let mut it = MatchIter::new(scope.clone());
        let mut count = 0u64;
        let mut reported = Instant::now();
        // The last point and the spacing, read back after each recorded point.
        let (mut last, mut every, mut bytes) =
            ((scope.start, 0), CHECKPOINT_EVERY, CHECKPOINT_BYTES);
        loop {
            let m = s.next_match(&mut it);
            if s.stopped() {
                return;
            }
            let done = m.is_none();
            if let Some(m) = &m {
                if count - last.1 >= every || m.start - last.0 >= bytes {
                    if let Some(cp) = self.checkpoints.lock().expect("checkpoints").as_mut()
                        && cp.of(generation, &snapshot, &query)
                    {
                        cp.push(m.start, count);
                        (every, bytes) = (cp.every, cp.bytes);
                    }
                    last = (m.start, count);
                }
                count += 1;
            }
            if !(done || count.is_multiple_of(REPORT_CHECK)) {
                continue;
            }
            if let Some(cp) = self.checkpoints.lock().expect("checkpoints").as_mut()
                && cp.of(generation, &snapshot, &query)
            {
                cp.pos = if done { scope.end } else { it.from };
            }
            if done || reported.elapsed() >= REPORT_EVERY {
                reported = Instant::now();
                let outcome = count_outcome(count, done, s.skipped_lines, s.complex_lines);
                self.reply(lane, generation, &snapshot, outcome);
            }
            if done {
                return;
            }
        }
    }
}

/// Replace All edits for `scope`, built while matching: every allocation of
/// the result, and the expansion of each match, is counted against `cap`
/// before it is made, and no partial result is returned.
fn replace_all(
    s: &mut Searcher<'_>,
    scope: Range<u64>,
    replacement: &Replacement,
    preserve: bool,
    cap: u64,
) -> Result<Vec<Edit>, String> {
    let over = || {
        format!(
            "Replace All would need more than {} MiB; nothing was replaced",
            cap / (1024 * 1024)
        )
    };
    let edit_size = size_of::<Edit>() as u64;
    let mut edits: Vec<Edit> = Vec::new();
    let mut text_bytes = 0u64;
    let mut it = MatchIter::new(scope);
    while let Some(m) = s.next_match(&mut it) {
        let capacity = if edits.len() == edits.capacity() {
            (edits.capacity() * 2).max(64)
        } else {
            edits.capacity()
        };
        let used = capacity as u64 * edit_size + text_bytes + ALLOC_OVERHEAD;
        let Some(budget) = cap.checked_sub(used) else {
            return Err(over());
        };
        let text = match s.expand_within(replacement, m.clone(), preserve, budget) {
            Expansion::Text(text) => text,
            Expansion::OverBudget => return Err(over()),
            Expansion::NotAMatch => {
                return Err("A match could not be expanded; nothing was replaced".to_owned());
            }
        };
        if !text.is_empty() {
            text_bytes += text.len() as u64 + ALLOC_OVERHEAD;
        }
        edits.reserve_exact(capacity - edits.len());
        edits.push(Edit::new(m, text));
    }
    Ok(edits)
}

fn count_outcome(count: u64, done: bool, skipped: u64, complex: u64) -> Outcome {
    Outcome::Count {
        count,
        done,
        skipped,
        complex,
    }
}

/// Outcome of a job for an empty query (no matches).
fn empty_outcome(work: &Work) -> Outcome {
    match work {
        Work::Count { .. } => Outcome::Count {
            count: 0,
            done: true,
            skipped: 0,
            complex: 0,
        },
        Work::Viewport { rows } => Outcome::Viewport {
            rows: rows.clone(),
            matches: Vec::new(),
        },
        Work::Find { .. } => Outcome::Found {
            found: None,
            index: None,
        },
        Work::Index { at, .. } => Outcome::Index {
            at: *at,
            index: None,
        },
        Work::SelectAll { .. } => Outcome::Selected(Ok(Vec::new())),
        Work::ReplaceOne { at, .. } => Outcome::Replaced {
            at: at.clone(),
            text: None,
        },
        Work::ReplaceAll { .. } => Outcome::ReplacedAll(Ok(Vec::new())),
    }
}

/// Matches starting in `from..at`, when the match at `at` is reached.
fn count_before(s: &mut Searcher<'_>, from: u64, at: u64) -> Option<u64> {
    let mut n = 0;
    let mut reached = false;
    s.for_each(from..u64::MAX, |m| {
        if m.start >= at {
            reached = m.start == at;
            return false;
        }
        n += 1;
        true
    });
    reached.then_some(n)
}

#[cfg(test)]
#[allow(clippy::single_range_in_vec_init)]
mod tests {
    use super::*;
    use rte_search::Mode;

    fn query(text: &str, mode: Mode) -> Query {
        Query {
            text: text.to_owned(),
            mode,
            match_case: true,
            whole_word: false,
        }
    }

    fn wait(t: &SearchThread) -> Reply {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(r) = t.try_recv() {
                return r;
            }
            assert!(Instant::now() < deadline, "no reply");
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn job(generation: u64, snapshot: &Snapshot, q: Query, work: Work) -> Job {
        Job {
            lane: Lane::Find,
            generation,
            snapshot: snapshot.clone(),
            query: q,
            work,
        }
    }

    #[test]
    fn counts_finds_and_indexes() {
        let t = SearchThread::spawn(|| {});
        let text = "cat\n".repeat(3_000_000);
        let s = Snapshot::from_bytes(text.as_bytes());
        let len = s.len();
        t.send(job(
            1,
            &s,
            query("cat", Mode::Text),
            Work::Count { scope: 0..len },
        ));
        let total = loop {
            let r = wait(&t);
            match r.outcome {
                Outcome::Count { count, done, .. } if done => break count,
                Outcome::Count { .. } => {}
                other => panic!("{other:?}"),
            }
        };
        assert_eq!(total, 3_000_000);
        let at = 4 * 2_500_000;
        t.send(job(
            1,
            &s,
            query("cat", Mode::Text),
            Work::Find {
                from: at - 1,
                scope: 0..len,
                backwards: false,
            },
        ));
        assert_eq!(
            wait(&t).outcome,
            Outcome::Found {
                found: Some(at..at + 3),
                index: Some(2_500_000)
            }
        );
        t.send(job(
            1,
            &s,
            query("cat", Mode::Text),
            Work::Find {
                from: len,
                scope: 0..len,
                backwards: false,
            },
        ));
        assert!(matches!(
            wait(&t).outcome,
            Outcome::Found { found: Some(r), .. } if r == (0..3)
        ));
    }

    #[test]
    fn invalid_patterns_newer_generations_and_limits() {
        let t = SearchThread::spawn(|| {});
        let s = Snapshot::from_bytes(b"a1 a2 a3 b4");
        let len = s.len();
        t.send(job(
            1,
            &s,
            query("(", Mode::Regex),
            Work::Viewport { rows: vec![0..len] },
        ));
        assert!(matches!(wait(&t).outcome, Outcome::Invalid(_)));
        t.cancel(Lane::Find, 3);
        t.send(job(
            2,
            &s,
            query("a", Mode::Text),
            Work::Viewport { rows: vec![0..len] },
        ));
        t.send(job(
            3,
            &s,
            query(r"a\d", Mode::Regex),
            Work::Viewport {
                rows: vec![0..4, 4..len],
            },
        ));
        let r = wait(&t);
        assert_eq!(r.generation, 3);
        assert_eq!(
            r.outcome,
            Outcome::Viewport {
                rows: vec![0..4, 4..len],
                matches: vec![0..2, 3..5, 6..8]
            }
        );
        t.send(job(
            4,
            &s,
            query("a", Mode::Text),
            Work::SelectAll {
                scope: 0..len,
                limit: 2,
            },
        ));
        assert!(matches!(wait(&t).outcome, Outcome::Selected(Err(_))));
    }

    #[test]
    fn replace_one_and_all() {
        let t = SearchThread::spawn(|| {});
        let s = Snapshot::from_bytes(b"x_cfg y_cfg");
        let len = s.len();
        let q = query(r"(\w)_cfg", Mode::Regex);
        let rep = Replacement::parse("$1_conf");
        t.send(job(
            1,
            &s,
            q.clone(),
            Work::ReplaceOne {
                at: 6..11,
                scope: 0..len,
                replacement: rep.clone(),
                preserve: false,
            },
        ));
        assert_eq!(
            wait(&t).outcome,
            Outcome::Replaced {
                at: 6..11,
                text: Some(b"y_conf".to_vec())
            }
        );
        t.send(job(
            2,
            &s,
            q.clone(),
            Work::ReplaceAll {
                scope: 0..len,
                replacement: rep.clone(),
                preserve: false,
                cap: 1 << 20,
            },
        ));
        match wait(&t).outcome {
            Outcome::ReplacedAll(Ok(edits)) => {
                let (next, _) = s.apply(&rte_text::Transaction::new(edits)).unwrap();
                assert_eq!(next.to_vec(0..next.len()), b"x_conf y_conf");
            }
            other => panic!("{other:?}"),
        }
        t.send(job(
            3,
            &s,
            q,
            Work::ReplaceAll {
                scope: 0..len,
                replacement: rep,
                preserve: false,
                cap: 10,
            },
        ));
        assert!(matches!(wait(&t).outcome, Outcome::ReplacedAll(Err(_))));
    }

    fn count_of(t: &SearchThread, generation: u64, s: &Snapshot, q: Query) -> (u64, usize) {
        t.send(job(generation, s, q, Work::Count { scope: 0..s.len() }));
        let mut reports = 0;
        loop {
            match wait(t).outcome {
                Outcome::Count { count, done, .. } if done => return (count, reports),
                Outcome::Count { .. } => reports += 1,
                other => panic!("{other:?}"),
            }
        }
    }

    #[test]
    fn counts_are_exact_across_long_inputs() {
        let t = SearchThread::spawn(|| {});
        let mut text = "x".repeat((4 << 20) - 1);
        text.push_str("a\nb\n");
        text.push_str(&"y\n".repeat(1 << 19));
        let s = Snapshot::from_bytes(text.as_bytes());
        assert_eq!(count_of(&t, 1, &s, query(r"a\nb", Mode::Regex)).0, 1);
        let lines = text.matches('\n').count() as u64 + 1;
        assert_eq!(count_of(&t, 2, &s, query("^", Mode::Regex)).0, lines);
        // Anchors and alternative priority see the whole text.
        let mut long = vec![b'a'; 6 << 20];
        long.push(b'Z');
        let long = Snapshot::from_bytes(&long);
        assert_eq!(count_of(&t, 3, &long, query(r"\Aa+Z", Mode::Regex)).0, 1);
        assert_eq!(count_of(&t, 4, &long, query("a+Z|a", Mode::Regex)).0, 1);
        let mut bytes = vec![b'a'; 6 << 20];
        bytes[1024] = b'X';
        bytes.push(b'Z');
        let mixed = Snapshot::from_bytes(&bytes);
        assert_eq!(count_of(&t, 5, &mixed, query("a.*Z|X", Mode::Regex)).0, 1);
    }

    #[test]
    fn dense_counts_run_beside_navigation_and_keep_bounded_checkpoints() {
        let t = SearchThread::spawn(|| {});
        let size: u64 = 5 << 20;
        let s = Snapshot::from_bytes(&vec![b'a'; size as usize]);
        let q = query("a", Mode::Text);
        t.send(job(1, &s, q.clone(), Work::Count { scope: 0..s.len() }));
        // A navigation sent after the count is answered while it runs.
        t.send(job(
            1,
            &s,
            q.clone(),
            Work::Find {
                from: 5,
                scope: 0..s.len(),
                backwards: false,
            },
        ));
        let mut found_first = false;
        let total = loop {
            match wait(&t).outcome {
                Outcome::Count { count, done, .. } if done => break count,
                Outcome::Count { .. } => {}
                Outcome::Found { found, .. } => {
                    assert_eq!(found, Some(5..6));
                    found_first = true;
                }
                other => panic!("{other:?}"),
            }
        };
        assert!(found_first, "the find was answered before the count ended");
        assert_eq!(total, size);
        t.send(job(
            1,
            &s,
            q,
            Work::Index {
                at: size - 3,
                scope: 0..s.len(),
            },
        ));
        assert_eq!(
            wait(&t).outcome,
            Outcome::Index {
                at: size - 3,
                index: Some(size - 3)
            }
        );

        let mut cp = Checkpoints::new(1, &Snapshot::empty(), &Query::default(), 0);
        for n in 0..100_000_000u64 / 64 {
            cp.push(n * 64, n);
        }
        assert!(cp.points.len() <= MAX_CHECKPOINTS);
        assert!(cp.points.windows(2).all(|w| w[0].0 < w[1].0));
    }

    /// Timings for the review's count and Replace All cases (`cargo test
    /// --release -p rte_app -- --ignored --nocapture profile`); not a gate.
    #[test]
    #[ignore]
    fn profile_counts_and_replace_all() {
        let t = SearchThread::spawn(|| {});
        let mut long = vec![b'a'; 6 << 20];
        long.push(b'Z');
        let long = Snapshot::from_bytes(&long);
        for (i, pattern) in [r"\Aa+Z", "a+Z|a", "a"].into_iter().enumerate() {
            let started = Instant::now();
            let (count, _) = count_of(&t, i as u64 + 1, &long, query(pattern, Mode::Regex));
            let ms = started.elapsed().as_secs_f64() * 1000.0;
            println!("profile.count_{pattern}_6MiB count={count} ms={ms:.1}");
        }
        let one = Snapshot::from_bytes(&vec![b'a'; 16 << 20]);
        let plus = Matcher::new(&query("a+", Mode::Regex)).unwrap().unwrap();
        let never = || false;
        let mut searcher = Searcher::new(&plus, &one, &never);
        let started = Instant::now();
        let refused = replace_all(
            &mut searcher,
            0..one.len(),
            &Replacement::parse("$&$&"),
            false,
            1 << 20,
        );
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        println!(
            "profile.replace_all_refused={} ms={ms:.1}",
            refused.is_err()
        );
    }

    #[test]
    fn backward_search_keeps_the_forward_match_partition() {
        let t = SearchThread::spawn(|| {});
        let s = Snapshot::from_bytes(&vec![b'a'; 300_001]);
        let len = s.len();
        for (generation, q) in [query("aa", Mode::Text), query("a{2}", Mode::Regex)]
            .into_iter()
            .enumerate()
        {
            t.send(job(
                generation as u64 + 1,
                &s,
                q,
                Work::Find {
                    from: len,
                    scope: 0..len,
                    backwards: true,
                },
            ));
            assert!(matches!(
                wait(&t).outcome,
                Outcome::Found { found: Some(r), .. } if r == (299_998..300_000)
            ));
        }
    }

    #[test]
    fn replace_one_verifies_captures_inside_the_scope() {
        let t = SearchThread::spawn(|| {});
        let s = Snapshot::from_bytes(b"aaaa");
        t.send(job(
            1,
            &s,
            query("a+", Mode::Regex),
            Work::ReplaceOne {
                at: 0..2,
                scope: 0..2,
                replacement: Replacement::parse("[$&]"),
                preserve: false,
            },
        ));
        assert_eq!(
            wait(&t).outcome,
            Outcome::Replaced {
                at: 0..2,
                text: Some(b"[aa]".to_vec())
            }
        );
    }

    #[test]
    fn replace_all_checks_its_cap_while_building() {
        let s = Snapshot::from_bytes(&vec![b'a'; 1 << 20]);
        let m = Matcher::new(&query("a", Mode::Text)).unwrap().unwrap();
        let never = || false;
        let mut searcher = Searcher::new(&m, &s, &never);
        let rep = Replacement::literal("x");
        let result = replace_all(&mut searcher, 0..s.len(), &rep, false, 1 << 20);
        assert!(result.unwrap_err().contains("more than 1 MiB"));
        let zero = Matcher::new(&query("(?=a)", Mode::Regex)).unwrap().unwrap();
        let mut searcher = Searcher::new(&zero, &s, &never);
        let rep = Replacement::parse("");
        assert!(replace_all(&mut searcher, 0..s.len(), &rep, false, 1 << 20).is_err());
        let small = Snapshot::from_bytes(b"ab ab");
        let mut searcher = Searcher::new(&zero, &small, &never);
        let edits = replace_all(&mut searcher, 0..small.len(), &rep, false, 1 << 20).unwrap();
        assert_eq!(edits.len(), 2);
        // One large match is refused before its expansion is copied.
        let one = Snapshot::from_bytes(&vec![b'a'; 4 << 20]);
        let plus = Matcher::new(&query("a+", Mode::Regex)).unwrap().unwrap();
        let mut searcher = Searcher::new(&plus, &one, &never);
        let twice = Replacement::parse("$&$&");
        assert!(twice.bound(&mut |_| Some(one.len())) > 1 << 20);
        let result = replace_all(&mut searcher, 0..one.len(), &twice, false, 1 << 20);
        assert!(result.unwrap_err().contains("more than 1 MiB"));
        let mut searcher = Searcher::new(&plus, &one, &never);
        let upper = Replacement::parse(r"\U$&");
        assert!(replace_all(&mut searcher, 0..one.len(), &upper, false, 4 << 20).is_err());
        let mut searcher = Searcher::new(&plus, &one, &never);
        assert!(replace_all(&mut searcher, 0..one.len(), &upper, false, 32 << 20).is_ok());
    }
}
