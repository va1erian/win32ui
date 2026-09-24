//! Batched child-window moves, so a whole layout moves in one pass.
//!
//! `MoveWindow` sends `WM_WINDOWPOSCHANGED`/`WM_SIZE` (and repaints) per call,
//! which flickers when a resize repositions several controls. Wrapping the
//! moves in `BeginDeferWindowPos`/`EndDeferWindowPos` lets the window manager
//! apply them together. `SWP_NOCOPYBITS` stops the window manager from blitting
//! a child's old pixels into its new rectangle: a resized control (or a tab page
//! shown at its previous bounds) would otherwise keep stale content until it
//! was next interacted with.

use windows::Win32::UI::WindowsAndMessaging::{
    BeginDeferWindowPos, DeferWindowPos, EndDeferWindowPos, SWP_NOACTIVATE, SWP_NOCOPYBITS,
    SWP_NOZORDER,
};

use crate::geometry::Rect;
use crate::hwnd::Hwnd;

use super::raw_hwnd;

/// Moves every `(hwnd, bounds)` in `moves` in a single deferred batch.
///
/// Stale handles make the individual `DeferWindowPos` fail; the batch is ended
/// regardless so no window-manager state is leaked.
pub(crate) fn apply(moves: &[(Hwnd, Rect)]) {
    if moves.is_empty() {
        return;
    }

    // SAFETY: `BeginDeferWindowPos` only allocates an internal buffer sized for
    // the given number of subsequent `DeferWindowPos` calls.
    let Ok(mut batch) = (unsafe { BeginDeferWindowPos(moves.len() as i32) }) else {
        return;
    };

    for &(hwnd, bounds) in moves {
        // SAFETY: `batch` is the live handle returned by `BeginDeferWindowPos`
        // (or the previous `DeferWindowPos`); only integer geometry and documented
        // flags are passed, and the handle may be stale, which is a
        // reported failure rather than undefined behaviour.
        match unsafe {
            DeferWindowPos(
                batch,
                raw_hwnd(hwnd),
                None,
                bounds.left,
                bounds.top,
                bounds.width(),
                bounds.height(),
                SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOCOPYBITS,
            )
        } {
            Ok(next) => batch = next,
            Err(_) => {
                // SAFETY: `batch` is still the last successful handle; ending it
                // applies the moves deferred so far.
                unsafe {
                    let _ = EndDeferWindowPos(batch);
                }
                return;
            }
        }
    }

    // SAFETY: `batch` is the live handle from the last successful call.
    unsafe {
        let _ = EndDeferWindowPos(batch);
    }
}
