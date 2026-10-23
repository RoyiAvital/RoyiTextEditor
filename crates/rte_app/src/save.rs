//! Saving: write a temporary sibling, verify the target against its
//! baseline, replace it atomically with a backup, verify the backup, clean up.
//! No failure path deletes the only copy of any version. Every file
//! operation goes through [`FileOps`] so tests can inject each outcome.

use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::thread;

use rte_platform::files::{self, FileStamp};
use rte_text::{Encoding, Snapshot, Utf16Encoder};
use xxhash_rust::xxh3::Xxh3;

use crate::loader::WakeOnDrop;

/// Files up to this size are compared by content hash as well.
pub const HASH_LIMIT: u64 = 64 * 1024 * 1024;

/// Recorded state of the file on disk: stamp, plus the XXH3-64 hash of
/// its bytes when it is at most [`HASH_LIMIT`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Baseline {
    pub stamp: FileStamp,
    pub hash: Option<u64>,
}

pub trait FileOps: Send + Sync {
    /// Stamp of `path`, `None` when it does not exist.
    fn stamp(&self, path: &Path) -> io::Result<Option<FileStamp>>;
    /// XXH3-64 of the bytes of `path`.
    fn hash(&self, path: &Path) -> io::Result<u64>;
    /// Creates `path` (failing if it exists), lets `write` fill it and
    /// flushes it to disk. On failure nothing created by this call remains,
    /// and a file that already existed is never touched.
    fn write_new(
        &self,
        path: &Path,
        write: &mut dyn FnMut(&mut dyn Write) -> io::Result<()>,
    ) -> io::Result<()>;
    /// `ReplaceFileW`; the error is the Win32 code.
    fn replace(&self, target: &Path, replacement: &Path, backup: &Path) -> Result<(), u32>;
    /// Move without replacing an existing destination.
    fn move_new(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn delete(&self, path: &Path) -> io::Result<()>;
}

/// The real file system.
pub struct SystemFiles;

impl FileOps for SystemFiles {
    fn stamp(&self, path: &Path) -> io::Result<Option<FileStamp>> {
        files::stamp(path)
    }

