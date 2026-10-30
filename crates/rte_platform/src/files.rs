//! File identity and the atomic replacement primitives used by saving.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::ptr::null;

use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_READONLY, FILE_ID_INFO, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, FileIdInfo, GetFileAttributesW,
    GetFileInformationByHandleEx, INVALID_FILE_ATTRIBUTES, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    REPLACEFILE_IGNORE_MERGE_ERRORS, ReplaceFileW,
};

/// Identity and version of a file: volume serial and 128-bit file ID
/// (`FILE_ID_INFO`), size and last-write time (FILETIME ticks).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileStamp {
    pub volume: u64,
    pub id: u128,
    pub size: u64,
    pub write_time: u64,
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// Stamp of an open file.
pub fn stamp_of(file: &File) -> io::Result<FileStamp> {
    let mut info = FILE_ID_INFO {
        VolumeSerialNumber: 0,
        FileId: Default::default(),
    };
    // SAFETY: the handle is owned by `file`; `info` is a live, correctly sized buffer.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileIdInfo,
            (&mut info as *mut FILE_ID_INFO).cast(),
            size_of::<FILE_ID_INFO>() as u32,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    let meta = file.metadata()?;
    Ok(FileStamp {
        volume: info.VolumeSerialNumber,
        id: u128::from_le_bytes(info.FileId.Identifier),
        size: meta.len(),
        write_time: meta.last_write_time(),
    })
}

/// Stamp of the file at `path`, or `None` when it does not exist. Opening
/// only reads attributes and shares everything, so it never blocks writers.
pub fn stamp(path: &Path) -> io::Result<Option<FileStamp>> {
    let file = OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(path);
    match file {
        Ok(file) => stamp_of(&file).map(Some),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// The read-only attribute of `path`; `false` when it does not exist.
pub fn is_read_only(path: &Path) -> io::Result<bool> {
    let path = wide(path);
    // SAFETY: NUL-terminated path that outlives the call.
    let attributes = unsafe { GetFileAttributesW(path.as_ptr()) };
    if attributes == INVALID_FILE_ATTRIBUTES {
        let e = io::Error::last_os_error();
        return if e.kind() == io::ErrorKind::NotFound {
            Ok(false)
        } else {
            Err(e)
        };
    }
    Ok(attributes & FILE_ATTRIBUTE_READONLY != 0)
}

/// `ReplaceFileW(target, replacement, backup)`: the old target becomes
/// `backup`, `replacement` takes the target's name. Returns the Win32 error
/// code on failure (see the Architecture's failure table).
pub fn replace(target: &Path, replacement: &Path, backup: &Path) -> Result<(), u32> {
    let (target, replacement, backup) = (wide(target), wide(replacement), wide(backup));
    // SAFETY: NUL-terminated paths that outlive the call; reserved pointers are null.
    let ok = unsafe {
        ReplaceFileW(
            target.as_ptr(),
            replacement.as_ptr(),
            backup.as_ptr(),
            REPLACEFILE_IGNORE_MERGE_ERRORS,
            null(),
            null(),
        )
    };
    if ok == 0 {
        // SAFETY: plain Win32 call.
        Err(unsafe { GetLastError() })
    } else {
        Ok(())
    }
}

/// Moves `from` to `to` with write-through; fails when `to` exists.
pub fn move_new(from: &Path, to: &Path) -> io::Result<()> {
    let (from, to) = (wide(from), wide(to));
    // SAFETY: NUL-terminated paths that outlive the call.
    let ok = unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rte_files_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn stamps_identify_and_version_files() {
        let dir = temp_dir("stamp");
        let path = dir.join("a.txt");
        assert_eq!(stamp(&path).unwrap(), None);
        std::fs::write(&path, b"one").unwrap();
        let first = stamp(&path).unwrap().unwrap();
        assert_eq!(first.size, 3);
        assert_eq!(stamp_of(&File::open(&path).unwrap()).unwrap(), first);
        std::fs::write(&path, b"three").unwrap();
        let second = stamp(&path).unwrap().unwrap();
        assert_eq!((second.volume, second.id), (first.volume, first.id));
        assert_eq!(second.size, 5);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn replace_keeps_the_old_file_as_backup() {
        let dir = temp_dir("replace");
        let (target, tmp, backup) = (dir.join("t"), dir.join("t.tmp"), dir.join("t.bak"));
        std::fs::write(&target, b"old").unwrap();
        std::fs::write(&tmp, b"new").unwrap();
        let old = stamp(&target).unwrap().unwrap();
        replace(&target, &tmp, &backup).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert_eq!(std::fs::read(&backup).unwrap(), b"old");
        assert!(!tmp.exists());
        let kept = stamp(&backup).unwrap().unwrap();
        assert_eq!((kept.size, kept.write_time), (old.size, old.write_time));
        assert!(replace(&dir.join("missing"), &target, &backup).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn read_only_attribute() {
        let dir = temp_dir("read_only");
        let path = dir.join("a.txt");
        assert!(!is_read_only(&path).unwrap());
        std::fs::write(&path, b"a").unwrap();
        assert!(!is_read_only(&path).unwrap());
        let original = std::fs::metadata(&path).unwrap().permissions();
        let mut permissions = original.clone();
        permissions.set_readonly(true);
        std::fs::set_permissions(&path, permissions).unwrap();
        assert!(is_read_only(&path).unwrap());
        std::fs::set_permissions(&path, original).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn move_new_never_overwrites() {
        let dir = temp_dir("move");
        let (a, b) = (dir.join("a"), dir.join("b"));
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        assert!(move_new(&a, &b).is_err());
        assert_eq!(std::fs::read(&b).unwrap(), b"b");
        std::fs::remove_file(&b).unwrap();
        move_new(&a, &b).unwrap();
        assert_eq!(std::fs::read(&b).unwrap(), b"a");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
