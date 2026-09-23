//! Test-only raw helpers: force a tooltip on screen and read back its
//! geometry, so the widget-layer test can prove comctl32 sized the shown window
//! for the font and text the owner-draw paints with.

use core::ffi::c_void;

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::HFONT;
use windows::Win32::UI::Controls::{
    TTF_ABSOLUTE, TTF_TRACK, TTM_ADDTOOLW, TTM_DELTOOLW, TTM_TRACKACTIVATE, TTM_TRACKPOSITION,
    TTTOOLINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, WM_GETFONT};

use super::tool_info;
use crate::geometry::Rect;
use crate::hwnd::Hwnd;

/// The `uId` of the temporary tracking tool.
const TRACK_ID: usize = usize::MAX - 1;

thread_local! {
    /// Keeps each tracking tool's text buffer alive: the tooltip stores the
    /// pointer until the tool is removed, so the box must not move or drop.
    static TRACK_TEXT: std::cell::RefCell<Vec<Box<[u16]>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Shows `tooltip` for a tracking tool carrying `text` — the documented way to
/// display one without pointer input (`TTM_TRACKACTIVATE`). Returns whether the
/// tooltip accepted the tool.
pub(crate) fn show_tracking(tooltip: Hwnd, owner: Hwnd, text: &str) -> bool {
    let wide: Box<[u16]> = text.encode_utf16().chain(core::iter::once(0)).collect();
    let mut info = tool_info(
        owner,
        TRACK_ID,
        TTF_TRACK.0 | TTF_ABSOLUTE.0,
        None,
        wide.as_ptr() as *mut u16,
    );
    TRACK_TEXT.with(|cell| cell.borrow_mut().push(wide));
    let added = super::super::window::send_message(
        tooltip,
        TTM_ADDTOOLW,
        0,
        &mut info as *mut TTTOOLINFOW as isize,
    );
    if added == 0 {
        return false;
    }
    let at: isize = (((300i32) << 16) | 300) as isize;
    super::super::window::send_message(tooltip, TTM_TRACKPOSITION, 0, at);
    super::super::window::send_message(
        tooltip,
        TTM_TRACKACTIVATE,
        1,
        &mut info as *mut TTTOOLINFOW as isize,
    );
    true
}

/// Hides and removes the tracking tool [`show_tracking`] added.
pub(crate) fn hide_tracking(tooltip: Hwnd, owner: Hwnd) {
    let mut info = tool_info(
        owner,
        TRACK_ID,
        TTF_TRACK.0 | TTF_ABSOLUTE.0,
        None,
        core::ptr::null_mut(),
    );
    super::super::window::send_message(
        tooltip,
        TTM_TRACKACTIVATE,
        0,
        &mut info as *mut TTTOOLINFOW as isize,
    );
    super::super::window::send_message(
        tooltip,
        TTM_DELTOOLW,
        0,
        &mut info as *mut TTTOOLINFOW as isize,
    );
}

/// The screen rectangle of `hwnd`.
pub(crate) fn window_rect(hwnd: Hwnd) -> Rect {
    let mut rect = RECT::default();
    // SAFETY: `hwnd` is live and `rect` is a valid out-pointer.
    unsafe {
        let _ = GetWindowRect(HWND(hwnd.raw() as *mut c_void), &mut rect);
    }
    Rect::new(rect.left, rect.top, rect.right, rect.bottom)
}

/// The font `WM_GETFONT` reports for `tooltip`: the one comctl32 sized it with
/// and the owner-draw paints with.
pub(crate) fn paint_font(tooltip: Hwnd) -> HFONT {
    let raw = super::super::window::send_message(tooltip, WM_GETFONT, 0, 0);
    HFONT(raw as *mut c_void)
}