    fn hash(&self, path: &Path) -> io::Result<u64> {
        let mut file = File::open(path)?;
        let mut hasher = Xxh3::new();
        let mut buf = vec![0u8; 1 << 20];
        loop {
            match file.read(&mut buf) {
                Ok(0) => return Ok(hasher.digest()),
                Ok(n) => hasher.update(&buf[..n]),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
    }

    fn write_new(
        &self,
        path: &Path,
        write: &mut dyn FnMut(&mut dyn Write) -> io::Result<()>,
    ) -> io::Result<()> {
        let file = OpenOptions::new().write(true).create_new(true).open(path)?;
        let result = (|| {
            let mut out = BufWriter::with_capacity(1 << 20, file);
            write(&mut out)?;
            let file = out.into_inner().map_err(|e| e.into_error())?;
            file.sync_all()
        })();
        if result.is_err() {
            // Created by this call above; its handle is closed by now.
            let _ = std::fs::remove_file(path);
        }
        result
    }

    fn replace(&self, target: &Path, replacement: &Path, backup: &Path) -> Result<(), u32> {
        files::replace(target, replacement, backup)
    }

    fn move_new(&self, from: &Path, to: &Path) -> io::Result<()> {
        files::move_new(from, to)
    }

    fn delete(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_file(path)
    }
}

pub struct SaveRequest {
    pub path: PathBuf,
    pub snapshot: Snapshot,
    pub encoding: Encoding,
    /// The version the user last saw; `None` when the file did not exist.
    pub baseline: Option<Baseline>,
}

#[derive(Debug)]
pub enum SaveOutcome {
    /// `snapshot` is on disk. `warning` reports a kept backup.
    Saved {
        snapshot: Snapshot,
        baseline: Option<Baseline>,
        warning: Option<String>,
    },
    /// The file differs from the baseline; nothing was changed.
    Conflict { disk: Baseline },
    /// Nothing was saved; the message lists any surviving files.
    Failed { error: String },
}

const ERROR_UNABLE_TO_REMOVE_REPLACED: u32 = 1175;
const ERROR_UNABLE_TO_MOVE_REPLACEMENT: u32 = 1176;
const ERROR_UNABLE_TO_MOVE_REPLACEMENT_2: u32 = 1177;

/// Writes `snapshot` in `encoding` (with its BOM).
fn write_content(out: &mut dyn Write, snapshot: &Snapshot, encoding: Encoding) -> io::Result<()> {
    out.write_all(encoding.bom())?;
    let little_endian = match encoding {
        Encoding::Utf16Le => true,
        Encoding::Utf16Be => false,
        Encoding::Utf8 | Encoding::Utf8Bom => {
            for chunk in snapshot.chunks(0..snapshot.len()) {
                out.write_all(chunk)?;
            }
            return Ok(());
        }
    };
    let invalid = |_| io::Error::new(io::ErrorKind::InvalidData, "text is not valid UTF-8");
    let mut encoder = Utf16Encoder::new(little_endian);
    let mut buf = Vec::new();
    for chunk in snapshot.chunks(0..snapshot.len()) {
        buf.clear();
        encoder.encode(chunk, &mut buf).map_err(invalid)?;
        out.write_all(&buf)?;
    }
    encoder.finish().map_err(invalid)
}

/// Counts and hashes the bytes passing through.
struct Hashing<'a> {
    inner: &'a mut dyn Write,
    hasher: Xxh3,
    len: u64,
}

impl Write for Hashing<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hasher.update(&buf[..n]);
        self.len += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// A sibling of `path` named `<name>.~rte-<pid>[-n].<kind>` that does not exist.
fn free_sibling(ops: &dyn FileOps, path: &Path, kind: &str) -> io::Result<PathBuf> {
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?
        .to_string_lossy()
        .into_owned();
    let pid = std::process::id();
    for n in 0..100 {
        let suffix = if n == 0 {
            String::new()
        } else {
            format!("-{n}")
        };
        let candidate = path.with_file_name(format!("{name}.~rte-{pid}{suffix}.{kind}"));
        if ops.stamp(&candidate)?.is_none() {
            return Ok(candidate);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "no free temporary name",
    ))
}

fn hash_if_small(ops: &dyn FileOps, path: &Path, size: u64) -> io::Result<Option<u64>> {
    if size <= HASH_LIMIT {
        ops.hash(path).map(Some)
    } else {
        Ok(None)
    }
}

/// True when `path` (with `stamp`) is the baseline version: same identity,
/// size and time, and the same hash when the baseline has one.
fn is_baseline(
    ops: &dyn FileOps,
    path: &Path,
    stamp: &FileStamp,
    baseline: &Baseline,
) -> io::Result<bool> {
    if *stamp != baseline.stamp {
        return Ok(false);
    }
    match baseline.hash {
        Some(hash) => Ok(ops.hash(path)? == hash),
        None => Ok(true),
    }
}

/// True when the backup is the baseline version. `ReplaceFileW` renames the
/// old file, so size and time match; the hash (or, without one, the
/// identity) confirms it.
fn backup_is_baseline(ops: &dyn FileOps, backup: &Path, baseline: &Baseline) -> io::Result<bool> {
    let Some(stamp) = ops.stamp(backup)? else {
        return Ok(false);
    };
    if stamp.size != baseline.stamp.size || stamp.write_time != baseline.stamp.write_time {
        return Ok(false);
    }
    match baseline.hash {
        Some(hash) => Ok(ops.hash(backup)? == hash),
        None => Ok(stamp.id == baseline.stamp.id && stamp.volume == baseline.stamp.volume),
    }
}

fn exists(ops: &dyn FileOps, path: &Path) -> bool {
    ops.stamp(path).ok().flatten().is_some()
}

/// Baseline after the commit. The file must still hold the committed text
/// (size, and hash or identity); otherwise another writer changed it right
/// after the commit, and the committed version's baseline is kept instead,
/// so the next save reports a conflict rather than overwriting it.
fn committed_baseline(
    ops: &dyn FileOps,
    path: &Path,
    committed: Option<FileStamp>,
    len: u64,
    hash: u64,
) -> (Option<Baseline>, Option<String>) {
    let expected = (len <= HASH_LIMIT).then_some(hash);
    let disk = ops.stamp(path).ok().flatten();
    let holds = disk.is_some_and(|d| {
        d.size == len
            && match expected {
                Some(hash) => ops.hash(path).ok() == Some(hash),
                None => committed.is_none_or(|c| c.id == d.id && c.volume == d.volume),
            }
    });
    let stamp = if holds { disk } else { committed };
    let baseline = stamp.map(|stamp| Baseline {
        stamp,
        hash: expected,
    });
    let warning = (!holds).then(|| {
        format!(
            "Saved, but {} changed again right after the save; the next save asks before overwriting it",
            path.display()
        )
    });
    (baseline, warning)
}

fn failed(error: String) -> SaveOutcome {
    SaveOutcome::Failed { error }
}

/// Runs the save sequence of the Architecture (Saving).
pub fn save(ops: &dyn FileOps, req: &SaveRequest) -> SaveOutcome {
    let path = req.path.as_path();
    let shown = path.display();
    let tmp = match free_sibling(ops, path, "tmp") {
        Ok(tmp) => tmp,
        Err(e) => return failed(format!("Cannot save {shown}: {e}")),
    };

    // 1. Temporary sibling, flushed.
    let mut written = (0u64, 0u64);
    let result = ops.write_new(&tmp, &mut |out| {
        let mut hashing = Hashing {
            inner: out,
            hasher: Xxh3::new(),
            len: 0,
        };
        write_content(&mut hashing, &req.snapshot, req.encoding)?;
        written = (hashing.len, hashing.hasher.digest());
        Ok(())
    });
    if let Err(e) = result {
        // `write_new` removed anything it created; other files are not ours.
        return failed(format!("Cannot save {shown}: {e}"));
    }
    let (len, hash) = written;
    let committed = ops.stamp(&tmp).ok().flatten();

    // 2. Pre-check against the baseline.
    let disk = match ops.stamp(path) {
        Ok(disk) => disk,
        Err(e) => {
            let _ = ops.delete(&tmp);
            return failed(format!("Cannot save {shown}: {e}"));
        }
    };
    let (Some(stamp), Some(baseline)) = (disk, req.baseline) else {
        if let Some(stamp) = disk {
            // A file appeared where none existed.
            let _ = ops.delete(&tmp);
            return match hash_if_small(ops, path, stamp.size) {
                Ok(hash) => SaveOutcome::Conflict {
                    disk: Baseline { stamp, hash },
                },
                Err(e) => failed(format!("Cannot save {shown}: {e}")),
            };
        }
        // Absent target: create it; never overwrite a file appearing meanwhile.
        return match ops.move_new(&tmp, path) {
            Ok(()) => {
                let (baseline, warning) = committed_baseline(ops, path, committed, len, hash);
                SaveOutcome::Saved {
                    snapshot: req.snapshot.clone(),
                    baseline,
                    warning,
                }
            }
            Err(e) => failed(format!(
                "Cannot create {shown}: {e}. The text was written to {}",
                tmp.display()
            )),
        };
    };
    match is_baseline(ops, path, &stamp, &baseline) {
        Ok(true) => {}
        Ok(false) => {
            let _ = ops.delete(&tmp);
            return match hash_if_small(ops, path, stamp.size) {
                Ok(hash) => SaveOutcome::Conflict {
                    disk: Baseline { stamp, hash },
                },
                Err(e) => failed(format!("Cannot save {shown}: {e}")),
            };
        }
        Err(e) => {
            let _ = ops.delete(&tmp);
            return failed(format!("Cannot save {shown}: {e}"));
        }
    }

    // 3. Atomic replacement with a backup.
    let backup = match free_sibling(ops, path, "bak") {
        Ok(backup) => backup,
        Err(e) => {
            let _ = ops.delete(&tmp);
            return failed(format!("Cannot save {shown}: {e}"));
        }
    };
    if let Err(code) = ops.replace(path, &tmp, &backup) {
        return replace_failed(ops, req, &tmp, &backup, &baseline, code);
    }

    // 4. Post-check: the backup must be the baseline version.
    let warning = match backup_is_baseline(ops, &backup, &baseline) {
        Ok(true) => ops.delete(&backup).err().map(|e| {
            format!(
                "Saved; the previous version could not be removed from {}: {e}",
                backup.display()
            )
        }),
        _ => Some(format!(
            "Saved; another program changed the file during the save. Its version is kept at {}",
            backup.display()
        )),
    };
    let (baseline, changed) = committed_baseline(ops, path, committed, len, hash);
    let warning = match (warning, changed) {
        (Some(a), Some(b)) => Some(format!("{a}. {b}")),
        (a, b) => a.or(b),
    };
    SaveOutcome::Saved {
        snapshot: req.snapshot.clone(),
        baseline,
        warning,
    }
}

/// Handles a failed `ReplaceFileW`. An artifact is deleted only after the
/// target is verified to still hold the baseline version.
fn replace_failed(
    ops: &dyn FileOps,
    req: &SaveRequest,
    tmp: &Path,
    backup: &Path,
    baseline: &Baseline,
    code: u32,
) -> SaveOutcome {
    let path = req.path.as_path();
    let target_intact = || {
        ops.stamp(path)
            .ok()
            .flatten()
            .is_some_and(|stamp| is_baseline(ops, path, &stamp, baseline).unwrap_or(false))
    };
    let kept = |paths: &[&Path]| -> String {
        let surviving: Vec<String> = paths
            .iter()
            .filter(|p| exists(ops, p))
            .map(|p| p.display().to_string())
            .collect();
        if surviving.is_empty() {
            String::new()
        } else {
            format!(" Kept: {}", surviving.join(", "))
        }
    };
    let shown = path.display();
    match code {
        ERROR_UNABLE_TO_REMOVE_REPLACED | ERROR_UNABLE_TO_MOVE_REPLACEMENT if target_intact() => {
            let _ = ops.delete(tmp);
            failed(format!(
                "Cannot replace {shown} (error {code}); the file is unchanged.{}",
                kept(&[tmp, backup])
            ))
        }
        ERROR_UNABLE_TO_MOVE_REPLACEMENT_2 => failed(format!(
            "Cannot replace {shown} (error {code}). The previous version is at {} and the new text at {}",
            backup.display(),
            tmp.display()
        )),
        _ => failed(format!(
            "Cannot replace {shown} (error {code}).{}",
            kept(&[path, tmp, backup])
        )),
    }
}

/// Saves on its own thread. `wake` runs after the outcome is sent and when
/// the thread ends (a disconnected receiver means the save stopped).
pub fn spawn(
    ops: Arc<dyn FileOps>,
    req: SaveRequest,
    wake: impl Fn() + Clone + Send + 'static,
) -> Receiver<SaveOutcome> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _final_wake = WakeOnDrop(wake.clone());
        let _ = tx.send(save(ops.as_ref(), &req));
        wake();
    });
    rx
}

