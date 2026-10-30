use std::io;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::*;

fn wake() -> Wake {
    Arc::new(|| {})
}

fn start(storage: Option<Box<dyn Storage>>) -> SpellThread {
    let config = Config {
        engine: Engine::Builtin,
        hunspell: PathBuf::new(),
        storage,
    };
    SpellThread::spawn(config, wake()).unwrap()
}

/// The next event within 10 s.
fn next(thread: &SpellThread) -> Event {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(event) = thread.try_recv() {
            return event;
        }
        assert!(Instant::now() < until, "no spelling event");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// The next reply, skipping progress reports.
fn reply(thread: &SpellThread) -> Reply {
    loop {
        match next(thread) {
            Event::Reply(r) if matches!(r.outcome, Outcome::Progress { .. }) => {}
            Event::Reply(r) => return r,
            Event::Notice { text, .. } => panic!("notice: {text}"),
        }
    }
}

fn request(id: u64, snapshot: &Snapshot, work: Work) -> Request {
    Request {
        id,
        generation: 0,
        snapshot: snapshot.clone(),
        mask: None,
        work,
    }
}

fn marks_of(
    thread: &SpellThread,
    text: &str,
    ranges: Vec<Range<u64>>,
) -> (Vec<Range<u64>>, Vec<String>) {
    let s = Snapshot::from_bytes(text.as_bytes());
    thread.send(request(1, &s, Work::Marks(ranges)));
    match reply(thread).outcome {
        Outcome::Marks { covered, marks } => (covered, marks.into_iter().map(|m| m.word).collect()),
        other => panic!("{other:?}"),
    }
}

fn navigate(thread: &SpellThread, s: &Snapshot, origin: u64, backwards: bool) -> Outcome {
    thread.send(request(7, s, Work::Navigate { origin, backwards }));
    let r = reply(thread);
    assert_eq!((r.lane, r.id), (Lane::Navigate, 7));
    r.outcome
}

fn found(outcome: &Outcome) -> Option<&str> {
    match outcome {
        Outcome::Found(m) => Some(&m.word),
        _ => None,
    }
}

#[test]
fn marks_cover_the_requested_ranges_and_skip_urls_and_acronyms() {
    let thread = start(None);
    let text = "The recieve of teh http://exmple.org NASA data.\nSecond lin with wrod.\n";
    let all = 0..text.len() as u64;
    let (covered, words) = marks_of(&thread, text, vec![all.clone()]);
    assert_eq!(covered, [all]);
    assert_eq!(words, ["recieve", "teh", "lin", "wrod"]);
    // A range starting inside a word checks the whole word.
    let (_, words) = marks_of(&thread, text, vec![6..8]);
    assert_eq!(words, ["recieve"]);
}

#[test]
fn marks_stop_at_the_word_and_mark_bounds_with_partial_coverage() {
    let thread = start(None);
    let text = "zzq ".repeat(MAX_MARKS + 10);
    let (covered, words) = marks_of(&thread, &text, vec![0..text.len() as u64]);
    assert_eq!(words.len(), MAX_MARKS);
    assert_eq!(covered, [0..(MAX_MARKS * 4) as u64], "a prefix is covered");
    let text = "word ".repeat(MAX_REQUEST_WORDS + 10);
    let (covered, words) = marks_of(&thread, &text, vec![0..text.len() as u64]);
    assert!(words.is_empty());
    assert_eq!(covered, [0..(MAX_REQUEST_WORDS * 5) as u64]);
}

#[test]
fn navigation_finds_wraps_and_reports_exhausted() {
    let thread = start(None);
    let s = Snapshot::from_bytes(b"one recieve two teh three");
    assert_eq!(found(&navigate(&thread, &s, 0, false)), Some("recieve"));
    assert_eq!(found(&navigate(&thread, &s, 11, false)), Some("teh"));
    // Past the last one: wraps to the first.
    assert_eq!(found(&navigate(&thread, &s, 20, false)), Some("recieve"));
    assert_eq!(found(&navigate(&thread, &s, 20, true)), Some("teh"));
    assert_eq!(found(&navigate(&thread, &s, 16, true)), Some("recieve"));
    // At the start of a misspelling it is skipped once.
    assert_eq!(found(&navigate(&thread, &s, 4, false)), Some("teh"));

    let single = Snapshot::from_bytes(b"all good recieve here");
    match navigate(&thread, &single, 12, false) {
        Outcome::Exhausted(Some(m)) => assert_eq!(m.word, "recieve"),
        other => panic!("{other:?}"),
    }
    let clean = Snapshot::from_bytes(b"all good here");
    assert_eq!(navigate(&thread, &clean, 3, true), Outcome::Exhausted(None));
    assert_eq!(
        navigate(&thread, &Snapshot::empty(), 0, false),
        Outcome::Exhausted(None)
    );
}

#[test]
fn navigation_resumes_across_many_slices_and_slice_boundaries() {
    let thread = start(None);
    // About 1 MiB of correct text, then one misspelling: several slices.
    let mut text = "the quick brown fox jumps over the lazy dog ".repeat(24_000);
    let at = text.len();
    text.push_str("recieve end");
    let s = Snapshot::from_bytes(text.as_bytes());
    match navigate(&thread, &s, 0, false) {
        Outcome::Found(m) => assert_eq!(m.range, at as u64..at as u64 + 7),
        other => panic!("{other:?}"),
    }
    match navigate(&thread, &s, s.len(), true) {
        Outcome::Found(m) => assert_eq!(m.word, "recieve"),
        other => panic!("{other:?}"),
    }
    // A word across a slice boundary is checked whole.
    let mut text = "a".repeat(SLICE_BYTES as usize - 3);
    text.push_str(" wordd ");
    let s = Snapshot::from_bytes(text.as_bytes());
    assert_eq!(found(&navigate(&thread, &s, 0, false)), Some("wordd"));
}

#[test]
fn suggestions_come_closest_first() {
    let thread = start(None);
    let s = Snapshot::from_bytes(b"recieve");
    let mark = Mark {
        range: 0..7,
        word: "recieve".to_owned(),
    };
    thread.send(request(3, &s, Work::Suggest(mark)));
    match reply(&thread).outcome {
        Outcome::Suggestions(list) => {
            assert_eq!(list.first().map(String::as_str), Some("receive"));
            assert!(list.len() <= MAX_SUGGESTIONS);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_newer_request_replaces_the_pending_one_of_its_lane() {
    let thread = start(None);
    let s = Snapshot::from_bytes(b"teh");
    // Queued together: the thread may serve the first before the second
    // arrives, but the last reply is the newest request's.
    thread.send(request(1, &s, Work::Marks(vec![0..3])));
    thread.send(request(2, &s, Work::Marks(vec![0..3])));
    let mut last = reply(&thread);
    if last.id == 1 {
        last = reply(&thread);
    }
    assert_eq!(last.id, 2);
    thread.cancel(Lane::Marks);
}

#[test]
fn added_and_ignored_words_are_correct_and_added_ones_persist() {
    let storage = Memory::default();
    let thread = start(Some(Box::new(storage.clone())));
    thread.add("recieve");
    thread.ignore("teh");
    let (_, words) = marks_of(&thread, "recieve teh wrod", vec![0..16]);
    assert_eq!(words, ["wrod"]);
    drop(thread);
    assert_eq!(storage.text(), "recieve\r\n");
}

#[test]
fn a_missing_hunspell_dictionary_falls_back_with_a_notice() {
    let config = Config {
        engine: Engine::Hunspell,
        hunspell: PathBuf::from("missing/en_US"),
        storage: None,
    };
    let thread = SpellThread::spawn(config, wake()).unwrap();
    match next(&thread) {
        Event::Notice { text, reset } => {
            assert!(
                text.contains("Hunspell") && text.contains("built-in"),
                "{text}"
            );
            assert!(!reset);
        }
        other => panic!("{other:?}"),
    }
    let (_, words) = marks_of(&thread, "teh", vec![0..3]);
    assert_eq!(words, ["teh"]);
}

#[test]
fn an_invalid_user_dictionary_is_never_written() {
    let storage = Memory::default();
    storage.set(b"has space\r\n");
    let thread = start(Some(Box::new(storage.clone())));
    match next(&thread) {
        Event::Notice { text, .. } => assert!(text.contains("left unchanged"), "{text}"),
        other => panic!("{other:?}"),
    }
    thread.add("recieve");
    let (_, words) = marks_of(&thread, "recieve", vec![0..7]);
    assert!(words.is_empty(), "the word lasts for the session");
    assert_eq!(storage.text(), "has space\r\n");
}

/// Markdown marks come only from verified structure: code, links and HTML
/// are never marked.
#[test]
fn markdown_exclusions_come_from_the_syntax_worker() {
    let text = "Prose wrod `codez` [linkk](http://x.org/pth) <b>tagg</b>\n\n```\nfenc\n```\n";
    let s = Snapshot::from_bytes(text.as_bytes());
    let lang = rte_syntax::by_id("markdown").unwrap();
    let syntax = crate::syntax::Syntax::start(lang, s.clone(), true, wake());
    let thread = start(None);
    thread.send(Request {
        mask: Some(syntax.mask_port()),
        ..request(1, &s, Work::Marks(vec![0..s.len()]))
    });
    match reply(&thread).outcome {
        Outcome::Marks { marks, .. } => {
            let words: Vec<_> = marks.iter().map(|m| m.word.as_str()).collect();
            assert_eq!(words, ["wrod", "linkk", "tagg"]);
        }
        other => panic!("{other:?}"),
    }
    // An older text is not verified.
    let old = Snapshot::from_bytes(b"teh");
    thread.send(Request {
        mask: Some(syntax.mask_port()),
        ..request(2, &old, Work::Marks(vec![0..3]))
    });
    assert_eq!(reply(&thread).outcome, Outcome::Unavailable);
    // A stopped syntax thread answers nothing: unavailable.
    let port = syntax.mask_port();
    drop(syntax);
    thread.send(Request {
        mask: Some(port),
        ..request(3, &s, Work::Marks(vec![0..3]))
    });
    assert_eq!(reply(&thread).outcome, Outcome::Unavailable);
}

/// In-memory storage shared with the test.
#[derive(Clone, Default)]
struct Memory(Arc<Mutex<MemoryState>>);

#[derive(Default)]
struct MemoryState {
    file: Option<Vec<u8>>,
    fail_write: bool,
}

impl Memory {
    fn set(&self, bytes: &[u8]) {
        self.0.lock().unwrap().file = Some(bytes.to_vec());
    }

    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().file.clone().unwrap_or_default()).unwrap()
    }
}

impl Storage for Memory {
    fn read(&mut self, limit: u64) -> io::Result<Option<Vec<u8>>> {
        let state = self.0.lock().unwrap();
        Ok(state
            .file
            .as_ref()
            .map(|f| f[..f.len().min(limit as usize + 1)].to_vec()))
    }

    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        let mut state = self.0.lock().unwrap();
        if state.fail_write {
            return Err(io::Error::other("disk full"));
        }
        state.file = Some(bytes.to_vec());
        Ok(())
    }
}

#[test]
fn persisting_merges_external_changes_and_respects_the_limits() {
    let mut storage = Memory::default();
    storage.set(b"zebra\r\n");
    persist(&mut storage, &["apple".to_owned()]).unwrap();
    assert_eq!(storage.text(), "apple\r\nzebra\r\n");
    // Changed externally between additions: nothing is lost.
    storage.set(b"apple\r\nmango\r\nzebra\r\n");
    persist(&mut storage, &["apple".to_owned(), "kiwi".to_owned()]).unwrap();
    assert_eq!(storage.text(), "apple\r\nkiwi\r\nmango\r\nzebra\r\n");
    // Duplicates are written once.
    persist(&mut storage, &["kiwi".to_owned()]).unwrap();
    assert_eq!(storage.text(), "apple\r\nkiwi\r\nmango\r\nzebra\r\n");
}

#[test]
fn persisting_fails_without_losing_words_or_passing_limits() {
    // At the word limit: the file stays as it is.
    let letter = |n: usize| char::from(b'a' + (n % 26) as u8);
    let mut full: Vec<String> = (0..rte_spell::USER_MAX_WORDS)
        .map(|i| {
            [letter(i / 676), letter(i / 26), letter(i)]
                .iter()
                .collect()
        })
        .collect();
    full.sort();
    let mut storage = Memory::default();
    storage.set(&rte_spell::encode_user(&full).unwrap());
    let before = storage.text();
    assert!(
        persist(&mut storage, &["extra".to_owned()])
            .unwrap_err()
            .contains("10,000")
    );
    assert_eq!(storage.text(), before);
    // Write failures and invalid files leave the file alone.
    let mut storage = Memory::default();
    storage.set(b"apple\r\n");
    storage.0.lock().unwrap().fail_write = true;
    assert!(
        persist(&mut storage, &["kiwi".to_owned()])
            .unwrap_err()
            .contains("disk full")
    );
    assert_eq!(storage.text(), "apple\r\n");
    let mut storage = Memory::default();
    storage.set(b"two words\r\n");
    assert!(persist(&mut storage, &["kiwi".to_owned()]).is_err());
    assert_eq!(storage.text(), "two words\r\n");
}

/// An engine that fails at run time is replaced by the built-in one: one
/// notice that resets earlier replies, pending work dropped.
#[test]
fn an_engine_error_falls_back_to_the_builtin_engine() {
    struct Failing;
    impl Speller for Failing {
        fn name(&self) -> &'static str {
            "failing"
        }
        fn check(&mut self, _: &str) -> Result<bool, String> {
            Err("broken".to_owned())
        }
        fn suggest(&mut self, _: &str, _: usize, _: Instant) -> Result<Vec<String>, String> {
            Err("broken".to_owned())
        }
    }
    let (events_tx, events) = mpsc::channel();
    let (to_self, _rx) = mpsc::channel();
    let mut worker = Worker {
        checker: Checker::new(Box::new(Failing)),
        events: events_tx,
        wake: wake(),
        to_self,
        storage: None,
        writable: false,
        added: Vec::new(),
        lanes: [None, None, None],
        words: Vec::new(),
        text: Vec::new(),
    };
    let s = Snapshot::from_bytes(b"teh");
    worker.handle(Msg::Request(request(1, &s, Work::Marks(vec![0..3]))));
    worker.step(Lane::Marks as usize);
    match events.try_recv().unwrap() {
        Event::Notice { text, reset } => {
            assert!(
                text.contains("failing") && text.contains("broken"),
                "{text}"
            );
            assert!(reset);
        }
        other => panic!("{other:?}"),
    }
    assert!(worker.lanes.iter().all(Option::is_none));
    assert_eq!(worker.checker.engine(), "builtin");
    worker.handle(Msg::Request(request(2, &s, Work::Marks(vec![0..3]))));
    worker.step(Lane::Marks as usize);
    match events.try_recv().unwrap() {
        Event::Reply(r) => {
            assert!(matches!(r.outcome, Outcome::Marks { ref marks, .. } if marks.len() == 1))
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn widening_keeps_whole_tokens() {
    let s = Snapshot::from_bytes(b"one two three");
    assert_eq!(widen(&s, 5..6), 4..7);
    assert_eq!(widen(&s, 0..0), 0..3);
    assert_eq!(widen(&s, 13..13), 8..13);
    // Inside a token longer than the scan: a punctuation boundary.
    let long = format!("{},word", "x".repeat(600));
    let s = Snapshot::from_bytes(long.as_bytes());
    assert_eq!(widen(&s, 602..603), 601..s.len());
}
