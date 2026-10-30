//! The Windows Spell Checking API (`ISpellChecker`), loaded at run time.
//!
//! `ole32.dll` is loaded from System32 when a checker is created, so the
//! executable imports nothing new and nothing runs at startup. Only `Check`
//! and `Suggest` are called: `Add` and `Ignore` would write the user's
//! Windows dictionary. COM is initialized (multithreaded) by [`new`] and
//! uninitialized when the checker is dropped, on the same thread.
//!
//! [`new`]: WindowsSpeller::new

use std::ffi::c_void;
use std::ptr::null_mut;

use windows_sys::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows_sys::core::GUID;

type HResult = i32;
const S_OK: HResult = 0;
const S_FALSE: HResult = 1;
const COINIT_MULTITHREADED: u32 = 0;
const CLSCTX_INPROC_SERVER: u32 = 1;

const CLSID_SPELL_CHECKER_FACTORY: GUID = GUID::from_u128(0x7AB36653_1796_484B_BDFA_E74F1DB7C1DC);
const IID_ISPELL_CHECKER_FACTORY: GUID = GUID::from_u128(0x8E018A9D_2415_4677_BF08_794EA61F94BB);

type CoInitializeEx = unsafe extern "system" fn(*const c_void, u32) -> HResult;
type CoUninitialize = unsafe extern "system" fn();
type CoCreateInstance = unsafe extern "system" fn(
    *const GUID,
    *mut c_void,
    u32,
    *const GUID,
    *mut *mut c_void,
) -> HResult;
type CoTaskMemFree = unsafe extern "system" fn(*const c_void);

/// The first entries of a COM vtable: `IUnknown`.
#[repr(C)]
struct Unknown {
    query_interface: usize,
    add_ref: usize,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
}

#[repr(C)]
struct FactoryVtbl {
    unknown: Unknown,
    get_supported_languages: usize,
    is_supported: unsafe extern "system" fn(*mut c_void, *const u16, *mut i32) -> HResult,
    create_spell_checker:
        unsafe extern "system" fn(*mut c_void, *const u16, *mut *mut c_void) -> HResult,
}

#[repr(C)]
struct CheckerVtbl {
    unknown: Unknown,
    get_language_tag: usize,
    check: unsafe extern "system" fn(*mut c_void, *const u16, *mut *mut c_void) -> HResult,
    suggest: unsafe extern "system" fn(*mut c_void, *const u16, *mut *mut c_void) -> HResult,
}

#[repr(C)]
struct EnumErrorsVtbl {
    unknown: Unknown,
    next: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HResult,
}

#[repr(C)]
struct EnumStringVtbl {
    unknown: Unknown,
    next: unsafe extern "system" fn(*mut c_void, u32, *mut *mut u16, *mut u32) -> HResult,
}

