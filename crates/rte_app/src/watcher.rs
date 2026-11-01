//! Changes of the open file by other programs: a thread waits for change
//! notifications of the file's folder and reports the file's stamp when it
//! differs from the last report. All of its I/O runs on that thread.

use std::io;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

use rte_platform::files::{FileStamp, FolderWatch, Poke, Signal};

use crate::editor::Wake;
use crate::save::FileOps;

/// Change notifications of a folder (see [`FolderWatch`]).
pub trait Watch {
    /// Blocks until the folder changes or `poke` is set.
    fn wait(&mut self, poke: &Poke) -> io::Result<Signal>;
}

impl Watch for FolderWatch {
    fn wait(&mut self, poke: &Poke) -> io::Result<Signal> {
        FolderWatch::wait(self, poke)
    }
}

/// The file's state on disk (`None`: absent), read after `epoch` was.
#[derive(Clone, Copy, Debug)]
pub struct Report {
    pub epoch: u64,
    pub stamp: Option<FileStamp>,
}

/// Owned by the editor and the thread; the event outlives both.
struct Shared {
    poke: Poke,
    stop: AtomicBool,
    epoch: AtomicU64,
}

/// What [`Watcher::poll`] found.
pub struct Polled {
    /// The latest report of the current epoch.
    pub latest: Option<Option<FileStamp>>,
    /// The thread ended (no notifications, or a failed wait).
    pub ended: bool,
}

/// The editor's side of a watcher thread; dropping it stops the thread.
pub struct Watcher {
    rx: Receiver<Report>,
    shared: Arc<Shared>,
}

impl Watcher {
    /// Watches `path` (relative paths resolve against the working
    /// directory; the file need not exist).
    pub fn spawn(path: &Path, files: Arc<dyn FileOps>, wake: Wake) -> io::Result<Watcher> {
        let (tx, watcher) = Watcher::channel()?;
        let shared = watcher.shared.clone();
        let path = path.to_path_buf();
        thread::Builder::new()
            .name("watcher".to_owned())
            .stack_size(64 * 1024)
            .spawn(move || {
                run(&path, files.as_ref(), &shared, tx, wake.as_ref());
                // The channel is closed: the editor sees the end.
                wake();
            })?;
        Ok(watcher)
    }

    fn channel() -> io::Result<(Sender<Report>, Watcher)> {
        let shared = Arc::new(Shared {
            poke: Poke::new()?,
            stop: AtomicBool::new(false),
            epoch: AtomicU64::new(0),
        });
        let (tx, rx) = mpsc::channel();
        Ok((tx, Watcher { rx, shared }))
    }

    /// A watcher without a thread: tests send its reports.
    #[cfg(test)]
    pub fn fake() -> (Sender<Report>, Watcher) {
        Watcher::channel().expect("event")
    }

    pub fn epoch(&self) -> u64 {
        self.shared.epoch.load(Ordering::Acquire)
    }

    /// Drops the reports read so far and asks for a fresh one (after the
    /// editor wrote the file itself).
    pub fn refresh(&self) {
        self.shared.epoch.fetch_add(1, Ordering::AcqRel);
        self.shared.poke.poke();
    }

    pub fn poll(&self) -> Polled {
        let epoch = self.epoch();
        let mut polled = Polled {
            latest: None,
            ended: false,
        };
        loop {
            match self.rx.try_recv() {
                Ok(report) if report.epoch == epoch => polled.latest = Some(report.stamp),
                Ok(_) => {}
                Err(TryRecvError::Empty) => return polled,
                Err(TryRecvError::Disconnected) => {
                    polled.ended = true;
                    return polled;
                }
            }
        }
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        self.shared.poke.poke();
    }
}

