//! The crate's raw-Win32 boundary.
//!
//! Every `unsafe` block in `win32ui` lives in this module tree. The functions
//! here are safe to call; they encode the pointer/lifetime contracts of the
//! Win32 APIs they wrap and document each block with a `// SAFETY:` comment.

pub(crate) mod control;
pub(crate) mod dpi;
pub(crate) mod gdi;
pub(crate) mod message;
pub(crate) mod window;

use core::ffi::c_void;

use windows::Win32::Foundation::HWND;

use crate::hwnd::Hwnd;

/// Converts a safe handle into the raw `windows` newtype.
pub(crate) fn raw_hwnd(hwnd: Hwnd) -> HWND {
    HWND(hwnd.raw() as *mut c_void)
}

/// Converts a raw `windows` handle into the safe newtype.
pub(crate) fn hwnd_from(hwnd: HWND) -> Hwnd {
    Hwnd::from_raw(hwnd.0 as usize)
}
