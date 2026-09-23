//! Window placement, monitor work areas, size limits and per-window input
//! state: the raw-Win32 helpers behind the `Window` extension methods.

use core::cell::RefCell;
use core::mem::size_of;
use std::collections::HashMap;

use windows::Win32::Foundation::{LPARAM, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowPlacement, MINMAXINFO, SW_SHOWMAXIMIZED, SW_SHOWMINIMIZED, SW_SHOWMINNOACTIVE,
    SW_SHOWNORMAL, SetWindowPlacement, WINDOWPLACEMENT, WINDOWPLACEMENT_FLAGS,
};
use windows::core::BOOL;

use crate::error::Result;
use crate::geometry::{Rect, Size};
use crate::hwnd::Hwnd;
use crate::window::{Placement, ShowState};

use super::{raw_hwnd, win32_error};

/// The window's current placement, or `None` if it is gone.
pub(crate) fn get_placement(hwnd: Hwnd) -> Option<Placement> {
    let mut placement = WINDOWPLACEMENT {
        length: size_of::<WINDOWPLACEMENT>() as u32,
        ..Default::default()
    };
    // SAFETY: `placement` is a valid out-pointer whose `length` field describes
    // its size, as `GetWindowPlacement` requires.
    if unsafe { GetWindowPlacement(raw_hwnd(hwnd), &mut placement) }.is_err() {
        return None;
    }
    Some(Placement {
        normal: rect_from(placement.rcNormalPosition),
        show: show_state_from(placement.showCmd),
    })
}

/// Applies a placement, including its show state.
pub(crate) fn set_placement(hwnd: Hwnd, placement: &Placement) -> Result<()> {
    let value = WINDOWPLACEMENT {
        length: size_of::<WINDOWPLACEMENT>() as u32,
        flags: WINDOWPLACEMENT_FLAGS(0),
        showCmd: show_cmd(placement.show),
        ptMinPosition: POINT { x: 0, y: 0 },
        ptMaxPosition: POINT { x: 0, y: 0 },
        rcNormalPosition: rect_of(placement.normal),
    };
    // SAFETY: `value` is fully initialised with `length` set as required.
    unsafe { SetWindowPlacement(raw_hwnd(hwnd), &value) }.map_err(win32_error)
}

/// The work area (screen minus taskbar and docked bars) of every monitor, in
/// virtual-screen coordinates.
pub(crate) fn monitor_work_areas() -> Vec<Rect> {
    unsafe extern "system" fn collect(
        monitor: HMONITOR,
        _dc: HDC,
        _rect: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        // SAFETY: `data` is the `&mut Vec<Rect>` passed to `EnumDisplayMonitors`
        // below, which outlives every callback it makes.
        let areas = unsafe { &mut *(data.0 as *mut Vec<Rect>) };
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            rcMonitor: RECT::default(),
            rcWork: RECT::default(),
            dwFlags: 0,
        };
        // SAFETY: `info` is a valid out-pointer with `cbSize` set.
        if unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
            areas.push(rect_from(info.rcWork));
        }
        BOOL(1)
    }

    let mut areas = Vec::new();
    // SAFETY: `areas` outlives the call and the callback only appends to it.
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(collect),
            LPARAM(&mut areas as *mut Vec<Rect> as isize),
        );
    }
    areas
}

#[derive(Clone, Copy, Default)]
struct TrackLimits {
    min: Option<Size>,
    max: Option<Size>,
}

thread_local! {
    /// Size limits set with `set_min_size`/`set_max_size`, applied to every
    /// `WM_GETMINMAXINFO` for the window. Windows are thread-affine, so the
    /// owning thread's map is the one that serves their messages.
    static TRACK_LIMITS: RefCell<HashMap<isize, TrackLimits>> = RefCell::new(HashMap::new());
}

/// Records a minimum tracking size; a non-positive size clears the limit.
pub(crate) fn set_min_size(hwnd: Hwnd, size: Size) {
    update_track_limits(hwnd, |limits| {
        limits.min = (!size.is_empty()).then_some(size);
    });
}

