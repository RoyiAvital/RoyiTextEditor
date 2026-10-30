//! Background file loading in blocks, with progressive publication.

use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use rte_platform::files;
use rte_text::{BUFFER_MAX, Encoding, LineEnding, Snapshot, TreeBuilder, Utf16Decoder};
use xxhash_rust::xxh3::Xxh3;

use crate::save::{Baseline, HASH_LIMIT};

const FIRST_BLOCK: usize = 1024 * 1024;
const BLOCK: usize = 16 * 1024 * 1024;
/// Smallest read: confirms the end of the file, or catches a file that grew.
const PROBE: usize = 64 * 1024;
const PUBLISH_INTERVAL: Duration = Duration::from_millis(50);
const LINE_ENDING_SAMPLE: u64 = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileInfo {
    pub encoding: Encoding,
    pub line_ending: LineEnding,
    /// More sampled lines are indented with tabs than with spaces.
    pub indent_tabs: bool,
    /// Malformed UTF-16: the document must stay read-only and unsaved.
    pub malformed: bool,
    /// The sampled lines break with a lone CR only (no LF): shown as one line.
    pub cr_breaks: bool,
    /// The path does not exist yet.
    pub new_file: bool,
}

pub enum LoadEvent {
    /// Read-only prefix of a file still loading.
    Progress {
        snapshot: Snapshot,
        loaded: u64,
        total: u64,
    },
    Done {
        snapshot: Snapshot,
        info: FileInfo,
        /// `None` for a new file.
        baseline: Option<Baseline>,
        /// When reading finished, in ms since process creation.
        read_ms: f64,
    },
    /// Reading failed; `snapshot` holds what was read before the error.
    Failed { snapshot: Snapshot, error: String },
    /// Nothing was read: the path is a folder, or (when missing files are
    /// not created) does not exist. Sent before any other event.
    Rejected(String),
}

/// A running load: its events, and a stop flag the loader checks between
/// blocks and before publishing. Dropping it stops the loader.
pub struct Load {
    pub events: Receiver<LoadEvent>,
    stop: Arc<AtomicBool>,
}

