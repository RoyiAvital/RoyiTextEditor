//! Favorite and recently run palette commands, kept in
//! `UserSettings/Palette.json` beside the executable.
//!
//! The UI applies every change at once. A worker thread, started when the
//! palette first opens, reads the file and then persists each change in
//! order: it re-reads the file (keeping changes of other instances) and
//! replaces it atomically. A file that is unreadable, too large or not valid
//! is reported and left unchanged; changes then last for the session only.

use std::io::{self, Read};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use serde_json::Value;

use crate::commands::{COMMANDS, index_of};
use crate::config::RECENT_MAX;

pub const FILE_NAME: &str = "Palette.json";
/// Larger files are refused; the editor writes about 40 bytes per entry.
const MAX_FILE_BYTES: u64 = 64 * 1024;
pub const MAX_FAVORITES: usize = 256;
const MAX_ID_BYTES: usize = 128;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Lists {
    /// Command IDs in the order they were starred; unknown IDs are kept.
    pub favorites: Vec<String>,
    /// Command IDs, most recent first.
    pub recent: Vec<String>,
}

#[derive(Clone, Copy, Debug)]
enum Change {
    Favorite(&'static str, bool),
    Recent(&'static str),
}

impl Lists {
    fn apply(&mut self, change: Change) {
        match change {
            Change::Favorite(id, true) => {
                if self.favorites.len() < MAX_FAVORITES && !self.favorites.iter().any(|f| f == id) {
                    self.favorites.push(id.to_owned());
                }
            }
            Change::Favorite(id, false) => self.favorites.retain(|f| f != id),
            Change::Recent(id) => {
                self.recent.retain(|r| r != id);
                self.recent.insert(0, id.to_owned());
                self.recent.truncate(RECENT_MAX);
            }
        }
    }
}

/// Reads and replaces the state file.
pub trait Storage: Send + 'static {
    /// At most `limit + 1` bytes of the file, or `None` when it does not exist.
    fn read(&mut self, limit: u64) -> io::Result<Option<Vec<u8>>>;
    fn write(&mut self, bytes: &[u8]) -> io::Result<()>;
}

pub struct FileStorage(pub PathBuf);

impl Storage for FileStorage {
    fn read(&mut self, limit: u64) -> io::Result<Option<Vec<u8>>> {
        let file = match std::fs::File::open(&self.0) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let mut bytes = Vec::new();
        file.take(limit + 1).read_to_end(&mut bytes)?;
        Ok(Some(bytes))
    }

    /// Writes a sibling temporary file and renames it over the file.
    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        if let Some(dir) = self.0.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let temp = self
            .0
            .with_extension(format!("json.{}.tmp", std::process::id()));
        std::fs::write(&temp, bytes)?;
        std::fs::rename(&temp, &self.0).inspect_err(|_| {
            let _ = std::fs::remove_file(&temp);
        })
    }
}

/// The lists in the file, or why the file is left alone.
fn load(storage: &mut dyn Storage) -> Result<Lists, String> {
    let refused = |why: &str| {
        format!("{FILE_NAME} {why}; it is left unchanged and favorites last for this session")
    };
    let bytes = match storage.read(MAX_FILE_BYTES) {
        Ok(None) => return Ok(Lists::default()),
        Ok(Some(bytes)) => bytes,
        Err(e) => return Err(refused(&format!("cannot be read ({e})"))),
    };
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(refused("is larger than 64 KiB"));
    }
    parse(&bytes).ok_or_else(|| refused("is not valid"))
}

/// Lists in JSON text: at most [`MAX_FAVORITES`] favorites and IDs of at most
/// [`MAX_ID_BYTES`]. Duplicates and recent entries past [`RECENT_MAX`] are dropped.
fn parse(bytes: &[u8]) -> Option<Lists> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    let object = value.as_object()?;
    let ids = |key: &str, keep: usize| -> Option<Vec<String>> {
        let Some(items) = object.get(key) else {
            return Some(Vec::new());
        };
        let mut ids: Vec<String> = Vec::new();
        for item in items.as_array()? {
            let id = item.as_str().filter(|id| id.len() <= MAX_ID_BYTES)?;
            if ids.len() < keep && !ids.iter().any(|i| i == id) {
                ids.push(id.to_owned());
            }
        }
        Some(ids)
    };
    let favorite_count = object
        .get("favorites")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    if favorite_count > MAX_FAVORITES {
        return None;
    }
    Some(Lists {
        favorites: ids("favorites", MAX_FAVORITES)?,
        recent: ids("recent", RECENT_MAX)?,
    })
}

