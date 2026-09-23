//! A subclass hook that ferries worker-thread messages into the widget layer.
//!
//! [`install`] subclasses a top-level window so its private drain message runs
//! `on_drain` before reaching the window's own procedure. The widget layer's
//! [`Proxy`](crate::app::proxy::Proxy) uses this to move its thread-safe inbox
//! into the window's message queue ahead of the drain, so the queued messages
//! are delivered by the same drain without ever re-entering `update`.

use std::sync::atomic::{AtomicUsize, Ordering};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Shell::DefSubclassProc;
use windows::Win32::UI::WindowsAndMessaging::WM_NCDESTROY;

use crate::hwnd::Hwnd;

use super::window::{remove_subclass, set_subclass};

/// State for one installed hook: the drain message to intercept and the
/// callback that moves the worker inbox into the widget-layer queue.
///
/// The callback runs on the UI thread — the drain message is only ever
/// posted, never sent — so it may touch the window's non-thread-safe queue.
struct HookData {
    drain: u32,
    on_drain: Box<dyn Fn()>,
}

/// Subclasses `hwnd` so `drain` first runs `on_drain`, then reaches the
/// window's own procedure (which drains the widget-layer queue).
///
/// Returns whether the hook was installed. The hook removes itself and frees
/// its state when the window is destroyed, so there is nothing to drop.
pub(crate) fn install(hwnd: Hwnd, drain: u32, on_drain: Box<dyn Fn()>) -> bool {
    if hwnd.is_null() || drain == 0 {
        return false;
    }
    static NEXT_ID: AtomicUsize = AtomicUsize::new(1);
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let raw = Box::into_raw(Box::new(HookData { drain, on_drain }));
    if !set_subclass(hwnd, Some(proxy_proc), id, raw as usize) {
        // SAFETY: install failed before the subclass could adopt the state, so
        // this is the only reference to it.
        unsafe { drop(Box::from_raw(raw)) };
        return false;
    }
    true
}

/// The subclass procedure installed by [`install`].
///
/// # Safety
/// Called by Windows for the subclass installed by [`install`]; `refdata` is
/// the `HookData` pointer it installed.
unsafe extern "system" fn proxy_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    id: usize,
    refdata: usize,
) -> LRESULT {
    // SAFETY: `refdata` is the live `HookData` installed by `install`. It is
    // freed only below on `WM_NCDESTROY`, the window's last message, after
    // which no further callback can touch it.
    let data = unsafe { &*(refdata as *const HookData) };
    if msg == data.drain {
        // A panic unwinding across this `extern "system"` boundary is
        // undefined behaviour; isolate it instead of letting it propagate.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (data.on_drain)()));
    } else if msg == WM_NCDESTROY {
        remove_subclass(super::hwnd_from(hwnd), Some(proxy_proc), id);
        // SAFETY: as above; `WM_NCDESTROY` is delivered once, so this reclaims
        // the allocation exactly once.
        unsafe { drop(Box::from_raw(refdata as *mut HookData)) };
    }
    // SAFETY: forward to the subclass chain's original window procedure.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}
