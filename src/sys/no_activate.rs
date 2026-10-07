//! Showing a no-activate window (`WS_EX_NOACTIVATE`) maximized without
//! activating it.
//!
//! Win32 has no non-activating maximize: `ShowWindow(SW_SHOWMAXIMIZED)`,
//! `SetWindowPlacement` with that show command and `SC_MAXIMIZE` all activate
//! the window, `WS_EX_NOACTIVATE` or not, and disabling the window around the
//! call does not stop it. So the maximize is done by hand: the window gets
//! `WS_MAXIMIZE` (which `IsZoomed`, the caption buttons and a later restore
//! all read) and is moved with `SWP_NOACTIVATE` to the rectangle the system
//! gives a maximized window — the monitor's work area grown by the sizing
//! frame on every side. The normal (restored) placement is left untouched.

use core::mem::size_of;

use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
};
use windows::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_STYLE, GetWindowLongPtrW, SM_CXPADDEDBORDER, SM_CXSIZEFRAME, SM_CYSIZEFRAME,
    SW_SHOWMAXIMIZED, SW_SHOWMINIMIZED, SW_SHOWMINNOACTIVE, SW_SHOWNOACTIVATE, SW_SHOWNORMAL,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_NOZORDER, SWP_SHOWWINDOW,
    SetWindowLongPtrW, SetWindowPlacement, SetWindowPos, WINDOWPLACEMENT, WS_MAXIMIZE,
};

use crate::hwnd::Hwnd;

use super::raw_hwnd;

/// Maximizes and shows `hwnd` on its current monitor without activating it.
pub(crate) fn maximize(hwnd: Hwnd) {
    let raw = raw_hwnd(hwnd);
    // SAFETY: read-only queries on a live window and its monitor; `info` is
    // sized for `GetMonitorInfoW`. A stale handle makes them fail harmlessly.
    let (work, dpi) = unsafe {
        let monitor = MonitorFromWindow(raw, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return;
        }
        (info.rcWork, GetDpiForWindow(raw))
    };
    // SAFETY: plain metric queries.
    let (fx, fy) = unsafe {
        let padded = GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
        (
            GetSystemMetricsForDpi(SM_CXSIZEFRAME, dpi) + padded,
            GetSystemMetricsForDpi(SM_CYSIZEFRAME, dpi) + padded,
        )
    };
    // SAFETY: sets the window's own style bit, then moves it; neither call
    // activates it (`SWP_NOACTIVATE`), and a stale handle is a no-op.
    unsafe {
        let style = GetWindowLongPtrW(raw, GWL_STYLE);
        SetWindowLongPtrW(raw, GWL_STYLE, style | WS_MAXIMIZE.0 as isize);
        let _ = SetWindowPos(
            raw,
            None,
            work.left - fx,
            work.top - fy,
            (work.right - work.left) + 2 * fx,
            (work.bottom - work.top) + 2 * fy,
            SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_SHOWWINDOW,
        );
    }
}

/// Applies a saved `placement` to a no-activate window without activating
/// it: normal and minimized states map to their non-activating show commands,
/// and a maximized one is restored to its normal rectangle first and then
/// maximized with [`maximize`].
pub(crate) fn set_placement(hwnd: Hwnd, mut placement: WINDOWPLACEMENT) {
    let show = placement.showCmd;
    let maximized = show == SW_SHOWMAXIMIZED.0 as u32;
    placement.showCmd = if show == SW_SHOWMINIMIZED.0 as u32 {
        SW_SHOWMINNOACTIVE.0 as u32
    } else if maximized || show == SW_SHOWNORMAL.0 as u32 {
        SW_SHOWNOACTIVATE.0 as u32
    } else {
        show
    };
    // SAFETY: `placement` is a `WINDOWPLACEMENT` filled by `GetWindowPlacement`.
    unsafe {
        let _ = SetWindowPlacement(raw_hwnd(hwnd), &placement);
    }
    if maximized {
        maximize(hwnd);
    }
}

#[cfg(test)]
mod tests {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetActiveWindow, SetActiveWindow};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, GetWindowPlacement, IsZoomed, SW_SHOW, ShowWindow,
        WINDOW_EX_STYLE, WS_EX_NOACTIVATE, WS_OVERLAPPEDWINDOW,
    };
    use windows::core::w;

    use super::*;

    fn window(ex: WINDOW_EX_STYLE) -> windows::Win32::Foundation::HWND {
        // SAFETY: a parentless `STATIC` window, destroyed by the caller.
        unsafe {
            CreateWindowExW(
                ex,
                w!("STATIC"),
                w!(""),
                WS_OVERLAPPEDWINDOW,
                100,
                100,
                300,
                200,
                None,
                None,
                None,
                None,
            )
        }
        .expect("window")
    }

    fn placement(raw: windows::Win32::Foundation::HWND) -> WINDOWPLACEMENT {
        let mut p = WINDOWPLACEMENT {
            length: size_of::<WINDOWPLACEMENT>() as u32,
            ..Default::default()
        };
        // SAFETY: `p` is sized for the call.
        unsafe {
            let _ = GetWindowPlacement(raw, &mut p);
        }
        p
    }

    /// A no-activate window maximizes (and comes back from a saved maximized
    /// placement) without taking activation from the active window, and keeps
    /// its normal rectangle for a later restore.
    #[test]
    fn maximizes_without_activation() {
        let active = window(WINDOW_EX_STYLE(0));
        let panel = window(WS_EX_NOACTIVATE);
        let hwnd = super::super::hwnd_from(panel);
        // SAFETY: both windows are ours and live; destroyed below.
        unsafe {
            let _ = ShowWindow(active, SW_SHOW);
            let _ = SetActiveWindow(active);
        }
        let normal = placement(panel).rcNormalPosition;

        maximize(hwnd);
        // SAFETY: read-only queries on live windows.
        unsafe {
            assert!(IsZoomed(panel).as_bool(), "not maximized");
            assert_eq!(GetActiveWindow(), active, "maximize activated the window");
        }
        let saved = placement(panel);
        assert_eq!(saved.showCmd, SW_SHOWMAXIMIZED.0 as u32);
        assert_eq!(saved.rcNormalPosition, normal, "normal rect changed");

        // Restore through a normal placement, then re-apply the maximized one.
        let mut restored = saved;
        restored.showCmd = SW_SHOWNORMAL.0 as u32;
        set_placement(hwnd, restored);
        // SAFETY: as above.
        unsafe {
            assert!(!IsZoomed(panel).as_bool(), "still maximized");
            assert_eq!(GetActiveWindow(), active, "restore activated the window");
        }
        assert_eq!(
            placement(panel).rcNormalPosition,
            normal,
            "normal rect lost"
        );
        set_placement(hwnd, saved);
        // SAFETY: as above; each window is destroyed once.
        unsafe {
            assert!(IsZoomed(panel).as_bool(), "placement did not maximize");
            assert_eq!(GetActiveWindow(), active, "placement activated the window");
            let _ = DestroyWindow(panel);
            let _ = DestroyWindow(active);
        }
    }
}
