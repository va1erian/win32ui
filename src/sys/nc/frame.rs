//! The extended frame's lifecycle: applying it, keeping it right across
//! maximize/de-maximize, and hit-testing the caption buttons DWM draws.
//!
//! Split from [`super`] so that file stays under the size limit; the pure
//! geometry and the plain `WM_NCCALCSIZE`/`WM_NCHITTEST` dispatch stay there.

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Dwm::{DWMWA_CAPTION_BUTTON_BOUNDS, DwmGetWindowAttribute};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, HTCLOSE, HTMAXBUTTON, HTMINBUTTON, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SWP_NOZORDER, SetWindowPos,
};

use crate::geometry::{Point, Rect};
use crate::hwnd::Hwnd;

use super::geometry::extended_client_rect;
use super::{frame_thickness, is_maximized, raw_hwnd, strip_height, to_client};

/// Whether `hwnd`'s client rectangle no longer matches what its current window
/// rectangle calls for (Windows moved the window during a maximize/de-maximize
/// without a fresh `WM_NCCALCSIZE`). The app forces a frame change so the
/// client is recomputed against the final rectangle. A pixel of slack absorbs
/// the clamping Windows does for a maximized window.
pub(crate) fn client_mismatch(hwnd: Hwnd) -> bool {
    if !crate::window::nc::is_extended(hwnd) {
        return false;
    }
    let raw = raw_hwnd(hwnd);
    let window = crate::sys::window::window_rect(hwnd);
    let maximized = is_maximized(raw);
    let frame = frame_thickness(raw);
    let rect = Rect::new(window.left, window.top, window.right, window.bottom);
    // A maximized borderless window's client is the monitor work area; Windows
    // clamps our computed rectangle to it, so expect that.
    let expected = if maximized {
        maximized_client(raw).unwrap_or_else(|| extended_client_rect(rect, frame, true))
    } else {
        extended_client_rect(rect, frame, false)
    };
    let actual = crate::sys::window::client_rect(hwnd);
    let near = |a: i32, b: i32| (a - b).abs() <= 2;
    !(near(actual.left, expected.left)
        && near(actual.top, expected.top)
        && near(actual.right, expected.right)
        && near(actual.bottom, expected.bottom))
}

/// The work area of `hwnd`'s monitor, which is the client rectangle Windows
/// gives a maximized borderless window, or `None` when it cannot be queried.
fn maximized_client(hwnd: HWND) -> Option<Rect> {
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    };
    // SAFETY: `hwnd` is live; the call only reads its monitor.
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    if monitor.0.is_null() {
        return None;
    }
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `info` is a correctly-sized, initialised out-struct.
    if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        return None;
    }
    Some(Rect::new(
        info.rcWork.left,
        info.rcWork.top,
        info.rcWork.right,
        info.rcWork.bottom,
    ))
}

/// Recomputes an extended window's non-client area against its current window
/// rectangle (used after a maximize/de-maximize moved it without one).
pub(crate) fn reframe(hwnd: Hwnd) {
    force_frame_change(hwnd);
}

/// The `HT*` code for the caption button under the screen point, or `None`.
///
/// `DwmDefWindowProc` stops hit-testing the caption buttons of a *maximized*
/// borderless window (their bounds are still reported by DWM), so this is the
/// fallback that keeps minimize/maximize/close working there.
pub(crate) fn caption_button_hit(hwnd: HWND, screen: Point) -> Option<u32> {
    let bounds = caption_button_bounds(hwnd)?;
    let width = bounds.right - bounds.left;
    if width <= 0 {
        return None;
    }
    let mut window = RECT::default();
    // SAFETY: `window` is a valid out-pointer.
    let _ = unsafe { GetWindowRect(hwnd, &mut window) };
    let x = screen.x - window.left;
    let y = screen.y - window.top;
    if x < bounds.left || x >= bounds.right || y < bounds.top || y >= bounds.bottom {
        return None;
    }
    let third = (width / 3).max(1);
    let code = if x - bounds.left < third {
        HTMINBUTTON
    } else if x - bounds.left < third * 2 {
        HTMAXBUTTON
    } else {
        HTCLOSE
    };
    Some(code)
}

