//! Auto-hiding the cursor inside a window after a period without movement.
//!
//! The window's messages are intercepted in [`super::dispatch::window_proc`]:
//! a mouse move resets the timer (and shows the cursor again if it was hidden),
//! and the timer hides it. `WM_SETCURSOR` is suppressed while hidden so the
//! class cursor does not immediately bring it back.

use core::cell::RefCell;
use std::collections::HashMap;

use windows::Win32::UI::WindowsAndMessaging::{
    IDC_ARROW, LoadCursorW, SetCursor, WM_MOUSEMOVE, WM_SETCURSOR, WM_TIMER,
};

use crate::error::Result;
use crate::hwnd::Hwnd;

/// The idle state of one window.
struct Idle {
    /// The repeating `WM_TIMER` id that fires the hide.
    timer: usize,
    /// The period, so a mouse move can restart the timer.
    millis: u32,
    /// Whether the cursor is currently hidden.
    hidden: bool,
}

thread_local! {
    /// Windows with cursor idle-hiding armed, keyed by `HWND`. Windows are
    /// thread-affine, so the owning thread's map serves their messages.
    static IDLE: RefCell<HashMap<isize, Idle>> = RefCell::new(HashMap::new());
}

/// Arms idle hiding for `hwnd`: the cursor is hidden after `millis` without a
/// mouse move, and shown again on the next move. `0` disables it and restores
/// the cursor.
pub(crate) fn arm(hwnd: Hwnd, millis: u32) -> Result<()> {
    disarm(hwnd);
    if millis == 0 {
        return Ok(());
    }
    let timer = super::window::set_timer(hwnd, millis)?;
    IDLE.with(|map| {
        map.borrow_mut().insert(
            hwnd.raw() as isize,
            Idle {
                timer,
                millis,
                hidden: false,
            },
        );
    });
    Ok(())
}

/// Disarms idle hiding for `hwnd` and shows the cursor again.
pub(crate) fn disarm(hwnd: Hwnd) {
    let old = IDLE.with(|map| map.borrow_mut().remove(&(hwnd.raw() as isize)));
    if let Some(old) = old {
        super::window::kill_timer(hwnd, old.timer);
        show_cursor();
    }
}

/// Drops a destroyed window's idle state.
pub(crate) fn forget(hwnd: Hwnd) {
    IDLE.with(|map| {
        map.borrow_mut().remove(&(hwnd.raw() as isize));
    });
}

/// Handles the messages idle-hiding owns, returning `Some(result)` when the
/// window procedure must stop (`WM_SETCURSOR` suppressed, or the idle timer
/// consumed). A mouse move returns `None` so the app still sees it.
pub(crate) fn handled(hwnd: Hwnd, msg: u32, wparam: usize) -> Option<isize> {
    let key = hwnd.raw() as isize;
    match msg {
        WM_MOUSEMOVE => {
            let state = IDLE.with(|map| {
                map.borrow()
                    .get(&key)
                    .map(|state| (state.hidden, state.timer, state.millis))
            });
            let (hidden, timer, millis) = state?;
            if hidden {
                IDLE.with(|map| {
                    if let Some(state) = map.borrow_mut().get_mut(&key) {
                        state.hidden = false;
                    }
                });
                show_cursor();
            }
            super::window::kill_timer(hwnd, timer);
            if let Ok(next) = super::window::set_timer(hwnd, millis) {
                IDLE.with(|map| {
                    if let Some(state) = map.borrow_mut().get_mut(&key) {
                        state.timer = next;
                    }
                });
            }
            None
        }
        WM_SETCURSOR => IDLE
            .with(|map| map.borrow().get(&key).is_some_and(|state| state.hidden))
            .then_some(1),
        WM_TIMER => {
            let is_idle = IDLE.with(|map| {
                map.borrow()
                    .get(&key)
                    .is_some_and(|state| state.timer == wparam)
            });
            if !is_idle {
                return None;
            }
            IDLE.with(|map| {
                if let Some(state) = map.borrow_mut().get_mut(&key) {
                    state.hidden = true;
                }
            });
            // SAFETY: a null cursor hides the pointer; it is restored on the
            // next mouse move.
            unsafe {
                let _ = SetCursor(None);
            }
            Some(0)
        }
        _ => None,
    }
}

/// Restores the window class's arrow cursor.
fn show_cursor() {
    // SAFETY: `IDC_ARROW` is a shared system cursor and a null module selects
    // the shared resource.
    if let Ok(cursor) = unsafe { LoadCursorW(None, IDC_ARROW) } {
        // SAFETY: `cursor` is a shared system cursor that stays valid.
        unsafe {
            let _ = SetCursor(Some(cursor));
        }
    }
}
