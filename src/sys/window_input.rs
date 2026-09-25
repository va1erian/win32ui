//! Per-window input and activation: foreground, focus, enable state, mouse
//! capture and the cursor.

use windows::Win32::System::Threading::AttachThreadInput;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, IsWindowEnabled, ReleaseCapture, SetCapture, SetFocus,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GCLP_HCURSOR, GetForegroundWindow, GetWindowThreadProcessId, IDC_ARROW, IDC_HAND, IDC_IBEAM,
    IDC_SIZENS, IDC_SIZEWE, IDC_WAIT, IsIconic, LoadCursorW, SW_RESTORE, SetClassLongPtrW,
    SetCursor, SetForegroundWindow, ShowWindow, WM_GETDLGCODE, WM_PAINT,
};

use crate::hwnd::Hwnd;
use crate::window::CursorShape;

use super::raw_hwnd;

/// Brings a window to the foreground, restoring it first if minimized.
///
/// `SetForegroundWindow` is subject to the foreground lock: a process that did
/// not receive the last input event is refused, and Windows only flashes the
/// taskbar button. When the plain call is refused, the calling thread briefly
/// shares input state with the current foreground thread (the documented way
/// past the lock) and asks again.
pub(crate) fn set_foreground(hwnd: Hwnd) {
    let target = raw_hwnd(hwnd);
    // SAFETY: `IsIconic`/`ShowWindow` only read and write window state.
    if unsafe { IsIconic(target) }.as_bool() {
        unsafe {
            let _ = ShowWindow(target, SW_RESTORE);
        }
    }
    // SAFETY: each call below takes handles or thread ids and only changes
    // activation and input-queue state; a refusal is not an error.
    unsafe {
        if SetForegroundWindow(target).as_bool() {
            return;
        }
        let this_thread = GetWindowThreadProcessId(target, None);
        let foreground_thread = GetWindowThreadProcessId(GetForegroundWindow(), None);
        if foreground_thread == 0 || foreground_thread == this_thread {
            return;
        }
        if AttachThreadInput(this_thread, foreground_thread, true).as_bool() {
            let _ = SetForegroundWindow(target);
            let _ = AttachThreadInput(this_thread, foreground_thread, false);
        }
    }
}

/// The `DLGC_WANTARROWS` dialog code, from `winuser.h`: the window handles the
/// arrow keys itself.
pub(crate) const DLGC_WANTARROWS: isize = 1;

/// Whether `code` is the `WM_PAINT` message id. A `WM_PAINT` carrying a device
/// context in `wparam` (sent by the opaque top-bar subclass) decodes as
/// [`Message::Other`](crate::Message::Other) rather than `Message::Paint`.
pub(crate) fn is_paint(code: u32) -> bool {
    code == WM_PAINT
}

/// Whether `code` is the `WM_GETDLGCODE` message id.
pub(crate) fn is_get_dlg_code(code: u32) -> bool {
    code == WM_GETDLGCODE
}

/// Whether `hwnd` is the foreground window.
pub(crate) fn is_foreground(hwnd: Hwnd) -> bool {
    // SAFETY: `GetForegroundWindow` takes no arguments and only reads state.
    let foreground = unsafe { GetForegroundWindow() };
    foreground == raw_hwnd(hwnd)
}

/// Enables or disables a window.
pub(crate) fn set_enabled(hwnd: Hwnd, enabled: bool) {
    // SAFETY: `EnableWindow` only changes window state.
    unsafe {
        let _ = EnableWindow(raw_hwnd(hwnd), enabled);
    }
}

/// Whether a window is enabled.
pub(crate) fn is_enabled(hwnd: Hwnd) -> bool {
    // SAFETY: `IsWindowEnabled` only reads window state.
    unsafe { IsWindowEnabled(raw_hwnd(hwnd)).as_bool() }
}

/// Gives a window the keyboard focus.
pub(crate) fn focus(hwnd: Hwnd) {
    // SAFETY: `SetFocus` only changes focus; it fails for a window on another
    // thread's input queue, which is not an error.
    unsafe {
        let _ = SetFocus(Some(raw_hwnd(hwnd)));
    }
}

/// Captures the mouse for a window.
pub(crate) fn set_capture(hwnd: Hwnd) {
    // SAFETY: `SetCapture` only changes the capture window.
    unsafe {
        let _ = SetCapture(raw_hwnd(hwnd));
    }
}

/// Releases the mouse capture.
pub(crate) fn release_capture() {
    // SAFETY: `ReleaseCapture` takes no pointers.
    unsafe {
        let _ = ReleaseCapture();
    }
}

/// Sets the cursor shown over a window.
///
/// Replaces the window's class cursor (the fallback `WM_SETCURSOR` uses) and
/// applies it immediately.
pub(crate) fn set_cursor(hwnd: Hwnd, shape: CursorShape) {
    let name = match shape {
        CursorShape::Arrow => IDC_ARROW,
        CursorShape::Hand => IDC_HAND,
        CursorShape::IBeam => IDC_IBEAM,
        CursorShape::SizeHorizontal => IDC_SIZEWE,
        CursorShape::SizeVertical => IDC_SIZENS,
        CursorShape::Wait => IDC_WAIT,
    };
    // SAFETY: `name` is a shared system cursor id and a null module selects the
    // shared resource.
    let Ok(cursor) = (unsafe { LoadCursorW(None, name) }) else {
        return;
    };
    // SAFETY: `cursor` is a shared system cursor that stays valid; replacing the
    // class cursor and setting it for this thread only changes appearance.
    unsafe {
        let _ = SetClassLongPtrW(raw_hwnd(hwnd), GCLP_HCURSOR, cursor.0 as isize);
        let _ = SetCursor(Some(cursor));
    }
}
