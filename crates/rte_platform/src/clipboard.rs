//! Clipboard: Unicode text plus one private data format.

use std::ptr::{copy_nonoverlapping, null_mut};

use windows_sys::Win32::Foundation::{GlobalFree, HANDLE, HWND};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, RegisterClipboardFormatW,
    SetClipboardData,
};
use windows_sys::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};

use crate::wide;

const CF_UNICODETEXT: u32 = 13;

/// Opens the clipboard, retrying briefly while another process holds it
/// (at most 20 ms of waiting on the calling thread).
fn open(hwnd: HWND) -> bool {
    for attempt in 0..5 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        // SAFETY: plain Win32 call with a valid window handle.
        if unsafe { OpenClipboard(hwnd) } != 0 {
            return true;
        }
    }
    false
}

fn format_id(name: &str) -> u32 {
    // SAFETY: NUL-terminated string that outlives the call.
    unsafe { RegisterClipboardFormatW(wide(name).as_ptr()) }
}

/// Bytes of a global memory handle.
unsafe fn global_bytes(handle: HANDLE) -> Option<Vec<u8>> {
    // SAFETY: the handle comes from GetClipboardData and stays valid while
    // the clipboard is open; the lock is released before returning.
    unsafe {
        let ptr = GlobalLock(handle).cast::<u8>();
        if ptr.is_null() {
            return None;
        }
        let size = GlobalSize(handle);
        let bytes = std::slice::from_raw_parts(ptr, size).to_vec();
        GlobalUnlock(handle);
        Some(bytes)
    }
}

/// A new movable global block of `len` bytes filled by `fill`.
unsafe fn global_with(len: usize, fill: impl FnOnce(*mut u8)) -> HANDLE {
    // SAFETY: the block is allocated with at least `len` bytes (suitably
    // aligned for any type) and unlocked before being handed to the clipboard.
    unsafe {
        let handle = GlobalAlloc(GMEM_MOVEABLE, len.max(1));
        if handle.is_null() {
            return null_mut();
        }
        let ptr = GlobalLock(handle).cast::<u8>();
        if ptr.is_null() {
            GlobalFree(handle);
            return null_mut();
        }
        fill(ptr);
        GlobalUnlock(handle);
        handle
    }
}

/// NUL-terminated UTF-16 text of a clipboard block, decoded in place.
unsafe fn global_text(handle: HANDLE) -> Option<String> {
    // SAFETY: as in `global_bytes`; global blocks are aligned for `u16`.
    unsafe {
        let ptr = GlobalLock(handle).cast::<u16>();
        if ptr.is_null() {
            return None;
        }
        let units = std::slice::from_raw_parts(ptr, GlobalSize(handle) / 2);
        let len = units.iter().position(|&u| u == 0).unwrap_or(units.len());
        let text = String::from_utf16_lossy(&units[..len]);
        GlobalUnlock(handle);
        Some(text)
    }
}

pub(crate) fn read(hwnd: HWND, format: &str) -> Option<(String, Option<Vec<u8>>)> {
    let private = format_id(format);
    if !open(hwnd) {
        return None;
    }
    // SAFETY: the clipboard is open; handles are only used before closing it.
    unsafe {
        let text = global_text(GetClipboardData(CF_UNICODETEXT));
        let data = if private == 0 {
            None
        } else {
            let handle = GetClipboardData(private);
            if handle.is_null() {
                None
            } else {
                global_bytes(handle)
            }
        };
        CloseClipboard();
        text.map(|text| (text, data))
    }
}

pub(crate) fn write(hwnd: HWND, text: &str, format: &str, data: &[u8]) -> bool {
    let private = format_id(format);
    let units = text.encode_utf16().count() + 1;
    if !open(hwnd) {
        return false;
    }
    // SAFETY: the clipboard is open and emptied by this window; ownership of
    // each block passes to the clipboard on success and is freed otherwise.
    unsafe {
        EmptyClipboard();
        let set = |format: u32, handle: HANDLE| {
            if handle.is_null() {
                return false;
            }
            if SetClipboardData(format, handle).is_null() {
                GlobalFree(handle);
                return false;
            }
            true
        };
        let text_block = global_with(units * 2, |ptr| {
            let dst = ptr.cast::<u16>();
            for (i, unit) in text.encode_utf16().chain(std::iter::once(0)).enumerate() {
                dst.add(i).write(unit);
            }
        });
        let ok = set(CF_UNICODETEXT, text_block);
        if ok && private != 0 {
            let block = global_with(data.len(), |ptr| {
                copy_nonoverlapping(data.as_ptr(), ptr, data.len());
            });
            set(private, block);
        }
        CloseClipboard();
        ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Window, WindowOptions};

    /// Uses the system clipboard and replaces its content (restoring only
    /// text). Run explicitly: `cargo test -p rte_platform -- --ignored`.
    #[test]
    #[ignore = "replaces the user's clipboard"]
    fn native_round_trip() {
        let window = Window::create(&WindowOptions {
            title: "clipboard test",
            width: 100,
            height: 100,
        })
        .unwrap();
        let hwnd = window.hwnd;
        let saved = read(hwnd, "RoyiTextEditor.Test");
        let text = "line \u{4E2D} \u{1F600}\r\nnext";
        assert!(write(hwnd, text, "RoyiTextEditor.Test", b"RTE1-data"));
        let (got, data) = read(hwnd, "RoyiTextEditor.Test").unwrap();
        assert_eq!(got, text);
        assert!(data.unwrap().starts_with(b"RTE1-data"));
        if let Some((text, _)) = saved {
            write(hwnd, &text, "RoyiTextEditor.Test", &[]);
        }
    }
}
