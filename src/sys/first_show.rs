//! Showing a top-level window with its content already painted.
//!
//! A plain `ShowWindow` hands DWM a window whose children have not painted
//! yet, so the open animation plays over the class background (white or grey
//! controls) and the real content only appears once it ends. Instead the window
//! is cloaked (`DWMWA_CLOAK`: laid out and visible to the window manager, but
//! not composed), shown, painted synchronously down to every child, flushed to
//! DWM, and only then uncloaked, so the first composed frame is the finished
//! one. Uncloaking does not replay the open animation; the window appears
//! complete instead of zooming in over blank controls.

use core::ffi::c_void;

use windows::Win32::Graphics::Dwm::{DWMWA_CLOAK, DwmFlush, DwmSetWindowAttribute};
use windows::Win32::Graphics::Gdi::{
    RDW_ALLCHILDREN, RDW_ERASE, RDW_FRAME, RDW_INVALIDATE, RDW_UPDATENOW, RedrawWindow,
};
use windows::core::BOOL;

use crate::hwnd::Hwnd;

use super::raw_hwnd;

/// Cloaks or uncloaks `hwnd`; returns whether DWM accepted it.
fn set_cloak(hwnd: Hwnd, cloak: bool) -> bool {
    let value = BOOL::from(cloak);
    // SAFETY: `value` is a `BOOL` that outlives the call, and `DWMWA_CLOAK`
    // only reads `size_of::<BOOL>()` bytes from it.
    unsafe {
        DwmSetWindowAttribute(
            raw_hwnd(hwnd),
            DWMWA_CLOAK,
            &value as *const BOOL as *const c_void,
            size_of::<BOOL>() as u32,
        )
    }
    .is_ok()
}

/// Shows the top-level `hwnd` so that its first composed frame already holds
/// every child's content. Falls back to a plain show when DWM refuses to cloak.
pub(crate) fn show_painted(hwnd: Hwnd, show: impl FnOnce()) {
    let cloaked = set_cloak(hwnd, true);
    show();
    // SAFETY: `hwnd` is live; a null update rectangle means the whole window,
    // and only documented redraw flags are passed. The window is shown, so
    // `RDW_UPDATENOW` delivers `WM_PAINT` to it and every child before
    // returning.
    unsafe {
        let _ = RedrawWindow(
            Some(raw_hwnd(hwnd)),
            None,
            None,
            RDW_INVALIDATE | RDW_ERASE | RDW_FRAME | RDW_ALLCHILDREN | RDW_UPDATENOW,
        );
    }
    if cloaked {
        // SAFETY: `DwmFlush` takes no arguments; it waits for DWM to compose
        // the frame the paint above produced, and a failure only means there
        // is nothing to wait for.
        unsafe {
            let _ = DwmFlush();
        }
        set_cloak(hwnd, false);
    }
}