#[cfg(test)]
pub mod fake {
    //! In-memory [`FileOps`] with injectable failures and hooks.

    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;

    #[derive(Clone, Debug)]
    pub struct FakeFile {
        pub bytes: Vec<u8>,
        pub stamp: FileStamp,
    }

    type Hook = Box<dyn Fn(&FakeFiles) + Send + Sync>;

    #[derive(Default)]
    pub struct FakeFiles {
        pub files: Mutex<HashMap<PathBuf, FakeFile>>,
        next: Mutex<u64>,
        /// Error code returned by `replace` (disk state as documented).
        pub replace_error: Mutex<Option<u32>>,
        /// Runs before `replace` / `move_new` (racing writers).
        pub before_commit: Mutex<Option<Hook>>,
        /// Runs after a successful `replace` / `move_new`.
        pub after_commit: Mutex<Option<Hook>>,
        /// Runs when `write_new` starts, before it creates anything.
        pub before_create: Mutex<Option<Hook>>,
        /// Error returned by `write_new` before it creates anything.
        pub create_error: Mutex<Option<io::ErrorKind>>,
        /// Blocks `write_new` until the receiver gets a message.
        pub gate: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
    }

    impl FakeFiles {
        /// Writes `bytes` as a new version (new time; new ID when created).
        pub fn put(&self, path: &Path, bytes: &[u8]) {
            let mut next = self.next.lock().unwrap();
            *next += 1;
            let mut files = self.files.lock().unwrap();
            let id = files.get(path).map_or(u128::from(*next), |f| f.stamp.id);
            files.insert(
                path.to_path_buf(),
                FakeFile {
                    bytes: bytes.to_vec(),
                    stamp: FileStamp {
                        volume: 7,
                        id,
                        size: bytes.len() as u64,
                        write_time: *next,
                    },
                },
            );
        }