/// Records a maximum tracking size; a non-positive size clears the limit.
pub(crate) fn set_max_size(hwnd: Hwnd, size: Size) {
    update_track_limits(hwnd, |limits| {
        limits.max = (!size.is_empty()).then_some(size);
    });
}

fn update_track_limits(hwnd: Hwnd, update: impl FnOnce(&mut TrackLimits)) {
    TRACK_LIMITS.with(|map| {
        update(map.borrow_mut().entry(hwnd.raw() as isize).or_default());
    });
}

/// Writes the recorded size limits into the `MINMAXINFO` of a
/// `WM_GETMINMAXINFO` (a no-op when no limits are set).
pub(crate) fn apply_track_limits(hwnd: Hwnd, lparam: isize) {
    if lparam == 0 {
        return;
    }
    let limits = TRACK_LIMITS.with(|map| map.borrow().get(&(hwnd.raw() as isize)).copied());
    let Some(limits) = limits else {
        return;
    };
    // SAFETY: for `WM_GETMINMAXINFO`, `lparam` points at a `MINMAXINFO` owned by
    // the system for the duration of the message.
    let info = unsafe { &mut *(lparam as *mut MINMAXINFO) };
    if let Some(min) = limits.min {
        info.ptMinTrackSize = POINT {
            x: min.width,
            y: min.height,
        };
    }
    if let Some(max) = limits.max {
        info.ptMaxTrackSize = POINT {
            x: max.width,
            y: max.height,
        };
    }
}

/// Drops a destroyed window's size limits.
pub(crate) fn forget_track_limits(hwnd: Hwnd) {
    TRACK_LIMITS.with(|map| {
        map.borrow_mut().remove(&(hwnd.raw() as isize));
    });
}

fn rect_from(rect: RECT) -> Rect {
    Rect::new(rect.left, rect.top, rect.right, rect.bottom)
}

fn rect_of(rect: Rect) -> RECT {
    RECT {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
    }
}

fn show_cmd(show: ShowState) -> u32 {
    match show {
        ShowState::Normal => SW_SHOWNORMAL.0 as u32,
        ShowState::Minimized => SW_SHOWMINIMIZED.0 as u32,
        ShowState::Maximized => SW_SHOWMAXIMIZED.0 as u32,
    }
}

fn show_state_from(cmd: u32) -> ShowState {
    if cmd == SW_SHOWMAXIMIZED.0 as u32 {
        ShowState::Maximized
    } else if cmd == SW_SHOWMINIMIZED.0 as u32 || cmd == SW_SHOWMINNOACTIVE.0 as u32 {
        ShowState::Minimized
    } else {
        ShowState::Normal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn applied(hwnd: Hwnd) -> MINMAXINFO {
        let mut info = MINMAXINFO::default();
        apply_track_limits(hwnd, &mut info as *mut MINMAXINFO as isize);
        info
    }

    #[test]
    fn track_limits_are_written_into_minmax_info() {
        let hwnd = Hwnd::from_raw(0x1234);
        set_min_size(hwnd, Size::new(200, 150));
        set_max_size(hwnd, Size::new(900, 700));

        let info = applied(hwnd);
        assert_eq!((info.ptMinTrackSize.x, info.ptMinTrackSize.y), (200, 150));
        assert_eq!((info.ptMaxTrackSize.x, info.ptMaxTrackSize.y), (900, 700));

        forget_track_limits(hwnd);
        let cleared = applied(hwnd);
        assert_eq!(
            (cleared.ptMinTrackSize.x, cleared.ptMinTrackSize.y),
            (0, 0),
            "limits survived the window's destruction"
        );
    }

    #[test]
    fn non_positive_size_clears_the_limit() {
        let hwnd = Hwnd::from_raw(0x2345);
        set_min_size(hwnd, Size::new(100, 100));
        set_min_size(hwnd, Size::default());

        let info = applied(hwnd);
        assert_eq!((info.ptMinTrackSize.x, info.ptMinTrackSize.y), (0, 0));
        forget_track_limits(hwnd);
    }

    #[test]
    fn show_states_round_trip() {
        for show in [
            ShowState::Normal,
            ShowState::Minimized,
            ShowState::Maximized,
        ] {
            assert_eq!(show_state_from(show_cmd(show)), show);
        }
    }
}
