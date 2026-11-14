//! The WebAssembly face of `kraken-decode`: a byte allocator and one decode
//! call, exported with the C ABI so a few lines of JavaScript can drive it
//! (`js/src/loader.mjs`). No bindings generator is involved.
//!
//! This is the only place in the repository with `unsafe`: the host hands
//! buffers over as pointer and length, and turning those back into slices
//! cannot be checked by the compiler. Each block says what it relies on.

#![deny(unsafe_op_in_unsafe_fn)]
#![warn(missing_docs)]

use std::cell::Cell;

use kraken_decode::Error;

thread_local! {
    /// The part of the stream the last failed call named, for `kd_detail_*`.
    static DETAIL: Cell<&'static str> = const { Cell::new("") };
}

/// Allocates `len` zeroed bytes and returns their address. Release them with
/// [`kd_free`] and the same `len`.
#[no_mangle]
pub extern "C" fn kd_alloc(len: usize) -> *mut u8 {
    Box::into_raw(vec![0u8; len].into_boxed_slice()).cast()
}

/// Releases a buffer from [`kd_alloc`].
///
/// # Safety
///
/// `ptr` and `len` must be exactly what one earlier `kd_alloc(len)` call
/// returned and was given, and the buffer must not have been freed already.
#[no_mangle]
pub unsafe extern "C" fn kd_free(ptr: *mut u8, len: usize) {
    // SAFETY: by the contract above this is the boxed slice `kd_alloc` leaked,
    // with its original length, and nothing else owns it.
    drop(unsafe { Box::from_raw(core::ptr::slice_from_raw_parts_mut(ptr, len)) });
}

/// Decodes `src_len` bytes at `src` into the `dst_len` bytes at `dst`.
///
/// Returns 0 on success, or 1 (truncated), 2 (corrupt), 3 (unsupported) or
/// 4 (size mismatch). After 2 or 3, [`kd_detail_ptr`] and [`kd_detail_len`]
/// give the name of the part of the stream at fault.
///
/// # Safety
///
/// `src` and `dst` must each be a live buffer from [`kd_alloc`] of at least
/// the given length, and they must not overlap.
#[no_mangle]
pub unsafe extern "C" fn kd_decode(src: *const u8, src_len: usize, dst: *mut u8, dst_len: usize) -> u32 {
    // SAFETY: the caller promises `src` is a live `kd_alloc` buffer of at
    // least `src_len` bytes: non-null, aligned (u8), initialised (`kd_alloc`
    // zeroes), and not written through `dst`, which does not overlap it.
    let src = unsafe { core::slice::from_raw_parts(src, src_len) };
    // SAFETY: likewise for `dst`, which nothing else reads or writes while
    // this call runs (a WebAssembly instance runs one call at a time).
    let dst = unsafe { core::slice::from_raw_parts_mut(dst, dst_len) };
    let (code, detail) = match kraken_decode::decode(src, dst) {
        Ok(()) => (0, ""),
        Err(Error::Truncated) => (1, ""),
        Err(Error::Corrupt(what)) => (2, what),
        Err(Error::Unsupported(what)) => (3, what),
        Err(Error::SizeMismatch) => (4, ""),
        // `Error` is non-exhaustive; a variant added later lands here until this shim names it.
        Err(_) => (5, ""),
    };
    DETAIL.set(detail);
    code
}

/// Address of the detail text for the last failed [`kd_decode`] (UTF-8, not
/// NUL-terminated; valid until the instance is dropped).
#[no_mangle]
pub extern "C" fn kd_detail_ptr() -> *const u8 {
    DETAIL.get().as_ptr()
}

/// Length in bytes of the text at [`kd_detail_ptr`].
#[no_mangle]
pub extern "C" fn kd_detail_len() -> usize {
    DETAIL.get().len()
}