fn encode(lists: &Lists) -> Vec<u8> {
    let array = |ids: &[String]| serde_json::to_string(ids).unwrap_or_else(|_| "[]".to_owned());
    format!(
        "{{\n  \"favorites\": {},\n  \"recent\": {}\n}}\n",
        array(&lists.favorites),
        array(&lists.recent)
    )
    .into_bytes()
}

/// The lists after the change numbered `seq` (0: the first read).
struct Reply {
    seq: u64,
    lists: Lists,
    problem: Option<String>,
}

fn run(
    mut storage: impl Storage,
    changes: Receiver<(u64, Change)>,
    replies: Sender<Reply>,
    wake: impl Fn(),
) {
    let send = |reply: Reply| {
        if replies.send(reply).is_ok() {
            wake();
        }
    };
    let (mut lists, mut refused) = match load(&mut storage) {
        Ok(lists) => (lists, None),
        Err(why) => (Lists::default(), Some(why)),
    };
    send(Reply {
        seq: 0,
        lists: lists.clone(),
        problem: refused.clone(),
    });
    for (seq, change) in changes {
        let mut problem = None;
        if refused.is_none() {
            match load(&mut storage) {
                Ok(disk) => lists = disk,
                Err(why) => {
                    problem = Some(why.clone());
                    refused = Some(why);
                }
            }
        }
        lists.apply(change);
        if refused.is_none()
            && let Err(e) = storage.write(&encode(&lists))
        {
            problem = Some(format!("{FILE_NAME} was not saved: {e}"));
        }
        send(Reply {
            seq,
            lists: lists.clone(),
            problem,
        });
    }
}

struct Worker {
    changes: Sender<(u64, Change)>,
    replies: Receiver<Reply>,
}

/// The lists as the UI shows them, indexed by registry position.
pub struct Store {
    lists: Lists,
    starred: Vec<bool>,
    /// Registry indices of the known recent commands, without duplicates.
    recent: Vec<usize>,
    worker: Option<Worker>,
    /// Number of the last change sent to the worker.
    sent: u64,
    /// Number of the last reply received.
    received: Option<u64>,
    rev: u64,
}

impl Store {
    /// A store that is never saved.
    pub fn memory() -> Store {
        Store {
            lists: Lists::default(),
            starred: vec![false; COMMANDS.len()],
            recent: Vec::new(),
            worker: None,
            sent: 0,
            received: None,
            rev: 0,
        }
    }

    /// Starts the worker, which reads `storage` first; `wake` runs after
    /// each of its replies.
    pub fn spawn(storage: impl Storage, wake: impl Fn() + Send + 'static) -> Store {
        let (changes, worker_changes) = mpsc::channel();
        let (worker_replies, replies) = mpsc::channel();
        let started = thread::Builder::new()
            .name("palette-state".to_owned())
            .spawn(move || run(storage, worker_changes, worker_replies, wake));
        Store {
            worker: started.is_ok().then_some(Worker { changes, replies }),
            ..Store::memory()
        }
    }

    pub fn is_favorite(&self, index: usize) -> bool {
        self.starred.get(index).copied().unwrap_or(false)
    }

    /// Registry indices of recent commands, most recent first.
    pub fn recent(&self) -> &[usize] {
        &self.recent
    }

    /// Changes whenever the shown lists change.
    pub fn rev(&self) -> u64 {
        self.rev
    }

    /// Stars or unstars a registry command; `false` when the favorites are full.
    pub fn set_favorite(&mut self, index: usize, on: bool) -> bool {
        if self.is_favorite(index) == on {
            return true;
        }
        if on && self.lists.favorites.len() >= MAX_FAVORITES {
            return false;
        }
        self.change(Change::Favorite(COMMANDS[index].id, on));
        true
    }

