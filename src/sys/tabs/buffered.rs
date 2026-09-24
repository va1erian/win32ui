//! The tab control's double-buffered `WM_PAINT`.
//!
//! The native control paints its light frame and tabs straight to the screen,
//! and the dark chrome pass draws over them afterwards, so every repaint (a
//! hover, say) flashed white. Here the control paints into an off-screen buffer
//! with `WM_PRINTCLIENT`, the chrome pass draws over that, and only the finished
//! frame reaches the screen.

use std::cell::Cell;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{HDC, PAINTSTRUCT};
use windows::Win32::UI::Shell::DefSubclassProc;
use windows::Win32::UI::WindowsAndMessaging::{PRF_CLIENT, WM_ERASEBKGND, WM_PRINTCLIENT};

use crate::geometry::Rect;
use crate::sys::hwnd_from;

thread_local! {
    /// Whether the control is being asked to draw into the off-screen buffer.
    static PRINTING: Cell<bool> = const { Cell::new(false) };
}

/// Whether a `WM_ERASEBKGND` now belongs to the off-screen pass. The erase
/// `BeginPaint` sends for the screen must draw nothing, or the strip blanks
/// to the background before the finished frame lands.
pub(super) fn printing() -> bool {
    PRINTING.with(Cell::get)
}

/// Paints `hwnd` off-screen, calls `chrome` with the buffer's raw `HDC` for the
/// pass that overpaints the native frame, then flushes the dirty rectangle.
///
/// # Safety
/// Must be called from the tab control's subclass procedure while handling
/// `WM_PAINT`, with the control's client `bounds`.
pub(super) unsafe fn paint(hwnd: HWND, bounds: Rect, chrome: impl FnOnce(isize)) -> LRESULT {
    let window = hwnd_from(hwnd);
    let mut ps = PAINTSTRUCT::default();
    let screen = crate::sys::gdi::begin_paint(window, &mut ps);
    if screen.0.is_null() {
        crate::sys::gdi::end_paint(window, &ps);
        return LRESULT(0);
    }
    let dpi = crate::sys::dpi::window_dpi(window);
    let buffer =
        crate::sys::gdi::acquire_back_buffer(window, screen, bounds.width(), bounds.height(), dpi);
    // Without a buffer the control still paints, straight to the screen.
    let target: HDC = buffer.unwrap_or(screen);
    PRINTING.with(|flag| flag.set(true));
    // The control does not erase for `WM_PRINTCLIENT` itself, and the buffer
    // holds whatever it last drew, so erase it through the subclass first.
    crate::sys::control::send(window, WM_ERASEBKGND, target.0 as usize, 0);
    // SAFETY: `target` is a live DC for the duration of the call, and
    // `WM_PRINTCLIENT` with `PRF_CLIENT` asks the control to draw its client
    // into it.
    unsafe {
        DefSubclassProc(
            hwnd,
            WM_PRINTCLIENT,
            WPARAM(target.0 as usize),
            LPARAM(PRF_CLIENT as isize),
        );
    }
    PRINTING.with(|flag| flag.set(false));
    chrome(target.0 as isize);
    if buffer.is_some() {
        let dirty = Rect::new(
            ps.rcPaint.left,
            ps.rcPaint.top,
            ps.rcPaint.right,
            ps.rcPaint.bottom,
        );
        crate::sys::gdi::blit_rect(screen, target, dirty);
    }
    crate::sys::gdi::end_paint(window, &ps);
    LRESULT(0)
}
