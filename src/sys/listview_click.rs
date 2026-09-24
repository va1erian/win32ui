//! The list view subclass that lets the widget layer consume a left click on
//! a single cell — a star toggle, say — before the control selects or
//! activates the row.
//!
//! Handling the click at `WM_LBUTTONDOWN` (rather than the later `NM_CLICK`
//! notification) is what keeps the selection and focus untouched: the control
//! never sees the message, so it never changes state.

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Shell::DefSubclassProc;
use windows::Win32::UI::WindowsAndMessaging::{WM_LBUTTONDBLCLK, WM_LBUTTONDOWN};

use crate::hwnd::Hwnd;

/// Handles a left-button press: receives the raw message and the client-space
/// point, and returns whether the click was consumed (the list must not select
/// or activate the row).
pub(crate) type ClickHandler = Box<dyn Fn(u32, i32, i32) -> bool>;

struct ClickRefdata {
    handler: ClickHandler,
}

/// Owns the click handler and the subclass that feeds it left-button presses.
pub(crate) struct ClickSubclass {
    view: Hwnd,
    raw: *mut ClickRefdata,
}

impl ClickSubclass {
    /// Subclasses `view` so `handler` may consume a left click. Returns `None`
    /// if subclassing fails.
    pub(crate) fn install(view: Hwnd, handler: ClickHandler) -> Option<ClickSubclass> {
        let raw = Box::into_raw(Box::new(ClickRefdata { handler }));
        if !super::window::set_subclass(view, Some(click_proc), CLICK_SUBCLASS_ID, raw as usize) {
            // SAFETY: install failed before the subclass could adopt it.
            unsafe { drop(Box::from_raw(raw)) };
            return None;
        }
        Some(ClickSubclass { view, raw })
    }
}

impl Drop for ClickSubclass {
    fn drop(&mut self) {
        super::window::remove_subclass(self.view, Some(click_proc), CLICK_SUBCLASS_ID);
        // SAFETY: installed in `install` and reclaimed exactly once.
        unsafe { drop(Box::from_raw(self.raw)) };
    }
}

const CLICK_SUBCLASS_ID: usize = 0x7763_6c6b; // "wclk"

/// The subclass procedure that offers left-button presses to the handler.
///
/// # Safety
/// Called by Windows for the list view subclass installed by
/// [`ClickSubclass::install`]; `refdata` is the `ClickRefdata` pointer.
unsafe extern "system" fn click_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    refdata: usize,
) -> LRESULT {
    if msg == WM_LBUTTONDOWN || msg == WM_LBUTTONDBLCLK {
        let x = (lparam.0 & 0xffff) as i16 as i32;
        let y = ((lparam.0 >> 16) & 0xffff) as i16 as i32;
        // SAFETY: `refdata` is the live `ClickRefdata` installed by `install`.
        // A panic unwinding across this `extern "system"` boundary is
        // undefined behaviour; isolate it instead.
        let consumed = unsafe {
            let data = &*(refdata as *const ClickRefdata);
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (data.handler)(msg, x, y)))
                .unwrap_or(false)
        };
        if consumed {
            // Swallow the press so the control neither moves the selection nor
            // starts an activation.
            return LRESULT(0);
        }
    }
    // SAFETY: forward to the subclass chain's original window procedure.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}