        pub fn get(&self, path: &Path) -> Option<Vec<u8>> {
            self.files
                .lock()
                .unwrap()
                .get(path)
                .map(|f| f.bytes.clone())
        }

        pub fn names(&self) -> Vec<String> {
            let mut names: Vec<String> = self
                .files
                .lock()
                .unwrap()
                .keys()
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        }

        pub fn baseline(&self, path: &Path) -> Baseline {
            let files = self.files.lock().unwrap();
            let file = &files[path];
            Baseline {
                stamp: file.stamp,
                hash: Some(xxhash_rust::xxh3::xxh3_64(&file.bytes)),
            }
        }

        fn run_hook(&self, hook: &Mutex<Option<Hook>>) {
            let hook = hook.lock().unwrap().take();
            if let Some(hook) = hook {
                hook(self);
            }
        }
    }

    fn not_found() -> io::Error {
        io::Error::from(io::ErrorKind::NotFound)
    }

    impl FileOps for FakeFiles {
        fn stamp(&self, path: &Path) -> io::Result<Option<FileStamp>> {
            Ok(self.files.lock().unwrap().get(path).map(|f| f.stamp))
        }

        fn hash(&self, path: &Path) -> io::Result<u64> {
            self.get(path)
                .map(|b| xxhash_rust::xxh3::xxh3_64(&b))
                .ok_or_else(not_found)
        }