    pub fn push_recent(&mut self, index: usize) {
        if self.recent.first() != Some(&index) {
            self.change(Change::Recent(COMMANDS[index].id));
        }
    }

    fn change(&mut self, change: Change) {
        self.lists.apply(change);
        self.index();
        if let Some(worker) = &self.worker {
            self.sent += 1;
            let _ = worker.changes.send((self.sent, change));
        }
    }

    fn index(&mut self) {
        self.starred.fill(false);
        self.recent.clear();
        for i in self.lists.favorites.iter().filter_map(|id| index_of(id)) {
            self.starred[i] = true;
        }
        for i in self.lists.recent.iter().filter_map(|id| index_of(id)) {
            if !self.recent.contains(&i) {
                self.recent.push(i);
            }
        }
        self.rev += 1;
    }

    /// Applies the worker's replies; returns the last problem it reported.
    /// Lists from replies older than the last change are stale and ignored.
    pub fn drain(&mut self) -> Option<String> {
        let worker = self.worker.as_ref()?;
        let (mut problem, mut latest) = (None, None);
        while let Ok(reply) = worker.replies.try_recv() {
            self.received = Some(reply.seq);
            problem = reply.problem.or(problem);
            if reply.seq == self.sent {
                latest = Some(reply.lists);
            }
        }
        if let Some(lists) = latest
            && lists != self.lists
        {
            self.lists = lists;
            self.index();
        }
        problem
    }