/// The thread: registers the watch, reports once, then after every change
/// of the file's state and after every refresh. A failed stamp read is not
/// reported; the next signal reads again.
fn run(
    path: &Path,
    files: &dyn FileOps,
    shared: &Shared,
    tx: Sender<Report>,
    wake: &(dyn Fn() + Send + Sync),
) {
    let Ok(path) = std::path::absolute(path) else {
        return;
    };
    let Some(folder) = path.parent() else {
        return;
    };
    let Ok(mut watch) = files.watch(folder) else {
        return;
    };
    let mut last = None;
    let mut forced = true;
    loop {
        if shared.stop.load(Ordering::Acquire) {
            return;
        }
        let epoch = shared.epoch.load(Ordering::Acquire);
        if let Ok(stamp) = files.stamp(&path)
            && (forced || last != Some(stamp))
        {
            last = Some(stamp);
            if tx.send(Report { epoch, stamp }).is_err() {
                return;
            }
            wake();
        }
        forced = match watch.wait(&shared.poke) {
            Ok(Signal::Changed) => false,
            Ok(Signal::Poked) => true,
            Err(_) => return,
        };
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::io::Write;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::save::SystemFiles;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rte_watch_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The reports of the current epoch until `done` holds for the last one.
    fn until(watcher: &Watcher, done: impl Fn(Option<FileStamp>) -> bool) -> Option<FileStamp> {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Some(stamp) = watcher.poll().latest
                && done(stamp)
            {
                return stamp;
            }
            thread::sleep(Duration::from_millis(1));
        }
        panic!("no such report");
    }

    fn ended(watcher: &Watcher) -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if watcher.poll().ended {
                return true;
            }
            thread::sleep(Duration::from_millis(1));
        }
        false
    }

    fn system() -> Arc<dyn FileOps> {
        Arc::new(SystemFiles)
    }

    #[test]
    fn writes_replacements_and_deletions_are_reported() {
        let dir = temp_dir("report");
        let path = dir.join("a.txt");
        std::fs::write(&path, b"one").unwrap();
        let first = rte_platform::files::stamp(&path).unwrap();
        let watcher = Watcher::spawn(&path, system(), Arc::new(|| {})).unwrap();
        assert_eq!(until(&watcher, |_| true), first, "the initial report");
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b" two")
            .unwrap();
        assert_eq!(
            until(&watcher, |s| s.is_some_and(|s| s.size == 7))
                .unwrap()
                .size,
            7
        );
        std::fs::write(dir.join("a.tmp"), b"three!").unwrap();
        std::fs::rename(dir.join("a.tmp"), &path).unwrap();
        until(&watcher, |s| s.is_some_and(|s| s.size == 6));
        std::fs::remove_file(&path).unwrap();
        until(&watcher, |s| s.is_none());
        drop(watcher);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn other_files_wake_nothing_and_stopping_is_prompt() {
        let dir = temp_dir("busy");
        let path = dir.join("a.txt");
        std::fs::write(&path, b"one").unwrap();
        let wakes = Arc::new(AtomicU64::new(0));
        let counter = wakes.clone();
        let watcher = Watcher::spawn(
            &path,
            system(),
            Arc::new(move || {
                counter.fetch_add(1, Ordering::Relaxed);
            }),
        )
        .unwrap();
        until(&watcher, |_| true);
        let before = wakes.load(Ordering::Relaxed);
        for k in 0..1000 {
            std::fs::write(dir.join("other.log"), k.to_string()).unwrap();
        }
        // A last change of the file itself, replaced at once: every earlier
        // report would be older.
        std::fs::write(dir.join("a.tmp"), b"two!").unwrap();
        std::fs::rename(dir.join("a.tmp"), &path).unwrap();
        until(&watcher, |s| s.is_some_and(|s| s.size == 4));
        assert_eq!(
            wakes.load(Ordering::Relaxed),
            before + 1,
            "only the file's change"
        );
        let started = Instant::now();
        drop(watcher);
        // The thread wakes once more when it ends.
        let deadline = Instant::now() + Duration::from_secs(10);
        while wakes.load(Ordering::Relaxed) == before + 1 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(wakes.load(Ordering::Relaxed), before + 2);
        assert!(started.elapsed() < Duration::from_secs(1));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn relative_and_missing_files_are_watched_in_their_folder() {
        let dir = temp_dir("relative");
        let path = dir.join("later.txt");
        let watcher = Watcher::spawn(&path, system(), Arc::new(|| {})).unwrap();
        assert_eq!(until(&watcher, |_| true), None, "not created yet");
        std::fs::write(&path, b"now").unwrap();
        until(&watcher, |s| s.is_some());
        // A bare name and a relative subfolder resolve against the working
        // directory (the crate folder under cargo test).
        let cwd = std::env::current_dir().unwrap();
        for relative in [Path::new("Cargo.toml"), Path::new("src/main.rs")] {
            let watcher = Watcher::spawn(relative, system(), Arc::new(|| {})).unwrap();
            let expected = rte_platform::files::stamp(&cwd.join(relative)).unwrap();
            assert!(expected.is_some());
            assert_eq!(
                until(&watcher, |_| true),
                expected,
                "{}",
                relative.display()
            );
        }
        drop(watcher);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_refresh_drops_older_reports() {
        let dir = temp_dir("refresh");
        let path = dir.join("a.txt");
        std::fs::write(&path, b"one").unwrap();
        let watcher = Watcher::spawn(&path, system(), Arc::new(|| {})).unwrap();
        let first = until(&watcher, |_| true);
        // A report of the old epoch still queued is dropped; the refresh
        // reports the same, unchanged state again.
        watcher.refresh();
        assert_eq!(until(&watcher, |_| true), first);
        drop(watcher);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Scripted stamps and signals; `None` signals end the thread.
    struct Scripted {
        stamps: Mutex<VecDeque<io::Result<Option<FileStamp>>>>,
        signals: Arc<Mutex<VecDeque<io::Result<Signal>>>>,
        watchable: bool,
    }

    struct ScriptedWatch(Arc<Mutex<VecDeque<io::Result<Signal>>>>);

    impl Watch for ScriptedWatch {
        fn wait(&mut self, _: &Poke) -> io::Result<Signal> {
            self.0
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err(io::Error::other("script ended")))
        }
    }

    impl FileOps for Scripted {
        fn stamp(&self, _: &Path) -> io::Result<Option<FileStamp>> {
            self.stamps.lock().unwrap().pop_front().unwrap_or(Ok(None))
        }
        fn read_only(&self, _: &Path) -> io::Result<bool> {
            unreachable!()
        }
        fn hash(&self, _: &Path) -> io::Result<u64> {
            unreachable!()
        }
        fn write_new(
            &self,
            _: &Path,
            _: &mut dyn FnMut(&mut dyn Write) -> io::Result<()>,
        ) -> io::Result<()> {
            unreachable!()
        }
        fn replace(&self, _: &Path, _: &Path, _: &Path) -> Result<(), u32> {
            unreachable!()
        }
        fn move_new(&self, _: &Path, _: &Path) -> io::Result<()> {
            unreachable!()
        }
        fn delete(&self, _: &Path) -> io::Result<()> {
            unreachable!()
        }
        fn watch(&self, _: &Path) -> io::Result<Box<dyn Watch>> {
            if self.watchable {
                Ok(Box::new(ScriptedWatch(self.signals.clone())))
            } else {
                Err(io::Error::from(io::ErrorKind::Unsupported))
            }
        }
    }

    fn stamp(size: u64) -> FileStamp {
        FileStamp {
            volume: 1,
            id: 2,
            size,
            write_time: size,
        }
    }

    #[test]
    fn stamp_errors_are_skipped_and_failures_end_the_thread() {
        let files = Arc::new(Scripted {
            stamps: Mutex::new(VecDeque::from([
                Ok(Some(stamp(1))),
                Err(io::Error::from(io::ErrorKind::PermissionDenied)),
                Ok(Some(stamp(2))),
            ])),
            signals: Arc::new(Mutex::new(VecDeque::from([
                Ok(Signal::Changed),
                Ok(Signal::Changed),
                Err(io::Error::other("re-arm failed")),
            ]))),
            watchable: true,
        });
        let watcher = Watcher::spawn(Path::new(r"C:\docs\a.txt"), files, Arc::new(|| {})).unwrap();
        // The error is neither a deletion nor a change: the next read reports.
        until(&watcher, |s| s == Some(stamp(2)));
        assert!(ended(&watcher));

        let unsupported = Arc::new(Scripted {
            stamps: Mutex::new(VecDeque::new()),
            signals: Arc::new(Mutex::new(VecDeque::new())),
            watchable: false,
        });
        let watcher =
            Watcher::spawn(Path::new(r"C:\docs\a.txt"), unsupported, Arc::new(|| {})).unwrap();
        assert!(ended(&watcher));
        assert!(watcher.poll().latest.is_none(), "silent");
    }

    #[test]
    fn stopping_before_the_first_wait_and_poking_an_ended_thread_are_safe() {
        struct Never(Arc<AtomicBool>);
        impl Watch for Never {
            fn wait(&mut self, _: &Poke) -> io::Result<Signal> {
                self.0.store(true, Ordering::Release);
                Err(io::Error::other("waited after the stop"))
            }
        }
        struct Slow(Arc<Mutex<()>>, Arc<AtomicBool>);
        impl FileOps for Slow {
            fn stamp(&self, _: &Path) -> io::Result<Option<FileStamp>> {
                Ok(None)
            }
            fn read_only(&self, _: &Path) -> io::Result<bool> {
                unreachable!()
            }
            fn hash(&self, _: &Path) -> io::Result<u64> {
                unreachable!()
            }
            fn write_new(
                &self,
                _: &Path,
                _: &mut dyn FnMut(&mut dyn Write) -> io::Result<()>,
            ) -> io::Result<()> {
                unreachable!()
            }
            fn replace(&self, _: &Path, _: &Path, _: &Path) -> Result<(), u32> {
                unreachable!()
            }
            fn move_new(&self, _: &Path, _: &Path) -> io::Result<()> {
                unreachable!()
            }
            fn delete(&self, _: &Path) -> io::Result<()> {
                unreachable!()
            }
            fn watch(&self, _: &Path) -> io::Result<Box<dyn Watch>> {
                // Registration finishes after the editor stopped the watcher.
                drop(self.0.lock().unwrap());
                Ok(Box::new(Never(self.1.clone())))
            }
        }
        let gate = Arc::new(Mutex::new(()));
        let waited = Arc::new(AtomicBool::new(false));
        let held = gate.lock().unwrap();
        let watcher = Watcher::spawn(
            Path::new(r"C:\docs\a.txt"),
            Arc::new(Slow(gate.clone(), waited.clone())),
            Arc::new(|| {}),
        )
        .unwrap();
        let shared = watcher.shared.clone();
        drop(watcher);
        drop(held);
        let deadline = Instant::now() + Duration::from_secs(10);
        while Arc::strong_count(&shared) > 1 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(Arc::strong_count(&shared), 1, "the thread ended");
        assert!(!waited.load(Ordering::Acquire), "no wait after the stop");
        // The event lives while any owner holds it: a late poke is harmless.
        shared.poke.poke();
    }
}
