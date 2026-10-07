//! Showing a no-activate window (`WS_EX_NOACTIVATE`) maximized without
//! activating it.
//!
//! Win32 has no non-activating maximize: `ShowWindow(SW_SHOWMAXIMIZED)`,
//! `SetWindowPlacement` with that show command and `SC_MAXIMIZE` all activate
//! the window, `WS_EX_NOACTIVATE` or not, and disabling the window around the
//! call does not stop it. So the maximize is done by hand: the window gets
//! `WS_MAXIMIZE` (which `IsZoomed`, the caption buttons and a later restore
//! all read) and is moved with `SWP_NOACTIVATE` to the rectangle the system
//! gives a maximized window of that style: the monitor's work area for a
//! captioned, sizable window and the full monitor otherwise, grown on every
//! side by the window's frame. The normal (restored) placement is left
//! untouched; a minimized window is restored (without activation) first.

use core::mem::size_of;

use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
};
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GWL_STYLE, GetWindowLongPtrW, GetWindowPlacement, IsIconic, SW_SHOWMAXIMIZED,
    SW_SHOWMINIMIZED, SW_SHOWMINNOACTIVE, SW_SHOWNOACTIVATE, SW_SHOWNORMAL, SWP_FRAMECHANGED,
    SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_NOZORDER, SWP_SHOWWINDOW, SetWindowLongPtrW,
    SetWindowPlacement, SetWindowPos, WINDOW_EX_STYLE, WINDOW_STYLE, WINDOWPLACEMENT, WS_CAPTION,
    WS_MAXIMIZE, WS_MINIMIZE, WS_THICKFRAME,
};

use crate::hwnd::Hwnd;

use super::raw_hwnd;

/// Maximizes and shows `hwnd` on its current monitor without activating it.
pub(crate) fn maximize(hwnd: Hwnd) {
    let raw = raw_hwnd(hwnd);
    // SAFETY: read-only query on the window's own state.
    if unsafe { IsIconic(raw) }.as_bool() {
        // `WS_MINIMIZE` can't be cleared by restyling; a non-activating
        // placement restores the window to its normal rectangle first.
        let mut placement = WINDOWPLACEMENT {
            length: size_of::<WINDOWPLACEMENT>() as u32,
            ..Default::default()
        };
        // SAFETY: `placement` is sized for both calls.
        unsafe {
            let _ = GetWindowPlacement(raw, &mut placement);
            placement.showCmd = SW_SHOWNOACTIVATE.0 as u32;
            let _ = SetWindowPlacement(raw, &placement);
        }
    }
    let Some(rect) = maximized_rect(hwnd) else {
        return;
    };
    // SAFETY: sets the window's own style bit, then moves it; neither call
    // activates it (`SWP_NOACTIVATE`), and a stale handle is a no-op.
    unsafe {
        let style = GetWindowLongPtrW(raw, GWL_STYLE);
        SetWindowLongPtrW(raw, GWL_STYLE, style | WS_MAXIMIZE.0 as isize);
        let _ = SetWindowPos(
            raw,
            None,
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_SHOWWINDOW,
        );
    }
}