        fn write_new(
            &self,
            path: &Path,
            write: &mut dyn FnMut(&mut dyn Write) -> io::Result<()>,
        ) -> io::Result<()> {
            let gate = self.gate.lock().unwrap().take();
            if let Some(gate) = gate {
                let _ = gate.recv();
            }
            self.run_hook(&self.before_create);
            if let Some(kind) = *self.create_error.lock().unwrap() {
                return Err(io::Error::from(kind));
            }
            if self.files.lock().unwrap().contains_key(path) {
                return Err(io::Error::from(io::ErrorKind::AlreadyExists));
            }
            let mut bytes = Vec::new();
            write(&mut bytes)?;
            self.put(path, &bytes);
            Ok(())
        }

        fn replace(&self, target: &Path, replacement: &Path, backup: &Path) -> Result<(), u32> {
            self.run_hook(&self.before_commit);
            let error = *self.replace_error.lock().unwrap();
            let mut files = self.files.lock().unwrap();
            if !files.contains_key(target) || !files.contains_key(replacement) {
                return Err(2);
            }
            match error {
                None => {
                    let old = files.remove(target).unwrap();
                    let new = files.remove(replacement).unwrap();
                    files.insert(backup.to_path_buf(), old);
                    files.insert(target.to_path_buf(), new);
                    drop(files);
                    self.run_hook(&self.after_commit);
                    Ok(())
                }
                Some(ERROR_UNABLE_TO_MOVE_REPLACEMENT_2) => {
                    let old = files.remove(target).unwrap();
                    files.insert(backup.to_path_buf(), old);
                    Err(ERROR_UNABLE_TO_MOVE_REPLACEMENT_2)
                }
                Some(code) => Err(code),
            }
        }

