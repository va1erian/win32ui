//! `WM_TIMER` timers: plain (`SetTimer`) and coalescable
//! (`SetCoalescableTimer`). Both deliver `WM_TIMER` to the window with an id
//! drawn from the same per-thread sequence, so the two kinds never collide.

use core::cell::Cell;

use windows::Win32::UI::WindowsAndMessaging::{KillTimer, SetCoalescableTimer, SetTimer};

use crate::error::Result;
use crate::hwnd::Hwnd;

use super::{raw_hwnd, win32_error};

thread_local! {
    /// Source of unique, non-zero timer ids for this thread.
    static NEXT_TIMER_ID: Cell<usize> = const { Cell::new(0) };
}

fn next_id() -> usize {
    NEXT_TIMER_ID.with(|next| {
        let id = next.get().wrapping_add(1).max(1);
        next.set(id);
        id
    })
}

/// Maps a timer API's return value to the id it was started with.
///
/// With a non-null window handle, `SetTimer`/`SetCoalescableTimer` use
/// `nIDEvent` itself as the timer id; their return value is only documented as
/// nonzero on success, so it is not the id. The callers pass a fresh nonzero
/// id and return that id, which is what `WM_TIMER` reports back in `wparam`.
fn started(created: usize, id: usize) -> Result<usize> {
    if created == 0 {
        Err(win32_error(windows::core::Error::from_thread()))
    } else {
        Ok(id)
    }
}

/// Starts a repeating timer and returns the id it was given.
pub(crate) fn set_timer(hwnd: Hwnd, millis: u32) -> Result<usize> {
    let id = next_id();
    // SAFETY: `None` installs a WM_TIMER message rather than a callback.
    let created = unsafe { SetTimer(Some(raw_hwnd(hwnd)), id, millis, None) };
    started(created, id)
}

/// Starts a repeating timer that Windows may delay by up to `tolerance_ms` to
/// batch it with other timers, and returns the id it was given.
///
/// `tolerance_ms` is passed through as `uToleranceDelay`: `0`
/// (`TIMERV_DEFAULT_COALESCING`) uses the system default tolerance and
/// `u32::MAX` (`TIMERV_NO_COALESCING`) disables coalescing; any other value
/// above `0x7FFFFFF5` makes the call fail.
pub(crate) fn set_coalescable_timer(hwnd: Hwnd, millis: u32, tolerance_ms: u32) -> Result<usize> {
    let id = next_id();
    // SAFETY: `None` installs a WM_TIMER message rather than a callback.
    let created =
        unsafe { SetCoalescableTimer(Some(raw_hwnd(hwnd)), id, millis, None, tolerance_ms) };
    started(created, id)
}

/// Stops a timer started by [`set_timer`] or [`set_coalescable_timer`].
pub(crate) fn kill_timer(hwnd: Hwnd, id: usize) {
    // SAFETY: killing an unknown id is a documented no-op.
    unsafe {
        let _ = KillTimer(Some(raw_hwnd(hwnd)), id);
    }
}