/// Reads the caption buttons' bounds from DWM in window coordinates, or `None`.
fn caption_button_bounds(hwnd: HWND) -> Option<RECT> {
    let mut bounds = RECT::default();
    // SAFETY: `hwnd` is live; `bounds` is a correctly-sized out-struct, and
    // `DWMWA_CAPTION_BUTTON_BOUNDS` returns window-relative coordinates.
    let result = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CAPTION_BUTTON_BOUNDS,
            &mut bounds as *mut RECT as *mut core::ffi::c_void,
            size_of::<RECT>() as u32,
        )
    };
    if result.is_err() {
        return None;
    }
    Some(bounds)
}

/// Reads the caption buttons' bounds from DWM, relative to the window's top-left
/// corner (not the client's, and not the screen), or `None`
/// when DWM has none (a standard window, or a platform without the attribute).
pub(crate) fn caption_buttons_in_window(hwnd: Hwnd) -> Option<Rect> {
    let raw = caption_button_bounds(raw_hwnd(hwnd))?;
    Some(Rect::new(raw.left, raw.top, raw.right, raw.bottom))
}

/// Re-reads the caption buttons' bounds from DWM and records them (client
/// coordinates) for `hwnd`. Returns the bounds, or `None` when DWM has none.
pub(crate) fn refresh_caption_inset(hwnd: Hwnd) -> Option<Rect> {
    let bounds = caption_buttons_in_window(hwnd)?;
    // Window-relative to client-relative: the client's origin is where the
    // window's top-left corner sits in client coordinates, negated.
    let window = crate::sys::window::window_rect(hwnd);
    let origin = to_client(raw_hwnd(hwnd), Point::new(window.left, window.top));
    let client = Rect::new(
        bounds.left + origin.x,
        bounds.top + origin.y,
        bounds.right + origin.x,
        bounds.bottom + origin.y,
    );
    crate::window::nc::set_caption_inset(hwnd, client);
    Some(client)
}

/// Marks `hwnd` as using the extended title bar, reads its caption inset and
/// extends the frame over the caption strip. Returns whether DWM accepted the
/// extended frame.
pub(crate) fn enable_extended(hwnd: Hwnd) -> bool {
    crate::window::nc::set_extended(hwnd, true);
    // The window was created with a standard caption, so its client area was
    // first computed above the caption row. Ask Windows to recompute the
    // non-client area now that the flag is set, so the client — and with it the
    // strip's Direct2D surface — starts at the window's top edge.
    force_frame_change(hwnd);
    let extended = apply_extended_frame(hwnd);
    let _ = refresh_caption_inset(hwnd);
    extended
}

/// Recomputes `hwnd`'s non-client area (and sends `WM_NCCALCSIZE`), so the
/// extended-frame client rectangle takes effect without moving or resizing.
fn force_frame_change(hwnd: Hwnd) {
    // SAFETY: `hwnd` is live; the position/size and z-order are untouched and
    // only the frame is recalculated.
    unsafe {
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

/// Records the caption-strip height of an extended-frame `window` and extends
/// DWM's frame over it. The strip is where DWM draws the caption buttons and,
/// with a backdrop, the material. Re-apply after `WM_NCCALCSIZE`, on resize and
/// on DPI change (the strip height is DPI-dependent).
pub(crate) fn apply_extended_frame(window: Hwnd) -> bool {
    if !crate::window::nc::is_extended(window) {
        return false;
    }
    let raw = raw_hwnd(window);
    let height = strip_height(raw);
    crate::window::nc::set_strip_height(window, height);
    // A material top bar's band sits below the strip (and any menu bar) and
    // shows the material too; its native children paint opaquely (see
    // `sys::glass_child`).
    let top = match crate::window::nc::top_bar(window) {
        0 => height,
        band => super::title_bar_height(window) + band,
    };
    let bottom = crate::window::nc::status_bar(window);
    crate::sys::dwm::extend_frame(window, top, bottom)
}
