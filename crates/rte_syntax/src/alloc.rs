//! Tree-sitter's allocator: mimalloc, about 6-16% faster parses than the
//! system heap.

#![allow(unsafe_code)]

use std::ffi::c_void;
use std::sync::Once;

use libmimalloc_sys::{mi_calloc, mi_free, mi_malloc, mi_realloc};

/// `p`, or a process abort when an allocation of `size > 0` bytes failed:
/// tree-sitter cannot handle null, and a panic must not unwind into C.
pub(crate) fn checked(p: *mut c_void, size: usize) -> *mut c_void {
    if p.is_null() && size != 0 {
        std::process::abort();
    }
    p
}

unsafe extern "C" fn malloc(size: usize) -> *mut c_void {
    // SAFETY: plain mimalloc call.
    checked(unsafe { mi_malloc(size) }, size)
}

unsafe extern "C" fn calloc(count: usize, size: usize) -> *mut c_void {
    // SAFETY: plain mimalloc call; an overflowing product fails as null.
    checked(
        unsafe { mi_calloc(count, size) },
        count.saturating_mul(size),
    )
}

unsafe extern "C" fn realloc(p: *mut c_void, size: usize) -> *mut c_void {
    // SAFETY: `p` comes from this allocator (tree-sitter's contract).
    checked(unsafe { mi_realloc(p, size) }, size)
}

unsafe extern "C" fn free(p: *mut c_void) {
    // SAFETY: `p` comes from this allocator (tree-sitter's contract).
    unsafe { mi_free(p) }
}

/// Routes tree-sitter's allocations to mimalloc. Later calls do nothing.
///
/// # Safety
///
/// Call before any other tree-sitter use in the process (no parser, tree,
/// query or language may exist yet) and not concurrently with such use.
pub unsafe fn install_allocator() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // SAFETY: the caller guarantees no tree-sitter object exists; the
        // four functions belong to one allocator and never return null for a
        // non-empty request.
        unsafe {
            tree_sitter::set_allocator(Some(tree_sitter::Allocator {
                malloc,
                calloc,
                realloc,
                free,
            }));
        }
    });
}