impl Load {
    /// Events from a loader without a stop flag of its own (tests).
    #[cfg(test)]
    pub fn new(events: Receiver<LoadEvent>) -> Load {
        Load {
            events,
            stop: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Drop for Load {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Calls the wake function when dropped, also while unwinding from a panic.
pub(crate) struct WakeOnDrop<F: Fn()>(pub(crate) F);

impl<F: Fn()> Drop for WakeOnDrop<F> {
    fn drop(&mut self) {
        (self.0)();
    }
}

/// Called with the complete text just before [`LoadEvent::Done`].
pub type OnDone = Box<dyn FnOnce(&Snapshot) + Send>;

/// Starts loading `path` on its own thread. `wake` runs after every event and
/// once more when the thread ends, so the receiver also notices a loader that
/// stopped without a final event (its sender is disconnected by then). A
/// missing file is a new, empty document when `create_missing`, otherwise
/// [`LoadEvent::Rejected`].
pub fn spawn(
    path: PathBuf,
    wake: impl Fn() + Clone + Send + 'static,
    on_done: Option<OnDone>,
    create_missing: bool,
) -> Load {
    let (tx, rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    thread::spawn(move || {
        let _final_wake = WakeOnDrop(wake.clone());
        load_until(path, tx, wake, on_done, create_missing, &flag);
    });
    Load { events: rx, stop }
}

/// Loads `path`, sending events to `tx` and calling `wake` after each send.
#[cfg(test)]
pub fn load(path: PathBuf, tx: Sender<LoadEvent>, wake: impl Fn(), on_done: Option<OnDone>) {
    load_until(path, tx, wake, on_done, true, &AtomicBool::new(false));
}

/// [`load`] until `stop` is set: then no further block is read and nothing
/// more is published (an event already being sent may still arrive).
fn load_until(
    path: PathBuf,
    tx: Sender<LoadEvent>,
    wake: impl Fn(),
    on_done: Option<OnDone>,
    create_missing: bool,
    stop: &AtomicBool,
) {
    let stopped = || stop.load(Ordering::Relaxed);
    let send = |event| {
        if !stopped() {
            let _ = tx.send(event);
            wake();
        }
    };
    match read(&path, &send, on_done, create_missing, &stopped) {
        Ok(()) => {}
        Err((snapshot, error)) => send(LoadEvent::Failed { snapshot, error }),
    }
}

fn read(
    path: &PathBuf,
    send: &impl Fn(LoadEvent),
    on_done: Option<OnDone>,
    create_missing: bool,
    stopped: &impl Fn() -> bool,
) -> Result<(), (Snapshot, String)> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        // Windows reports a folder as not found or access denied.
        Err(_) if path.is_dir() => {
            send(LoadEvent::Rejected("Folders cannot be opened".to_owned()));
            return Ok(());
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound && !create_missing => {
            send(LoadEvent::Rejected(format!(
                "{} was not found",
                display_name(path)
            )));
            return Ok(());
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            send(LoadEvent::Done {
                snapshot: Snapshot::empty(),
                info: FileInfo {
                    encoding: Encoding::Utf8,
                    line_ending: LineEnding::CrLf,
                    indent_tabs: false,
                    malformed: false,
                    cr_breaks: false,
                    new_file: true,
                },
                baseline: None,
                read_ms: rte_platform::ms_since_process_start(),
            });
            return Ok(());
        }
        Err(e) => return Err((Snapshot::empty(), e.to_string())),
    };
    let meta = file.metadata().ok();
    if meta.as_ref().is_some_and(|m| m.is_dir()) {
        send(LoadEvent::Rejected("Folders cannot be opened".to_owned()));
        return Ok(());
    }
    let total = meta.map_or(0, |m| m.len());
    let stamp = files::stamp_of(&file).ok();
    let mut hasher = (total <= HASH_LIMIT).then(Xxh3::new);
    let mut builder = TreeBuilder::new();
    let mut encoding = Encoding::Utf8;
    let mut decoder: Option<Utf16Decoder> = None;
    let mut loaded = 0u64;
    let mut last_publish: Option<Instant> = None;
    let mut first = true;
    loop {
        if stopped() {
            return Ok(());
        }
        // Sized by what remains (a small probe at the expected end, in case the file grew).
        let standard = if first { FIRST_BLOCK } else { BLOCK };
        let remaining = usize::try_from(total.saturating_sub(loaded)).unwrap_or(usize::MAX);
        let size = standard.min(remaining.max(PROBE));
        // Allocated as the shared buffer and filled in place: full blocks are never copied.
        let mut block: Arc<[u8]> = std::iter::repeat_n(0u8, size).collect();
        let buf = Arc::get_mut(&mut block).expect("new block is unique");
        let n = fill(&mut file, buf).map_err(|e| (builder.snapshot(), e.to_string()))?;
        if n == 0 {
            break;
        }
        loaded += n as u64;
        let data = &block[..n];
        if let Some(hasher) = hasher.as_mut() {
            hasher.update(data);
        }
        let mut body = data;
        if first {
            encoding = Encoding::detect(data);
            body = &data[encoding.bom().len()..];
            decoder = match encoding {
                Encoding::Utf16Le => Some(Utf16Decoder::new(true)),
                Encoding::Utf16Be => Some(Utf16Decoder::new(false)),
                _ => None,
            };
        }
        match decoder.as_mut() {
            Some(d) => {
                // UTF-16 to UTF-8 can grow by half; keep each shared buffer within BUFFER_MAX.
                let mut utf8 = Vec::with_capacity(body.len() * 3 / 2);
                d.decode(body, &mut utf8);
                for part in utf8.chunks(BUFFER_MAX) {
                    builder.push_block(Arc::from(part));
                }
            }
            None if body.len() == block.len() => builder.push_block(block.clone()),
            // Short or BOM-trimmed: a right-sized copy, so the tree never holds unused bytes.
            None => builder.push_block(Arc::from(body)),
        }
        let more = n == size;
        if stopped() {
            return Ok(());
        }
        if more && last_publish.is_none_or(|t| t.elapsed() >= PUBLISH_INTERVAL) {
            send(LoadEvent::Progress {
                snapshot: builder.snapshot(),
                loaded,
                total: total.max(loaded),
            });
            last_publish = Some(Instant::now());
        }
        first = false;
        if !more {
            // A short read means the end of the file was reached.
            break;
        }
    }
    let mut malformed = false;
    if let Some(d) = decoder {
        let mut tail = Vec::new();
        malformed = !d.finish(&mut tail);
        if !tail.is_empty() {
            builder.push_block(Arc::from(tail));
        }
    }
    let snapshot = builder.finish();
    let sample = snapshot.to_vec(0..snapshot.len().min(LINE_ENDING_SAMPLE));
    let (mut tabs, mut spaces) = (0u32, 0u32);
    for line in sample.split(|&b| b == b'\n') {
        match line.first() {
            Some(b'\t') => tabs += 1,
            Some(b' ') => spaces += 1,
            _ => {}
        }
    }
    let baseline = stamp.map(|stamp| Baseline {
        stamp,
        // A file that changed size while loading keeps no hash; its stamp differs anyway.
        hash: hasher.filter(|_| loaded == stamp.size).map(|h| h.digest()),
    });
    // Without LF a CR is lone, unless it ends the sample and the next byte is LF.
    let cr_breaks = !sample.contains(&b'\n')
        && sample.iter().position(|&b| b == b'\r').is_some_and(|i| {
            let next = i as u64 + 1;
            i + 1 < sample.len() || snapshot.to_vec(next..next + 1) != b"\n"
        });
    // A stopped load starts no syntax work and publishes nothing.
    if stopped() {
        return Ok(());
    }
    if let Some(on_done) = on_done {
        on_done(&snapshot);
    }
    send(LoadEvent::Done {
        snapshot,
        info: FileInfo {
            encoding,
            line_ending: LineEnding::detect(&sample),
            indent_tabs: tabs > spaces,
            malformed,
            cr_breaks,
            new_file: false,
        },
        baseline,
        read_ms: rte_platform::ms_since_process_start(),
    });
    Ok(())
}

/// File name of `path` for messages.
pub fn display_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// Reads until `buf` is full or the file ends; returns the bytes read.
fn fill(file: &mut File, buf: &mut [u8]) -> io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match file.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn load_bytes(name: &str, bytes: &[u8]) -> Vec<LoadEvent> {
        let dir = std::env::temp_dir().join(format!("rte_loader_{}_{name}", std::process::id()));
        std::fs::write(&dir, bytes).unwrap();
        let (tx, rx) = mpsc::channel();
        load(dir.clone(), tx, || {}, None);
        std::fs::remove_file(&dir).unwrap();
        rx.try_iter().collect()
    }

    #[test]
    fn the_done_hook_sees_the_complete_text_before_the_event() {
        let path = std::env::temp_dir().join(format!("rte_loader_{}_hook", std::process::id()));
        std::fs::write(&path, b"fn main() {}\n").unwrap();
        let (tx, rx) = mpsc::channel();
        let seen = Arc::new(std::sync::Mutex::new(None));
        let hook = seen.clone();
        let pending = rx;
        load(
            path.clone(),
            tx,
            || {},
            Some(Box::new(move |s: &Snapshot| {
                *hook.lock().unwrap() = Some(s.to_vec(0..s.len()));
            })),
        );
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            seen.lock().unwrap().as_deref(),
            Some(&b"fn main() {}\n"[..])
        );
        let events: Vec<_> = pending.try_iter().collect();
        assert!(matches!(events.last(), Some(LoadEvent::Done { .. })));
    }

    fn done(events: &[LoadEvent]) -> (&Snapshot, FileInfo) {
        match events.last() {
            Some(LoadEvent::Done { snapshot, info, .. }) => (snapshot, *info),
            _ => panic!("load did not finish"),
        }
    }

    #[test]
    fn utf8_bom_is_stripped_and_line_ending_detected() {
        let events = load_bytes("bom", b"\xEF\xBB\xBFa\r\nb\r\n");
        let (s, info) = done(&events);
        assert_eq!(s.to_vec(0..s.len()), b"a\r\nb\r\n");
        assert_eq!(info.encoding, Encoding::Utf8Bom);
        assert_eq!(info.line_ending, LineEnding::CrLf);
        assert!(!info.malformed);
        assert!(!info.cr_breaks);
    }

    #[test]
    fn cr_only_line_breaks_are_flagged() {
        assert!(done(&load_bytes("cr", b"a\rb\rc")).1.cr_breaks);
        assert!(done(&load_bytes("cr_end", b"abc\r")).1.cr_breaks);
        assert!(!done(&load_bytes("lf", b"a\rb\nc")).1.cr_breaks);
        assert!(!done(&load_bytes("plain", b"abc")).1.cr_breaks);
        // A CRLF pair around the end of the sample is not a lone CR.
        let sample = LINE_ENDING_SAMPLE as usize;
        for prefix in [sample - 2, sample - 1, sample] {
            let mut bytes = vec![b'a'; prefix];
            bytes.extend_from_slice(b"\r\nnext\r\n");
            let name = format!("crlf_{prefix}");
            assert!(!done(&load_bytes(&name, &bytes)).1.cr_breaks, "{prefix}");
        }
    }

    #[test]
    fn utf16_is_decoded_and_malformed_flagged() {
        let events = load_bytes("utf16", b"\xFF\xFEh\0i\0\n\0");
        let (s, info) = done(&events);
        assert_eq!(s.to_vec(0..s.len()), b"hi\n");
        assert_eq!(info.encoding, Encoding::Utf16Le);
        assert!(!info.malformed);
        let events = load_bytes("utf16bad", b"\xFF\xFE\x00\xD8A");
        assert!(done(&events).1.malformed);
    }

    #[test]
    fn empty_and_exact_block_files_load_completely() {
        let events = load_bytes("empty", b"");
        assert!(done(&events).0.is_empty());
        let bytes: Vec<u8> = (0..FIRST_BLOCK).map(|i| b'a' + (i % 26) as u8).collect();
        let events = load_bytes("exact", &bytes);
        let (s, info) = done(&events);
        assert_eq!(s.to_vec(0..s.len()), bytes);
        assert!(!info.malformed);
    }

    #[test]
    fn large_file_publishes_prefix_first() {
        let bytes: Vec<u8> = (0..FIRST_BLOCK + 100)
            .map(|i| if i % 50 == 0 { b'\n' } else { b'x' })
            .collect();
        let events = load_bytes("large", &bytes);
        match &events[0] {
            LoadEvent::Progress {
                snapshot, loaded, ..
            } => {
                assert_eq!(*loaded, FIRST_BLOCK as u64);
                assert_eq!(snapshot.len(), FIRST_BLOCK as u64);
            }
            _ => panic!("expected a prefix snapshot first"),
        }
        let (s, _) = done(&events);
        assert_eq!(s.to_vec(0..s.len()), bytes);
    }

    #[test]
    fn folders_and_missing_drops_are_rejected_before_any_event() {
        let run = |path: PathBuf, create_missing: bool| {
            let (tx, rx) = mpsc::channel();
            let hook_ran = Arc::new(AtomicBool::new(false));
            let flag = hook_ran.clone();
            let hook: OnDone = Box::new(move |_| flag.store(true, Ordering::SeqCst));
            load_until(
                path,
                tx,
                || {},
                Some(hook),
                create_missing,
                &AtomicBool::new(false),
            );
            (
                rx.try_iter().collect::<Vec<_>>(),
                hook_ran.load(Ordering::SeqCst),
            )
        };
        let (events, hook) = run(std::env::temp_dir(), true);
        assert!(matches!(&events[..], [LoadEvent::Rejected(m)] if m.contains("Folders")));
        assert!(!hook);
        let missing = PathBuf::from("Z:\\definitely\\missing\\file.txt");
        let (events, _) = run(missing.clone(), false);
        assert!(matches!(&events[..], [LoadEvent::Rejected(m)] if m.contains("file.txt")));
        let (events, _) = run(missing, true);
        assert!(matches!(&events[..], [LoadEvent::Done { .. }]));
    }

    #[test]
    fn a_stopped_load_publishes_nothing_more_and_runs_no_hook() {
        let path = std::env::temp_dir().join(format!("rte_loader_{}_stop", std::process::id()));
        let bytes = vec![b'x'; FIRST_BLOCK + BLOCK + 10];
        std::fs::write(&path, &bytes).unwrap();
        for stop_at in [0usize, 1] {
            let (tx, rx) = mpsc::channel();
            let stop = Arc::new(AtomicBool::new(stop_at == 0));
            let hook_ran = Arc::new(AtomicBool::new(false));
            let flag = hook_ran.clone();
            let hook: OnDone = Box::new(move |_| flag.store(true, Ordering::SeqCst));
            // Stopped before the start, or right after the first published prefix.
            let sent = std::cell::Cell::new(0);
            let wake = || {
                sent.set(sent.get() + 1);
                if sent.get() == stop_at {
                    stop.store(true, Ordering::SeqCst);
                }
            };
            load_until(path.clone(), tx, wake, Some(hook), true, &stop);
            let events: Vec<_> = rx.try_iter().collect();
            assert_eq!(events.len(), stop_at, "stopped at {stop_at}");
            assert!(
                events
                    .iter()
                    .all(|e| matches!(e, LoadEvent::Progress { .. }))
            );
            assert!(!hook_ran.load(Ordering::SeqCst), "stopped at {stop_at}");
        }
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn dropping_a_load_stops_its_loader() {
        let path = std::env::temp_dir().join(format!("rte_loader_{}_drop", std::process::id()));
        std::fs::write(&path, vec![b'y'; FIRST_BLOCK + 10]).unwrap();
        let load = spawn(path.clone(), || {}, None, true);
        assert!(!load.stop.load(Ordering::SeqCst));
        let stop = load.stop.clone();
        drop(load);
        assert!(stop.load(Ordering::SeqCst));
        thread::sleep(Duration::from_millis(50));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn missing_file_is_a_new_empty_document() {
        let (tx, rx) = mpsc::channel();
        load(
            PathBuf::from("Z:\\definitely\\missing\\file.txt"),
            tx,
            || {},
            None,
        );
        let events: Vec<_> = rx.try_iter().collect();
        let (s, info) = done(&events);
        assert!(s.is_empty());
        assert!(info.new_file);
    }

    #[test]
    fn utf16_expanding_past_the_buffer_limit_loads() {
        // A full 16 MiB second block of U+4E2D decodes to 24 MiB of UTF-8.
        let chars = (FIRST_BLOCK + BLOCK) / 2 + 1000;
        for le in [true, false] {
            let mut bytes: Vec<u8> = if le {
                vec![0xFF, 0xFE]
            } else {
                vec![0xFE, 0xFF]
            };
            for _ in 0..chars {
                bytes.extend_from_slice(&if le {
                    0x4E2Du16.to_le_bytes()
                } else {
                    0x4E2Du16.to_be_bytes()
                });
            }
            let events = load_bytes(if le { "u16le_big" } else { "u16be_big" }, &bytes);
            let (s, info) = done(&events);
            assert!(!info.malformed);
            assert_eq!(s.len(), 3 * chars as u64);
            let middle = s.to_vec(s.len() / 2 / 3 * 3..s.len() / 2 / 3 * 3 + 6);
            assert_eq!(middle, "中中".as_bytes());
        }
    }

    #[test]
    fn spawned_loader_wakes_after_its_last_event() {
        let wakes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = wakes.clone();
        let rx = spawn(
            PathBuf::from("Z:\\definitely\\missing\\file.txt"),
            move || {
                counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            },
            None,
            true,
        );
        let events: Vec<_> = rx.events.iter().collect();
        assert!(matches!(events.last(), Some(LoadEvent::Done { .. })));
        // One wake for the event, one when the thread ends (just after the channel closes).
        let deadline = Instant::now() + Duration::from_secs(5);
        while wakes.load(std::sync::atomic::Ordering::SeqCst) < 2 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(wakes.load(std::sync::atomic::Ordering::SeqCst), 2);
    }
}
