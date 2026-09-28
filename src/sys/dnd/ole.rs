//! Lazy, per-thread `OleInitialize`.

use std::cell::RefCell;

use windows::Win32::System::Ole::{OleInitialize, OleUninitialize};

use crate::error::Result;

use super::super::win32_error;

/// A successful `OleInitialize` on this thread, balanced on thread exit.
struct OleGuard;

impl Drop for OleGuard {
    fn drop(&mut self) {
        // SAFETY: balances the successful `OleInitialize` that created this
        // guard, on the same thread (the guard lives in a `thread_local`).
        unsafe { OleUninitialize() };
    }
}

thread_local! {
    static OLE: RefCell<Option<OleGuard>> = const { RefCell::new(None) };
}

/// Makes the calling thread an OLE apartment, once. Returns the OS error when
/// the thread is in an incompatible (multithreaded) apartment.
pub(super) fn ensure() -> Result<()> {
    OLE.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_some() {
            return Ok(());
        }
        // SAFETY: `OleInitialize` takes a reserved null pointer and only
        // affects the calling thread. `S_FALSE` (already initialised) also
        // counts as success and must be balanced, which the guard does.
        unsafe { OleInitialize(None) }.map_err(win32_error)?;
        *slot = Some(OleGuard);
        Ok(())
    })
}
