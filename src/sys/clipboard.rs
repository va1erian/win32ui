//! Unicode clipboard I/O over `OpenClipboard` / `EmptyClipboard` /
//! `SetClipboardData` with a moveable `GlobalAlloc` block.

use core::mem::size_of;
use std::thread::sleep;
use std::time::Duration;

use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};

use crate::error::Result;
use crate::hwnd::Hwnd;

use super::{raw_hwnd, win32_error};

/// `CF_UNICODETEXT` from `winuser.h` (`#define CF_UNICODETEXT 13`). The
/// `Win32_System_DataExchange` feature does not declare it — the `windows`
/// crate keeps clipboard formats under `Win32_System_Ole`, which this crate
/// deliberately does not enable — so the SDK value is mirrored here.
const CF_UNICODETEXT: u32 = 13;

/// How many times `OpenClipboard` is retried while another window holds it.
const OPEN_ATTEMPTS: u32 = 5;
/// Delay between `OpenClipboard` retries; the total wait is about 40 ms.
const RETRY_DELAY: Duration = Duration::from_millis(10);

/// An open clipboard. Dropping it calls `CloseClipboard`, so no early return
/// in the operations below can leave the clipboard open.
struct Open;

impl Drop for Open {
    fn drop(&mut self) {
        // SAFETY: the only way to obtain an `Open` is a successful
        // `OpenClipboard` on this thread, and `CloseClipboard` only touches the
        // thread's clipboard.
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

/// Opens the clipboard, retrying briefly while another window holds it.
fn open(owner: Hwnd) -> Result<Open> {
    let owner = raw_hwnd(owner);
    for attempt in 0..OPEN_ATTEMPTS {
        // SAFETY: `owner` is a window handle (possibly null) and the call only
        // opens the calling thread's clipboard.
        match unsafe { OpenClipboard(Some(owner)) } {
            Ok(()) => return Ok(Open),
            Err(error) => {
                if attempt + 1 == OPEN_ATTEMPTS {
                    return Err(win32_error(error));
                }
                sleep(RETRY_DELAY);
            }
        }
    }
    unreachable!("the final attempt returns")
}

/// Replaces the clipboard contents with `text` as `CF_UNICODETEXT`.
///
/// `text` is written verbatim; the safe layer has already normalised newlines.
/// `owner` must be a real window, because `SetClipboardData` fails when the
/// clipboard has no owner.
pub(crate) fn write_text(owner: Hwnd, text: &str) -> Result<()> {
    let _open = open(owner)?;

    // SAFETY: the clipboard is open on this thread; `EmptyClipboard` only
    // clears it and assigns ownership to `owner`.
    unsafe { EmptyClipboard() }.map_err(win32_error)?;

    let mut units: Vec<u16> = text.encode_utf16().collect();
    units.push(0); // CF_UNICODETEXT requires a nul-terminated string
    let bytes = units.len() * size_of::<u16>();

    // SAFETY: requests a moveable global block of `bytes`; it is unlocked and
    // either handed to the clipboard or freed before returning.
    let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes) }.map_err(win32_error)?;

    // SAFETY: `handle` is a live moveable block; the returned pointer stays
    // valid until the block is unlocked, which happens before it is published.
    let pointer = unsafe { GlobalLock(handle) };
    if pointer.is_null() {
        // SAFETY: the block is still owned by us and has not been published.
        unsafe {
            let _ = GlobalFree(Some(handle));
        }
        return Err(win32_error(windows::core::Error::from_thread()));
    }

    // SAFETY: `pointer` points at `bytes` writable bytes in the block, and
    // `units` is exactly `bytes` long; the two do not overlap.
    unsafe {
        std::ptr::copy_nonoverlapping(units.as_ptr(), pointer.cast::<u16>(), units.len());
    }
    // SAFETY: `pointer` came from locking `handle`, so this releases that lock
    // before the block is published. `GlobalUnlock`'s result is unreliable (it
    // reports "no lock count left" as an error), so it is ignored.
    unsafe {
        let _ = GlobalUnlock(handle);
    }

    // SAFETY: the clipboard is open on this thread. On success the system owns
    // the block and it must not be freed; on failure ownership stays with us
    // and it is freed below. `HANDLE` and `HGLOBAL` are both `*mut c_void`
    // wrappers, so the cast preserves the pointer.
    match unsafe { SetClipboardData(CF_UNICODETEXT, Some(HANDLE(handle.0))) } {
        Ok(_) => Ok(()),
        Err(error) => {
            // SAFETY: `SetClipboardData` failed, so the block was not handed
            // over and is still ours to free.
            unsafe {
                let _ = GlobalFree(Some(handle));
            }
            Err(win32_error(error))
        }
    }
}

/// Reads the clipboard's `CF_UNICODETEXT`, or `None` when it holds none.
pub(crate) fn read_text(owner: Hwnd) -> Result<Option<String>> {
    let _open = open(owner)?;

    // SAFETY: the clipboard is open on this thread; the call only queries it.
    if unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT) }.is_err() {
        return Ok(None);
    }

    // SAFETY: the clipboard is open and the format is present. The returned
    // handle is owned by the clipboard and must not be freed here; it stays
    // valid until `CloseClipboard` (on drop). A failure means the format was
    // taken away between the check and this call.
    let handle = match unsafe { GetClipboardData(CF_UNICODETEXT) } {
        Ok(handle) => handle,
        Err(_) => return Ok(None),
    };
    let block = HGLOBAL(handle.0);

    // SAFETY: `block` is a live moveable block owned by the clipboard.
    let pointer = unsafe { GlobalLock(block) };
    if pointer.is_null() {
        return Err(win32_error(windows::core::Error::from_thread()));
    }

    // SAFETY: a CF_UNICODETEXT block is a nul-terminated UTF-16 string; scan up
    // to (and excluding) the terminator and copy that many code units out
    // before the block is unlocked.
    let text = unsafe {
        let start = pointer.cast::<u16>();
        let mut length = 0usize;
        while *start.add(length) != 0 {
            length += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(start, length))
    };

    // SAFETY: `pointer` came from locking `block`; release it before the
    // clipboard is closed. The result is ignored (see `write_text`).
    unsafe {
        let _ = GlobalUnlock(block);
    }

    Ok(Some(text))
}
