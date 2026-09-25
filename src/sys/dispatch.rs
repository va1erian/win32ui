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
    CREATESTRUCTW, DefWindowProcW, GWLP_USERDATA, GetWindowLongPtrW, MSG, SetWindowLongPtrW,
    WM_ERASEBKGND, WM_GETMINMAXINFO, WM_GETOBJECT, WM_NCCALCSIZE, WM_NCCREATE, WM_NCDESTROY,
    WM_NCHITTEST, WM_NOTIFY,
};

use crate::message::{Command, Message};
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

    // A window that describes itself to assistive technology answers the UI
    // Automation root request; everything else falls through unchanged.
    if msg == WM_GETOBJECT
        && let Some(result) = super::uia::get_object(hwnd_from(hwnd), wparam.0, lparam.0)
    {
        return LRESULT(result);
    }

    if msg == WM_GETMINMAXINFO {
        // Apply the window's configured tracking limits before the handler, so
        // it can read them with `Window::min_max_info`.
        super::window_ext::apply_track_limits(hwnd_from(hwnd), lparam.0);
    }

    // Auto-hiding the cursor owns `WM_MOUSEMOVE`/`WM_SETCURSOR`/one `WM_TIMER`;
    // it returns `None` for everything else (and for a move the app still sees).
    if let Some(result) = super::cursor_idle::handled(hwnd_from(hwnd), msg, wparam.0) {
        return LRESULT(result);
    }

    // The extended title bar owns these two: it removes the standard caption
    // and routes the strip's hit-testing through DWM. Both return `None` for a
    // standard window, so the default path below is unchanged.
    if msg == WM_NCCALCSIZE
        && let Some(result) = super::nc::calc_size(hwnd, wparam, lparam)
    {
        return result;
    }
    if msg == WM_NCHITTEST
        && let Some(result) = super::nc::hit_test(hwnd, wparam, lparam)
    {
        return result;
    }
    // An extended-frame window erases to the theme background, then clears its
    // caption strip to black so DWM's backdrop shows through it. Both are
    // internal to the extended title bar; a standard window falls through.
    if msg == WM_ERASEBKGND
        && let Some(result) = super::nc::erase_background(hwnd, wparam)
    {
        return result;
    }

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
        // An app that claims a `WM_CTLCOLOR*` overrides the theme; otherwise
        // the window's theme answers it so native edits, statics and buttons
        // are correct without the app doing anything.
        None if crate::theme::is_ctlcolor(msg) => {
            match crate::theme::ctlcolor_answer(hwnd_from(hwnd), msg, wparam.0 as isize) {
                Some(brush) => LRESULT(brush),
                None => default_proc(hwnd, msg, wparam, lparam),
            }
        }
        None => default_proc(hwnd, msg, wparam, lparam),
    };

    // The system has just drawn the non-client area; paint over the light seam
    // it leaves under a dark menu bar.
    super::menu_seam::after_message(hwnd, msg);

    // `DefWindowProcW` fills in default limits, so re-apply the configured ones
    // when the handler did not claim the message.
    if msg == WM_GETMINMAXINFO && handled.is_none() {
        super::window_ext::apply_track_limits(hwnd_from(hwnd), lparam.0);
    }

    if msg == WM_NCDESTROY {
        // The window is gone: drop its cached off-screen buffer so a dead
        // `HWND` never keeps a thread-local bitmap alive.
        super::gdi::release_back_buffer(hwnd_from(hwnd));
        super::window_ext::forget_track_limits(hwnd_from(hwnd));
        super::cursor_idle::forget(hwnd_from(hwnd));
        super::fullscreen::forget(hwnd_from(hwnd));
        super::menu_seam::forget(hwnd_from(hwnd));
        super::looper::forget_keyboard(hwnd_from(hwnd));
        crate::theme::forget_window_theme(hwnd_from(hwnd));
        crate::controls::tooltip::forget_window(hwnd_from(hwnd));
        crate::window::nc::forget_window(hwnd_from(hwnd));
        crate::accessibility::registry::forget(hwnd_from(hwnd));
    }

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
    // A raw-message hook (shell integrations) sees every message before it is
    // decoded, mirroring winit's `with_msg_hook`; a hook that claims it stops
    // further handling. The `MSG` is a stack local valid for the call, which is
    // the same pointer contract those hooks expect.
    let raw = MSG {
        hwnd,
        message: msg,
        wParam: wparam,
        lParam: lparam,
        ..MSG::default()
    };
    if let Some(result) = handler.raw_message((&raw as *const MSG).cast()) {
        return Some(result);
    }

    // `WM_NOTIFY` is special: it may be consumed by a registered control
    // (self-contained owner-data/custom-draw plumbing), mapped to the app's
    // `Msg` by a widget-layer event mapper, or left for the window handler.
    if msg == WM_NOTIFY
        && let Some((from, _id, code)) = message::notify_header(lparam)
    {
        if let Some(result) =
            crate::controls::registry::dispatch(hwnd_from(from), code, wparam.0, lparam.0)
        {
            return Some(result);
        }
        let decoded = message::decode(hwnd, msg, wparam, lparam)?;
        if crate::controls::registry::dispatch_app_event(hwnd_from(from), &decoded) {
            return Some(0);
        }
        let window = crate::window::Window::from_raw(hwnd_from(hwnd));
        return handler.message(&window, decoded);
    }

    // `WM_DRAWITEM` is special like `WM_NOTIFY`: an owner-drawn control
    // (`BS_OWNERDRAW`) asks its parent to paint, so the request is offered to
    // the widget-layer mapper registered for that control first. A mapper
    // that paints returns `Some(1)` (TRUE).
    if msg == message::draw_message_id()
        && let Some(drawn) = message::decode_draw(wparam, lparam)
    {
        if let Message::DrawItem { control, .. } = &drawn
            && crate::controls::registry::dispatch_app_event(*control, &drawn)
        {
            return Some(1);
        }
        let window = crate::window::Window::from_raw(hwnd_from(hwnd));
        return handler.message(&window, drawn);
    }

    // A message may be suppressed (the first half of a `WM_CHAR` surrogate
    // pair), in which case the handler is not called.
    let message = message::decode(hwnd, msg, wparam, lparam)?;
    // A window that opted into `follow_system_theme` re-reads `Theme::system`
    // and applies it here, then still delivers the message below so the app
    // can react too.
    if crate::theme::is_theme_change(&message) {
        crate::theme::notify_theme_change(hwnd_from(hwnd));
    }
    // A `WM_COMMAND` from a child control is first offered to that control's
    // widget-layer mapper (as `WM_NOTIFY` is above), so a combo's
    // `CBN_SELCHANGE` reaches the app as a typed message without the app
    // knowing the control's numeric id.
    if let Message::Command(Command {
        control: Some(control),
        ..
    }) = &message
        && crate::controls::registry::dispatch_app_event(*control, &message)
    {
        return Some(0);
    }
    let window = crate::window::Window::from_raw(hwnd_from(hwnd));
    handler.message(&window, message)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    /// Claims every raw message so `deliver` must return before decoding it.
    struct RawProbe {
        seen: Cell<bool>,
    }

    impl WindowHandler for RawProbe {
        fn raw_message(&self, _msg: *const std::ffi::c_void) -> Option<isize> {
            self.seen.set(true);
            Some(42)
        }

        fn message(&self, _window: &crate::window::Window, _message: Message) -> Option<isize> {
            panic!("a claimed raw message must not be decoded");
        }
    }

    #[test]
    fn deliver_offers_the_raw_message_before_decoding() {
        let probe = RawProbe {
            seen: Cell::new(false),
        };
        // A null HWND is fine: a claiming hook returns before the message is
        // decoded or the window is touched.
        let result = deliver(
            HWND::default(),
            0x1234,
            WPARAM::default(),
            LPARAM::default(),
            &probe,
        );
        assert_eq!(result, Some(42));
        assert!(probe.seen.get(), "the raw hook ran first");
    }
}
