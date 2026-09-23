//! The crate's raw-Win32 boundary.
//!
//! Every `unsafe` block in `win32ui` lives in this module tree. The functions
//! here are safe to call; they encode the pointer/lifetime contracts of the
//! Win32 APIs they wrap and document each block with a `// SAFETY:` comment.

pub(crate) mod capture;
pub(crate) mod clipboard;
pub(crate) mod control;
pub(crate) mod dispatch;
pub(crate) mod dpi;
pub(crate) mod gdi;
pub(crate) mod layout;
pub(crate) mod message;
pub(crate) mod theme;
pub(crate) mod window;
pub(crate) mod window_ext;
pub(crate) mod window_icon;
pub(crate) mod window_input;

pub(crate) use theme::{
    NativeControlKind, apply_native_theme, ctlcolor_brush, ctlcolor_msg_btn, ctlcolor_msg_dlg,
    ctlcolor_msg_edit, ctlcolor_msg_listbox, ctlcolor_msg_static, set_class_background,
    set_ctlcolor, set_titlebar_dark,
};

use core::ffi::c_void;

use windows::Win32::Foundation::HWND;

use crate::error::{Error, Win32Error};
use crate::hwnd::Hwnd;

/// Converts a safe handle into the raw `windows` newtype.
pub(crate) fn raw_hwnd(hwnd: Hwnd) -> HWND {
    HWND(hwnd.raw() as *mut c_void)
}

/// Converts a raw `windows` handle into the safe newtype.
pub(crate) fn hwnd_from(hwnd: HWND) -> Hwnd {
    Hwnd::from_raw(hwnd.0 as usize)
}

/// Converts a `windows` crate error into the crate-owned [`Win32Error`]. This
/// is the only conversion from a `windows` type, and it stays inside `sys`.
pub(crate) fn win32(error: windows::core::Error) -> Win32Error {
    Win32Error::new(error.code().0, error.message())
}

/// Converts a `windows` crate error into the crate-owned [`Error`], keeping
/// the `windows` type out of the public API.
pub(crate) fn win32_error(error: windows::core::Error) -> Error {
    Error::from(win32(error))
}
