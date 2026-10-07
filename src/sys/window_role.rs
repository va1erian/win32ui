//! A top-level window's shell role: tool window (no taskbar button, not in
//! Alt+Tab) and no-activate (clicking it never takes the focus).
//!
//! Both are extended styles, so they survive the fullscreen restyle, which
//! only rewrites `GWL_STYLE`. The shell reads `WS_EX_TOOLWINDOW` only when a
//! window is shown, so changing it on a visible window hides and re-shows it.

use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetWindowLongPtrW, SW_HIDE, SW_SHOWNA, SWP_FRAMECHANGED, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

use crate::hwnd::Hwnd;

use super::raw_hwnd;

/// The extended style bits of `hwnd`.
fn ex_style(hwnd: Hwnd) -> u32 {
    // SAFETY: `GWL_EXSTYLE` only reads the window's own extended style bits; a
    // stale handle reads 0.
    unsafe { GetWindowLongPtrW(raw_hwnd(hwnd), GWL_EXSTYLE) as u32 }
}

/// Whether `hwnd` has `WS_EX_NOACTIVATE`, so showing or repositioning it must
/// not activate it either.
pub(crate) fn is_no_activate(hwnd: Hwnd) -> bool {
    ex_style(hwnd) & WS_EX_NOACTIVATE.0 != 0
}

/// Whether `hwnd` is a tool window (`WS_EX_TOOLWINDOW`).
pub(crate) fn is_tool_window(hwnd: Hwnd) -> bool {
    ex_style(hwnd) & WS_EX_TOOLWINDOW.0 != 0
}

/// Sets or clears `WS_EX_NOACTIVATE` on `hwnd`.
pub(crate) fn set_no_activate(hwnd: Hwnd, on: bool) {
    let current = ex_style(hwnd);
    let next = if on {
        current | WS_EX_NOACTIVATE.0
    } else {
        current & !WS_EX_NOACTIVATE.0
    };
    write_ex_style(hwnd, current, next);
}

/// Makes `hwnd` a tool window (no taskbar button, not in Alt+Tab) or a normal
/// one. `WS_EX_APPWINDOW` is cleared either way: it would force the taskbar
/// button back, and this crate never sets it.
pub(crate) fn set_tool_window(hwnd: Hwnd, on: bool) {
    let current = ex_style(hwnd);
    let next = if on {
        (current | WS_EX_TOOLWINDOW.0) & !WS_EX_APPWINDOW.0
    } else {
        current & !WS_EX_TOOLWINDOW.0
    };
    if next == current {
        return;
    }
    let visible = super::window::is_visible(hwnd);
    if visible {
        // SAFETY: state flags only; a stale handle is a documented no-op.
        unsafe {
            let _ = ShowWindow(raw_hwnd(hwnd), SW_HIDE);
        }
    }
    write_ex_style(hwnd, current, next);
    if visible {
        // SAFETY: as above. `SW_SHOWNA` brings the taskbar entry in line
        // without activating the window.
        unsafe {
            let _ = ShowWindow(raw_hwnd(hwnd), SW_SHOWNA);
        }
    }
}

fn write_ex_style(hwnd: Hwnd, current: u32, next: u32) {
    if next == current {
        return;
    }
    // SAFETY: writes the window's own extended style; a stale handle is a
    // documented no-op. `SWP_FRAMECHANGED` makes the change take effect, the
    // other flags keep position, size, z-order and activation.
    unsafe {
        SetWindowLongPtrW(raw_hwnd(hwnd), GWL_EXSTYLE, next as isize);
        let _ = SetWindowPos(
            raw_hwnd(hwnd),
            None,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

#[cfg(test)]
mod tests {
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WINDOW_STYLE, WS_POPUP,
    };
    use windows::core::w;

    use super::*;

    /// Both roles follow their flag in both directions, and a tool window
    /// never keeps `WS_EX_APPWINDOW`.
    #[test]
    fn roles_can_be_set_and_cleared() {
        // SAFETY: a hidden, parentless `STATIC` window, destroyed below.
        let raw = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(WS_EX_APPWINDOW.0),
                w!("STATIC"),
                w!(""),
                WINDOW_STYLE(WS_POPUP.0),
                0,
                0,
                10,
                10,
                None,
                None,
                None,
                None,
            )
        }
        .expect("window");
        let hwnd = super::super::hwnd_from(raw);
        assert!(!is_tool_window(hwnd));
        assert!(!is_no_activate(hwnd));

        set_tool_window(hwnd, true);
        set_no_activate(hwnd, true);
        assert!(is_tool_window(hwnd), "not a tool window");
        assert!(is_no_activate(hwnd), "not no-activate");
        assert_eq!(
            ex_style(hwnd) & WS_EX_APPWINDOW.0,
            0,
            "kept WS_EX_APPWINDOW"
        );

        set_tool_window(hwnd, false);
        set_no_activate(hwnd, false);
        assert!(!is_tool_window(hwnd), "still a tool window");
        assert!(!is_no_activate(hwnd), "still no-activate");
        // SAFETY: `raw` was created above and is destroyed once.
        unsafe {
            let _ = DestroyWindow(raw);
        }
    }
}