/// The rectangle the system gives `hwnd` when maximized: the work area for a
/// window with both a caption and a sizing frame, the full monitor otherwise,
/// grown on every side by the frame `AdjustWindowRectExForDpi` reports for the
/// window's style (0 for a bare popup, 1 for `WS_BORDER`, the resize border
/// plus padding for a captioned window).
fn maximized_rect(hwnd: Hwnd) -> Option<RECT> {
    let raw = raw_hwnd(hwnd);
    // SAFETY: read-only queries on a live window and its monitor; `info` is
    // sized for `GetMonitorInfoW`. A stale handle makes them fail harmlessly.
    unsafe {
        let monitor = MonitorFromWindow(raw, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return None;
        }
        let style = WINDOW_STYLE(GetWindowLongPtrW(raw, GWL_STYLE) as u32);
        let ex_style = WINDOW_EX_STYLE(GetWindowLongPtrW(raw, GWL_EXSTYLE) as u32);
        let sizable_caption = style.contains(WS_CAPTION) && style.contains(WS_THICKFRAME);
        let base = if sizable_caption {
            info.rcWork
        } else {
            info.rcMonitor
        };
        let mut frame = RECT::default();
        let _ = AdjustWindowRectExForDpi(
            &mut frame,
            style & !WS_MAXIMIZE & !WS_MINIMIZE,
            false,
            ex_style,
            GetDpiForWindow(raw),
        );
        let border = -frame.left;
        Some(RECT {
            left: base.left - border,
            top: base.top - border,
            right: base.right + border,
            bottom: base.bottom + border,
        })
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
        CreateWindowExW, DestroyWindow, GetWindowRect, IsZoomed, SW_SHOW, SW_SHOWMINNOACTIVE,
        ShowWindow, WS_BORDER, WS_DLGFRAME, WS_EX_NOACTIVATE, WS_OVERLAPPED, WS_OVERLAPPEDWINDOW,
        WS_POPUP, WS_SYSMENU,
    };
    use windows::core::w;

    use super::*;

    fn window(ex: WINDOW_EX_STYLE) -> windows::Win32::Foundation::HWND {
        styled(WS_OVERLAPPEDWINDOW, ex)
    }

    fn styled(style: WINDOW_STYLE, ex: WINDOW_EX_STYLE) -> windows::Win32::Foundation::HWND {
        // SAFETY: a parentless `STATIC` window, destroyed by the caller.
        unsafe {
            CreateWindowExW(
                ex,
                w!("STATIC"),
                w!(""),
                style,
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

    fn window_rect(raw: windows::Win32::Foundation::HWND) -> RECT {
        let mut rect = RECT::default();
        // SAFETY: `rect` outlives the call.
        unsafe {
            let _ = GetWindowRect(raw, &mut rect);
        }
        rect
    }

    /// The hand-made maximize lands exactly where the system's own does, for
    /// sizable captioned windows (work area) and every other frame (monitor).
    #[test]
    fn matches_the_system_maximized_rect() {
        let styles = [
            WS_OVERLAPPEDWINDOW,
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU,
            WS_POPUP,
            WS_POPUP | WS_THICKFRAME,
            WS_POPUP | WS_BORDER,
            WS_POPUP | WS_DLGFRAME,
        ];
        for style in styles {
            let system = styled(style, WINDOW_EX_STYLE(0));
            let manual = styled(style, WINDOW_EX_STYLE(0));
            // SAFETY: both windows are ours; destroyed below.
            unsafe {
                let _ = ShowWindow(system, SW_SHOWMAXIMIZED);
            }
            maximize(super::super::hwnd_from(manual));
            assert_eq!(
                window_rect(manual),
                window_rect(system),
                "style {:#x}",
                style.0
            );
            // SAFETY: each window is destroyed once.
            unsafe {
                assert!(IsZoomed(manual).as_bool(), "style {:#x}", style.0);
                let _ = DestroyWindow(system);
                let _ = DestroyWindow(manual);
            }
        }
    }

    /// A minimized no-activate window is restored, then maximized, without
    /// activation.
    #[test]
    fn maximizes_a_minimized_window_without_activation() {
        let active = window(WINDOW_EX_STYLE(0));
        let panel = window(WS_EX_NOACTIVATE);
        // SAFETY: both windows are ours and live; destroyed below.
        unsafe {
            let _ = ShowWindow(active, SW_SHOW);
            let _ = SetActiveWindow(active);
            let _ = ShowWindow(panel, SW_SHOWMINNOACTIVE);
            assert!(IsIconic(panel).as_bool());
        }
        maximize(super::super::hwnd_from(panel));
        // SAFETY: as above; each window is destroyed once.
        unsafe {
            assert!(!IsIconic(panel).as_bool(), "still minimized");
            assert!(IsZoomed(panel).as_bool(), "not maximized");
            assert_eq!(GetActiveWindow(), active, "maximize activated the window");
            let _ = DestroyWindow(panel);
            let _ = DestroyWindow(active);
        }
    }
}
