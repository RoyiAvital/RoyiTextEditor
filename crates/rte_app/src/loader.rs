//! Background file loading in blocks, with progressive publication.

use std::fs::File;
use std::io::{self, Read};
use std::path::PathBuf;
use std::sync::Arc;
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
/// stopped without a final event (its sender is disconnected by then).
pub fn spawn(
    path: PathBuf,
    wake: impl Fn() + Clone + Send + 'static,
    on_done: Option<OnDone>,
) -> Receiver<LoadEvent> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _final_wake = WakeOnDrop(wake.clone());
        load(path, tx, wake, on_done);
    });
    rx
}

/// Loads `path`, sending events to `tx` and calling `wake` after each send.
pub fn load(path: PathBuf, tx: Sender<LoadEvent>, wake: impl Fn(), on_done: Option<OnDone>) {
    let send = |event| {
        let _ = tx.send(event);
        wake();
    };
    match read(&path, &send, on_done) {
        Ok(()) => {}
        Err((snapshot, error)) => send(LoadEvent::Failed { snapshot, error }),
    }
}

fn read(
    path: &PathBuf,
    send: &impl Fn(LoadEvent),
    on_done: Option<OnDone>,
) -> Result<(), (Snapshot, String)> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            send(LoadEvent::Done {
                snapshot: Snapshot::empty(),
                info: FileInfo {
                    encoding: Encoding::Utf8,
                    line_ending: LineEnding::CrLf,
                    indent_tabs: false,
                    malformed: false,
                    new_file: true,
                },
                baseline: None,
                read_ms: rte_platform::ms_since_process_start(),
            });
            return Ok(());
        }
        Err(e) => return Err((Snapshot::empty(), e.to_string())),
    };
    let total = file.metadata().map(|m| m.len()).unwrap_or(0);
    let stamp = files::stamp_of(&file).ok();
    let mut hasher = (total <= HASH_LIMIT).then(Xxh3::new);
    let mut builder = TreeBuilder::new();
    let mut encoding = Encoding::Utf8;
    let mut decoder: Option<Utf16Decoder> = None;
    let mut loaded = 0u64;
    let mut last_publish: Option<Instant> = None;
    let mut first = true;
    loop {
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
            new_file: false,
        },
        baseline,
        read_ms: rte_platform::ms_since_process_start(),
    });
    Ok(())
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
        );
        let events: Vec<_> = rx.iter().collect();
        assert!(matches!(events.last(), Some(LoadEvent::Done { .. })));
        // One wake for the event, one when the thread ends (just after the channel closes).
        let deadline = Instant::now() + Duration::from_secs(5);
        while wakes.load(std::sync::atomic::Ordering::SeqCst) < 2 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(wakes.load(std::sync::atomic::Ordering::SeqCst), 2);
    }
}