    /// Waits until the worker answered every change; returns the last problem.
    #[cfg(test)]
    pub fn settle(&mut self) -> Option<String> {
        let mut problem = None;
        for _ in 0..5000 {
            problem = self.drain().or(problem);
            if self.worker.is_none() || self.received == Some(self.sent) {
                return problem;
            }
            thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("the palette state worker did not answer");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    const SAVE: &str = "workbench.action.files.save";
    const UNDO: &str = "undo";

    fn at(id: &str) -> usize {
        index_of(id).unwrap()
    }

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rte_favorites_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("UserSettings").join(FILE_NAME)
    }

    fn remove(path: &std::path::Path) {
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    fn open(path: &std::path::Path) -> Store {
        Store::spawn(FileStorage(path.to_path_buf()), || {})
    }

    #[test]
    fn changes_are_written_and_read_back() {
        let path = temp_path("roundtrip");
        let mut store = open(&path);
        assert_eq!(store.settle(), None);
        store.set_favorite(at(UNDO), true);
        store.set_favorite(at(SAVE), true);
        store.set_favorite(at(UNDO), false);
        store.push_recent(at(UNDO));
        store.push_recent(at(SAVE));
        assert_eq!(store.settle(), None);
        let mut again = open(&path);
        assert_eq!(again.settle(), None);
        assert!(again.is_favorite(at(SAVE)) && !again.is_favorite(at(UNDO)));
        assert_eq!(again.recent(), [at(SAVE), at(UNDO)]);
        let leftovers = std::fs::read_dir(path.parent().unwrap()).unwrap().count();
        assert_eq!(leftovers, 1, "no temporary file remains");
        remove(&path);
    }

    #[test]
    fn each_change_keeps_changes_of_other_instances() {
        let path = temp_path("shared");
        let mut first = open(&path);
        let mut second = open(&path);
        first.settle();
        second.settle();
        first.set_favorite(at(UNDO), true);
        first.settle();
        second.set_favorite(at(SAVE), true);
        second.settle();
        assert!(second.is_favorite(at(UNDO)) && second.is_favorite(at(SAVE)));
        remove(&path);
    }

    #[test]
    fn the_recent_list_is_capped_and_unique() {
        let mut store = Store::memory();
        for i in (0..COMMANDS.len()).chain([0]) {
            store.push_recent(i);
        }
        assert_eq!(store.recent().len(), RECENT_MAX);
        assert_eq!(store.recent()[0], 0);
    }

    #[test]
    fn refused_files_are_reported_and_left_unchanged() {
        let path = temp_path("refused");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let long_id = format!(r#"{{"favorites": ["{}"]}}"#, "x".repeat(MAX_ID_BYTES + 1));
        let many: Vec<String> = (0..=MAX_FAVORITES).map(|i| format!("id{i}")).collect();
        let too_many = format!(
            r#"{{"favorites": {}}}"#,
            serde_json::to_string(&many).unwrap()
        );
        let oversized = format!(
            r#"{{"recent": [{}"{SAVE}"]}}"#,
            r#""x","#.repeat(MAX_FILE_BYTES as usize / 4)
        );
        for text in [
            "not json",
            "[1]",
            r#"{"favorites": [1]}"#,
            r#"{"recent": "x"}"#,
            &long_id,
            &too_many,
            &oversized,
        ] {
            std::fs::write(&path, text).unwrap();
            let mut store = open(&path);
            let problem = store.settle().unwrap_or_default();
            assert!(problem.contains("left unchanged"), "{problem}");
            store.set_favorite(at(UNDO), true);
            assert_eq!(store.settle(), None, "reported once");
            assert!(store.is_favorite(at(UNDO)), "kept for the session");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        }
        remove(&path);
    }

    #[test]
    fn duplicates_unknown_ids_and_extra_recent_entries() {
        let recent: Vec<String> = (0..RECENT_MAX + 10).map(|i| format!("r{i}")).collect();
        let text = format!(
            r#"{{"favorites": ["{UNDO}", "gone", "{UNDO}"], "recent": {}}}"#,
            serde_json::to_string(&recent).unwrap()
        );
        let lists = parse(text.as_bytes()).unwrap();
        assert_eq!(lists.favorites, vec![UNDO, "gone"]);
        assert_eq!(lists.recent.len(), RECENT_MAX);
        assert_eq!(parse(&encode(&lists)), Some(lists));
    }

    /// Storage that blocks every read until released and can fail writes.
    struct Gated {
        gate: Receiver<()>,
        file: Arc<Mutex<Option<Vec<u8>>>>,
        fail_writes: bool,
    }

    impl Storage for Gated {
        fn read(&mut self, _: u64) -> io::Result<Option<Vec<u8>>> {
            self.gate.recv().map_err(|_| io::ErrorKind::BrokenPipe)?;
            Ok(self.file.lock().unwrap().clone())
        }

        fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
            if self.fail_writes {
                return Err(io::ErrorKind::PermissionDenied.into());
            }
            *self.file.lock().unwrap() = Some(bytes.to_vec());
            Ok(())
        }
    }

    #[test]
    fn slow_storage_never_delays_changes() {
        let (open, gate) = mpsc::channel();
        let file = Arc::new(Mutex::new(Some(
            format!(r#"{{"favorites": ["{SAVE}"]}}"#).into_bytes(),
        )));
        let mut store = Store::spawn(
            Gated {
                gate,
                file: file.clone(),
                fail_writes: false,
            },
            || {},
        );
        // The worker is blocked in its first read; changes still apply at once.
        store.set_favorite(at(UNDO), true);
        store.push_recent(at(UNDO));
        assert!(store.is_favorite(at(UNDO)));
        assert_eq!(store.drain(), None);
        assert!(!store.is_favorite(at(SAVE)), "not read yet");
        for _ in 0..3 {
            open.send(()).unwrap();
        }
        assert_eq!(store.settle(), None);
        assert!(store.is_favorite(at(SAVE)) && store.is_favorite(at(UNDO)));
        assert_eq!(store.recent(), [at(UNDO)]);
        let saved = parse(file.lock().unwrap().as_deref().unwrap()).unwrap();
        assert_eq!(saved.favorites, vec![SAVE, UNDO]);
    }

    #[test]
    fn failed_writes_are_reported_and_changes_kept() {
        let (open, gate) = mpsc::channel();
        let mut store = Store::spawn(
            Gated {
                gate,
                file: Arc::new(Mutex::new(None)),
                fail_writes: true,
            },
            || {},
        );
        open.send(()).unwrap();
        open.send(()).unwrap();
        store.set_favorite(at(UNDO), true);
        let problem = store.settle().unwrap();
        assert!(problem.contains("was not saved"), "{problem}");
        assert!(store.is_favorite(at(UNDO)));
    }
}
