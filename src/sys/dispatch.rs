//! The shared window procedure and its reentrancy bookkeeping.
//!
//! Every message a window receives is decoded and delivered to its handler,
//! reentrant or not: the handler is shared (`&self`), so a synchronous message
//! that arrives while the handler is already on the stack may run nested. The
//! bookkeeping here keeps the boxed handler alive until the outermost dispatch
//! for its window returns, so it cannot be freed while a caller still holds a
//! reference to it.

use core::cell::RefCell;
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, DefWindowProcW, GWLP_USERDATA, GetWindowLongPtrW, SetWindowLongPtrW,
    WM_NCCREATE, WM_NCDESTROY, WM_NOTIFY,
};

use crate::window::WindowHandler;

use super::{hwnd_from, message};

/// Reentrancy bookkeeping for one window: how many dispatches are on the stack,
/// and a boxed handler whose free had to be deferred because the window was
/// destroyed from inside one of them.
#[derive(Default)]
struct DispatchState {
    depth: usize,
    pending_free: Option<*mut Box<dyn WindowHandler>>,
}

thread_local! {
    /// Per-window dispatch depth, keyed by `HWND` as `isize`. A handler is
    /// shared (`&self`) so reentrant messages may run while an outer call is
    /// still on the stack; the boxed handler is therefore only freed once the
    /// outermost dispatch for its window has returned.
    static DISPATCH: RefCell<HashMap<isize, DispatchState>> = RefCell::new(HashMap::new());
}

/// Records the start of a dispatch for `hwnd`.
fn enter_dispatch(hwnd: HWND) {
    DISPATCH.with(|cell| {
        cell.borrow_mut().entry(hwnd.0 as isize).or_default().depth += 1;
    });
}

/// Records the end of a dispatch for `hwnd`. Once the outermost dispatch
/// returns, a handler whose `WM_NCDESTROY` was deferred is freed.
fn leave_dispatch(hwnd: HWND) {
    let pending = DISPATCH.with(|cell| {
        let mut map = cell.borrow_mut();
        let key = hwnd.0 as isize;
        let state = map.get_mut(&key)?;
        state.depth = state.depth.saturating_sub(1);
        if state.depth > 0 {
            return None;
        }
        let pending = state.pending_free.take();
        map.remove(&key);
        pending
    });
    if let Some(raw) = pending {
        // SAFETY: `raw` came from `Box::into_raw` in `window::create`; this was
        // the last dispatch for its window, so no reference to it remains.
        // Dropping it here, after `DISPATCH` is no longer borrowed, is what lets
        // a child control it owns destroy itself without re-entering the map.
        unsafe { drop(Box::from_raw(raw)) };
    }
}

/// Defers the free of `raw` to the outermost dispatch if one is in progress,
/// returning whether it was deferred (a window destroyed from inside its own
/// handler still holds a shared reference on the stack).
fn defer_free(hwnd: HWND, raw: *mut Box<dyn WindowHandler>) -> bool {
    DISPATCH.with(|cell| match cell.borrow_mut().get_mut(&(hwnd.0 as isize)) {
        Some(state) if state.depth > 0 => {
            state.pending_free = Some(raw);
            true
        }
        _ => false,
    })
}

/// The window procedure shared by every class registered by this crate.
///
/// # Safety
/// Called by Windows with a valid `hwnd` for a window created through
/// [`super::window::create`]; `msg`/`wparam`/`lparam` follow the documented
/// Win32 contract.
pub(crate) unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_NCCREATE {
        // SAFETY: for WM_NCCREATE, lparam is a CREATESTRUCTW* owned by the
        // system for the duration of the call; the handler pointer was boxed
        // by `window::create` and is reclaimed exactly once on WM_NCDESTROY.
        unsafe {
            let create = &*(lparam.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        }
    }

    // SAFETY: reads back the pointer stored above (null for foreign windows).
    let raw = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Box<dyn WindowHandler>;

    // Every message but `WM_NCDESTROY` goes to the handler, reentrant or not:
    // the handler is shared (`&self`), so a synchronous second message to the
    // same window may run while the first is still on the stack.
    let handled = if !raw.is_null() && msg != WM_NCDESTROY {
        enter_dispatch(hwnd);
        // SAFETY: `raw` was produced by `Box::into_raw` in `window::create`
        // and is freed only once the outermost dispatch for this window returns
        // (see `leave_dispatch`), so a shared reference stays valid across
        // reentrant messages.
        let handler: &dyn WindowHandler = unsafe { &**raw };
        // A panic unwinding across this `extern "system"` boundary is
        // undefined behaviour; isolate it instead of letting it propagate.
        let result = catch_unwind(AssertUnwindSafe(|| {
            deliver(hwnd, msg, wparam, lparam, handler)
        }))
        .unwrap_or(None);
        leave_dispatch(hwnd);
        result
    } else {
        None
    };

    let result = match handled {
        Some(value) => LRESULT(value),
        None => default_proc(hwnd, msg, wparam, lparam),
    };

    if msg == WM_NCDESTROY && !raw.is_null() {
        // SAFETY: the window is gone; clear the slot so no later lookup can
        // reach a handler that is about to be freed.
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        }
        // A handler that destroyed its own window is still on the stack with a
        // shared reference to the box, so hand the free to the outermost
        // dispatch; otherwise this is the only reference and it drops now.
        if !defer_free(hwnd, raw) {
            // SAFETY: no dispatch for this window is on the stack, so `raw` is
            // unreferenced and is reclaimed exactly once.
            unsafe { drop(Box::from_raw(raw)) };
        }
    }
    result
}

/// Default handling for a message the application did not claim.
fn default_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: `DefWindowProcW` is the documented default for any message a
    // window procedure does not handle.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn deliver(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    handler: &dyn WindowHandler,
) -> Option<isize> {
    // Registered controls get first refusal on their own notifications
    // (owner-data requests, custom draw, lazy expansion…).
    if msg == WM_NOTIFY
        && let Some((from, _id, code)) = message::notify_header(lparam)
        && let Some(result) =
            crate::controls::registry::dispatch(hwnd_from(from), code, wparam.0, lparam.0)
    {
        return Some(result);
    }

    let message = message::decode(hwnd, msg, wparam, lparam);
    let window = crate::window::Window::from_raw(hwnd_from(hwnd));
    handler.message(&window, message)
}