/// Calls `Release` on a COM object.
///
/// # Safety
/// `object` is a live COM interface pointer owned by the caller.
unsafe fn release(object: *mut c_void) {
    if !object.is_null() {
        // SAFETY: every COM vtable starts with IUnknown.
        unsafe { ((*(*(object as *mut *const Unknown))).release)(object) };
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A Windows spell checker for one language.
pub struct WindowsSpeller {
    checker: *mut c_void,
    free: CoTaskMemFree,
    uninitialize: CoUninitialize,
}

// SAFETY: created in a multithreaded apartment; COM allows its interface
// pointers on any MTA thread, and the spelling thread is the only user.
unsafe impl Send for WindowsSpeller {}

impl WindowsSpeller {
    /// Loads COM and creates the checker for `language` (for example
    /// `en-US`); an error names what failed.
    pub fn new(language: &str) -> Result<WindowsSpeller, String> {
        let name = wide("ole32.dll");
        // SAFETY: a valid, NUL-terminated name; System32 only.
        let module =
            unsafe { LoadLibraryExW(name.as_ptr(), null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32) };
        if module.is_null() {
            return Err("ole32.dll could not be loaded".to_owned());
        }
        let proc = |symbol: &[u8]| {
            // SAFETY: a loaded module and a NUL-terminated name.
            unsafe { GetProcAddress(module, symbol.as_ptr()) }
                .ok_or_else(|| "COM functions not found".to_owned())
        };
        type Proc = unsafe extern "system" fn() -> isize;
        // SAFETY: the exported functions have these documented signatures.
        let (init, uninitialize, create, free) = unsafe {
            (
                std::mem::transmute::<Proc, CoInitializeEx>(proc(b"CoInitializeEx\0")?),
                std::mem::transmute::<Proc, CoUninitialize>(proc(b"CoUninitialize\0")?),
                std::mem::transmute::<Proc, CoCreateInstance>(proc(b"CoCreateInstance\0")?),
                std::mem::transmute::<Proc, CoTaskMemFree>(proc(b"CoTaskMemFree\0")?),
            )
        };
        // SAFETY: plain COM initialization of this thread.
        let hr = unsafe { init(std::ptr::null(), COINIT_MULTITHREADED) };
        if hr < 0 {
            return Err(format!("COM initialization failed (0x{hr:08X})"));
        }
        // Uninitializes COM on every failure below.
        let fail = |message: String| {
            // SAFETY: balances the successful CoInitializeEx above.
            unsafe { uninitialize() };
            Err(message)
        };
        let mut factory = null_mut();
        // SAFETY: valid GUIDs and an out pointer.
        let hr = unsafe {
            create(
                &CLSID_SPELL_CHECKER_FACTORY,
                null_mut(),
                CLSCTX_INPROC_SERVER,
                &IID_ISPELL_CHECKER_FACTORY,
                &mut factory,
            )
        };
        if hr < 0 || factory.is_null() {
            return fail(format!("the spell checker is not available (0x{hr:08X})"));
        }
        let tag = wide(language);
        // SAFETY: `factory` is a live ISpellCheckerFactory.
        let vtbl = unsafe { &**(factory as *mut *const FactoryVtbl) };
        let mut supported = 0;
        // SAFETY: a NUL-terminated tag and an out pointer.
        let hr = unsafe { (vtbl.is_supported)(factory, tag.as_ptr(), &mut supported) };
        if hr < 0 || supported == 0 {
            // SAFETY: owned reference.
            unsafe { release(factory) };
            return fail(format!("{language} is not installed in Windows"));
        }
        let mut checker = null_mut();
        // SAFETY: as above.
        let hr = unsafe { (vtbl.create_spell_checker)(factory, tag.as_ptr(), &mut checker) };
        // SAFETY: owned reference, no longer needed.
        unsafe { release(factory) };
        if hr < 0 || checker.is_null() {
            return fail(format!("the {language} spell checker failed (0x{hr:08X})"));
        }
        Ok(WindowsSpeller {
            checker,
            free,
            uninitialize,
        })
    }

    fn vtbl(&self) -> &CheckerVtbl {
        // SAFETY: `checker` is a live ISpellChecker for the life of self.
        unsafe { &**(self.checker as *mut *const CheckerVtbl) }
    }

    /// `word` has no spelling error.
    pub fn check(&mut self, word: &str) -> Result<bool, String> {
        let text = wide(word);
        let mut errors = null_mut();
        // SAFETY: a live checker, NUL-terminated text, an out pointer.
        let hr = unsafe { (self.vtbl().check)(self.checker, text.as_ptr(), &mut errors) };
        if hr < 0 || errors.is_null() {
            return Err(format!("spell check failed (0x{hr:08X})"));
        }
        // SAFETY: `errors` is a live IEnumSpellingError.
        let vtbl = unsafe { &**(errors as *mut *const EnumErrorsVtbl) };
        let mut error = null_mut();
        // SAFETY: an out pointer; an error object is released at once.
        let hr = unsafe { (vtbl.next)(errors, &mut error) };
        unsafe {
            release(error);
            release(errors);
        }
        match hr {
            S_OK => Ok(false),
            S_FALSE => Ok(true),
            hr => Err(format!("spell check failed (0x{hr:08X})")),
        }
    }

    /// Up to `max` suggestions for `word`.
    pub fn suggest(&mut self, word: &str, max: usize) -> Result<Vec<String>, String> {
        let text = wide(word);
        let mut list = null_mut();
        // SAFETY: a live checker, NUL-terminated text, an out pointer.
        let hr = unsafe { (self.vtbl().suggest)(self.checker, text.as_ptr(), &mut list) };
        if hr < 0 || list.is_null() {
            return Err(format!("suggestions failed (0x{hr:08X})"));
        }
        // SAFETY: `list` is a live IEnumString.
        let vtbl = unsafe { &**(list as *mut *const EnumStringVtbl) };
        let mut out = Vec::new();
        while out.len() < max {
            let mut item: *mut u16 = null_mut();
            let mut fetched = 0;
            // SAFETY: one element into valid out pointers.
            let hr = unsafe { (vtbl.next)(list, 1, &mut item, &mut fetched) };
            if hr != S_OK || fetched == 0 || item.is_null() {
                break;
            }
            // SAFETY: a NUL-terminated string allocated with CoTaskMemAlloc.
            unsafe {
                let len = (0..).take_while(|&i| *item.add(i) != 0).count();
                out.push(String::from_utf16_lossy(std::slice::from_raw_parts(
                    item, len,
                )));
                (self.free)(item.cast());
            }
        }
        // SAFETY: owned reference.
        unsafe { release(list) };
        Ok(out)
    }
}

impl Drop for WindowsSpeller {
    fn drop(&mut self) {
        // SAFETY: the checker is owned; COM was initialized by `new` on this
        // thread (the spelling thread creates and drops it).
        unsafe {
            release(self.checker);
            (self.uninitialize)();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs `en-US` in Windows; skipped with a printed reason otherwise.
    #[test]
    fn windows_checks_and_suggests_en_us() {
        let mut speller = match WindowsSpeller::new("en-US") {
            Ok(s) => s,
            Err(e) => {
                eprintln!("skipped: {e}");
                return;
            }
        };
        assert!(speller.check("receive").unwrap());
        assert!(!speller.check("recieve").unwrap());
        let s = speller.suggest("recieve", 8).unwrap();
        assert!(s.iter().any(|w| w == "receive"), "{s:?}");
        assert!(s.len() <= 8);
        drop(speller);
        // A second checker after the first was released and COM uninitialized.
        assert!(WindowsSpeller::new("en-US").is_ok());
        assert!(WindowsSpeller::new("xx-XX").is_err());
    }
}
