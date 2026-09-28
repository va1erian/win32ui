//! Moveable global memory blocks, the medium every format here travels in.

use windows::Win32::Foundation::{GlobalFree, HGLOBAL};
use windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};

/// Copies `bytes` into a new moveable block the caller owns (and must free or
/// hand to a `STGMEDIUM`).
pub(super) fn alloc(bytes: &[u8]) -> windows::core::Result<HGLOBAL> {
    // SAFETY: requests a moveable block; it is freed below on failure and
    // otherwise handed to the caller.
    let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)) }?;
    // SAFETY: `handle` is a live moveable block.
    let pointer = unsafe { GlobalLock(handle) };
    if pointer.is_null() {
        // SAFETY: the block was never published, so it is still ours.
        unsafe {
            let _ = GlobalFree(Some(handle));
        }
        return Err(windows::core::Error::from_thread());
    }
    // SAFETY: the block is at least `bytes.len()` bytes and locked at
    // `pointer`; the two ranges do not overlap.
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer.cast::<u8>(), bytes.len()) };
    // SAFETY: releases the lock taken above; the result is unreliable (it
    // reports "no lock left" as an error) and is ignored.
    unsafe {
        let _ = GlobalUnlock(handle);
    }
    Ok(handle)
}

/// Copies the whole block out, or `None` when it cannot be locked.
///
/// `handle` must be a live `HGLOBAL` (as held by a `TYMED_HGLOBAL` medium).
pub(super) fn read(handle: HGLOBAL) -> Option<Vec<u8>> {
    if handle.0.is_null() {
        return None;
    }
    // SAFETY: the caller passes a live block; the size is read while it is
    // still valid.
    let size = unsafe { GlobalSize(handle) };
    // SAFETY: as above; locking pins the block while it is copied.
    let pointer = unsafe { GlobalLock(handle) };
    if pointer.is_null() {
        return None;
    }
    // SAFETY: the locked block is `size` readable bytes long, and the copy
    // ends before the unlock.
    let bytes = unsafe { std::slice::from_raw_parts(pointer.cast::<u8>(), size) }.to_vec();
    // SAFETY: releases the lock taken above (result ignored, see `alloc`).
    unsafe {
        let _ = GlobalUnlock(handle);
    }
    Some(bytes)
}