        fn move_new(&self, from: &Path, to: &Path) -> io::Result<()> {
            self.run_hook(&self.before_commit);
            let mut files = self.files.lock().unwrap();
            if files.contains_key(to) {
                return Err(io::Error::from(io::ErrorKind::AlreadyExists));
            }
            let file = files.remove(from).ok_or_else(not_found)?;
            files.insert(to.to_path_buf(), file);
            drop(files);
            self.run_hook(&self.after_commit);
            Ok(())
        }

        fn delete(&self, path: &Path) -> io::Result<()> {
            self.files
                .lock()
                .unwrap()
                .remove(path)
                .map(|_| ())
                .ok_or_else(not_found)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeFiles;
    use super::*;

    fn target() -> PathBuf {
        PathBuf::from(r"C:\docs\a.txt")
    }

    fn request(files: &FakeFiles, text: &[u8]) -> SaveRequest {
        let exists = files.get(&target()).is_some();
        SaveRequest {
            path: target(),
            snapshot: Snapshot::from_bytes(text),
            encoding: Encoding::Utf8,
            baseline: exists.then(|| files.baseline(&target())),
        }
    }

    fn saved(outcome: SaveOutcome) -> (Option<Baseline>, Option<String>) {
        match outcome {
            SaveOutcome::Saved {
                baseline, warning, ..
            } => (baseline, warning),
            other => panic!("expected Saved, got {other:?}"),
        }
    }

    fn error(outcome: SaveOutcome) -> String {
        match outcome {
            SaveOutcome::Failed { error } => error,
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn replaces_and_cleans_up() {
        let files = FakeFiles::default();
        files.put(&target(), b"old");
        let (baseline, warning) = saved(save(&files, &request(&files, b"new")));
        assert_eq!(files.get(&target()).unwrap(), b"new");
        assert_eq!(files.names(), vec!["a.txt"]);
        assert_eq!(warning, None);
        assert_eq!(baseline, Some(files.baseline(&target())));
    }

    #[test]
    fn encodes_bom_and_utf16() {
        let files = FakeFiles::default();
        let mut req = request(&files, "h\u{4E2D}".as_bytes());
        req.encoding = Encoding::Utf16Be;
        saved(save(&files, &req));
        assert_eq!(
            files.get(&target()).unwrap(),
            [0xFE, 0xFF, 0, b'h', 0x4E, 0x2D]
        );
        files.delete(&target()).unwrap();
        req.encoding = Encoding::Utf8Bom;
        req.baseline = None;
        saved(save(&files, &req));
        assert_eq!(
            files.get(&target()).unwrap(),
            [0xEF, 0xBB, 0xBF, b'h', 0xE4, 0xB8, 0xAD]
        );
    }

    #[test]
    fn changed_file_is_a_conflict_and_untouched() {
        let files = FakeFiles::default();
        files.put(&target(), b"old");
        let req = request(&files, b"new");
        files.put(&target(), b"other!");
        let disk = files.baseline(&target());
        match save(&files, &req) {
            SaveOutcome::Conflict { disk: got } => assert_eq!(got, disk),
            other => panic!("{other:?}"),
        }
        assert_eq!(files.get(&target()).unwrap(), b"other!");
        assert_eq!(files.names(), vec!["a.txt"]);
    }

    #[test]
    fn same_stamp_different_content_is_detected_by_hash() {
        let files = FakeFiles::default();
        files.put(&target(), b"old");
        let req = request(&files, b"new");
        files
            .files
            .lock()
            .unwrap()
            .get_mut(&target())
            .unwrap()
            .bytes = b"OLD".to_vec();
        assert!(matches!(save(&files, &req), SaveOutcome::Conflict { .. }));
        assert_eq!(files.get(&target()).unwrap(), b"OLD");
    }

    #[test]
    fn writer_racing_the_commit_survives_as_backup() {
        let files = FakeFiles::default();
        files.put(&target(), b"old");
        let req = request(&files, b"new");
        *files.before_commit.lock().unwrap() =
            Some(Box::new(|f: &FakeFiles| f.put(&target(), b"racer")));
        let (_, warning) = saved(save(&files, &req));
        assert_eq!(files.get(&target()).unwrap(), b"new");
        let backup = target().with_file_name(format!("a.txt.~rte-{}.bak", std::process::id()));
        assert_eq!(files.get(&backup).unwrap(), b"racer");
        assert!(warning.unwrap().contains(&backup.display().to_string()));
    }

    #[test]
    fn absent_target_is_created_but_never_overwritten() {
        let files = FakeFiles::default();
        saved(save(&files, &request(&files, b"new")));
        assert_eq!(files.get(&target()).unwrap(), b"new");

        let files = FakeFiles::default();
        let req = request(&files, b"new");
        *files.before_commit.lock().unwrap() =
            Some(Box::new(|f: &FakeFiles| f.put(&target(), b"appeared")));
        let message = error(save(&files, &req));
        assert_eq!(files.get(&target()).unwrap(), b"appeared");
        let tmp = target().with_file_name(format!("a.txt.~rte-{}.tmp", std::process::id()));
        assert_eq!(files.get(&tmp).unwrap(), b"new");
        assert!(message.contains(&tmp.display().to_string()));

        // A file present at the pre-check is a conflict.
        let files = FakeFiles::default();
        let req = request(&files, b"new");
        files.put(&target(), b"appeared");
        assert!(matches!(save(&files, &req), SaveOutcome::Conflict { .. }));
    }

    #[test]
    fn writer_right_after_the_commit_is_never_adopted() {
        // Different size, and same size (different content) as the saved text.
        for (existing, external) in [
            (Some(&b"old"[..]), &b"external!"[..]),
            (Some(b"old"), b"XYZ"),
            (None, b"external!"),
        ] {
            let files = FakeFiles::default();
            if let Some(existing) = existing {
                files.put(&target(), existing);
            }
            let req = request(&files, b"new");
            *files.after_commit.lock().unwrap() =
                Some(Box::new(move |f: &FakeFiles| f.put(&target(), external)));
            let (baseline, warning) = saved(save(&files, &req));
            assert!(warning.unwrap().contains("changed again"));
            assert_eq!(
                baseline.unwrap().hash,
                Some(xxhash_rust::xxh3::xxh3_64(b"new"))
            );
            let next = SaveRequest {
                baseline,
                ..request(&files, b"newer")
            };
            assert!(
                matches!(save(&files, &next), SaveOutcome::Conflict { .. }),
                "{external:?}"
            );
            assert_eq!(files.get(&target()).unwrap(), external);
        }
    }

    #[test]
    fn temporary_names_taken_by_others_are_never_deleted() {
        let tmp = target().with_file_name(format!("a.txt.~rte-{}.tmp", std::process::id()));
        for create_error in [None, Some(io::ErrorKind::PermissionDenied)] {
            let files = FakeFiles::default();
            files.put(&target(), b"old");
            let req = request(&files, b"new");
            let other = tmp.clone();
            *files.before_create.lock().unwrap() =
                Some(Box::new(move |f: &FakeFiles| f.put(&other, b"not ours")));
            *files.create_error.lock().unwrap() = create_error;
            error(save(&files, &req));
            assert_eq!(files.get(&tmp).unwrap(), b"not ours", "{create_error:?}");
            assert_eq!(files.get(&target()).unwrap(), b"old");
        }
    }

    #[test]
    fn system_write_new_removes_only_what_it_created() {
        let dir = std::env::temp_dir().join(format!("rte_write_new_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let theirs = dir.join("theirs");
        std::fs::write(&theirs, b"theirs").unwrap();
        let err = SystemFiles
            .write_new(&theirs, &mut |w| w.write_all(b"ours"))
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&theirs).unwrap(), b"theirs");
        let fresh = dir.join("fresh");
        SystemFiles
            .write_new(&fresh, &mut |w| {
                w.write_all(b"partial")?;
                Err(io::Error::other("disk full"))
            })
            .unwrap_err();
        assert!(!fresh.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn deleted_target_is_recreated() {
        let files = FakeFiles::default();
        files.put(&target(), b"old");
        let req = request(&files, b"new");
        files.delete(&target()).unwrap();
        saved(save(&files, &req));
        assert_eq!(files.get(&target()).unwrap(), b"new");
    }

    #[test]
    fn replace_failures_keep_every_version() {
        let pid = std::process::id();
        let tmp = format!("a.txt.~rte-{pid}.tmp");
        let bak = format!("a.txt.~rte-{pid}.bak");
        for code in [1175, 1176] {
            let files = FakeFiles::default();
            files.put(&target(), b"old");
            *files.replace_error.lock().unwrap() = Some(code);
            error(save(&files, &request(&files, b"new")));
            assert_eq!(files.get(&target()).unwrap(), b"old");
            assert_eq!(files.names(), vec!["a.txt"], "code {code}");
        }
        // 1176 with a target that is no longer the baseline: nothing is deleted.
        let files = FakeFiles::default();
        files.put(&target(), b"old");
        *files.replace_error.lock().unwrap() = Some(1176);
        let req = request(&files, b"new");
        *files.before_commit.lock().unwrap() =
            Some(Box::new(|f: &FakeFiles| f.put(&target(), b"racer")));
        let message = error(save(&files, &req));
        assert_eq!(files.names(), vec!["a.txt".to_owned(), tmp.clone()]);
        assert!(message.contains(&tmp));

        let files = FakeFiles::default();
        files.put(&target(), b"old");
        *files.replace_error.lock().unwrap() = Some(1177);
        let message = error(save(&files, &request(&files, b"new")));
        assert_eq!(files.names(), vec![bak.clone(), tmp.clone()]);
        assert!(message.contains(&bak) && message.contains(&tmp));

        let files = FakeFiles::default();
        files.put(&target(), b"old");
        *files.replace_error.lock().unwrap() = Some(5);
        let message = error(save(&files, &request(&files, b"new")));
        assert_eq!(files.names(), vec!["a.txt".to_owned(), tmp.clone()]);
        assert!(message.contains(&tmp));
    }

    #[test]
    fn real_files_round_trip_byte_identical() {
        let dir = std::env::temp_dir().join(format!("rte_save_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let fixtures: [(&str, &[u8]); 9] = [
            ("lf", b"a\nb\n"),
            ("crlf", b"a\r\nb\r\n"),
            ("mixed", b"a\r\nb\nc\rd"),
            ("lone_cr", b"a\rb\r"),
            ("bom", b"\xEF\xBB\xBFx\r\n"),
            ("utf16le", b"\xFF\xFEh\0i\0\n\0"),
            ("utf16be", b"\xFE\xFF\0h\0i\0\n"),
            ("invalid", b"a\xFF\xFEb\xC3"),
            ("no_newline", b"last line"),
        ];
        for (name, bytes) in fixtures {
            let path = dir.join(name);
            std::fs::write(&path, bytes).unwrap();
            let (tx, rx) = mpsc::channel();
            crate::loader::load(path.clone(), tx, || {});
            let Some(crate::loader::LoadEvent::Done {
                snapshot,
                info,
                baseline,
                ..
            }) = rx.try_iter().last()
            else {
                panic!("{name} did not load");
            };
            let outcome = save(
                &SystemFiles,
                &SaveRequest {
                    path: path.clone(),
                    snapshot,
                    encoding: info.encoding,
                    baseline,
                },
            );
            let (baseline, warning) = saved(outcome);
            assert_eq!(warning, None, "{name}");
            assert_eq!(std::fs::read(&path).unwrap(), bytes, "{name}");
            assert_eq!(
                baseline.unwrap().stamp,
                files::stamp(&path).unwrap().unwrap()
            );
        }
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), fixtures.len());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
