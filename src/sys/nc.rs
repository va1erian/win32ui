//! Non-client handling for the extended title bar.
//!
//! `WM_NCCALCSIZE` removes the standard caption while keeping the resize
//! borders (and insets a maximized window by the frame it overhangs the monitor
//! with). `WM_NCHITTEST` gives DWM first refusal, so the caption buttons — and
//! with them the Windows 11 snap layouts — keep working, then decides the rest
//! of the strip: an interactive widget is client area, the free strip is the
//! caption, and the borders resize.
//!
//! All `unsafe` in this crate lives under `sys`; every block below carries a
//! `// SAFETY:` note. Only documented APIs are used.

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::DwmDefWindowProc;
use windows::Win32::Graphics::Gdi::{ExcludeClipRect, HDC, RestoreDC, SaveDC};
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetSystemMetricsForDpi};
use windows::Win32::UI::WindowsAndMessaging::{
    CWP_SKIPDISABLED, CWP_SKIPINVISIBLE, ChildWindowFromPointEx, DefWindowProcW, GWL_EXSTYLE,
    GWL_STYLE, GetMenuBarInfo, GetWindowLongPtrW, GetWindowPlacement, IsZoomed, MENUBARINFO,
    NCCALCSIZE_PARAMS, OBJID_MENU, SM_CXPADDEDBORDER, SM_CYCAPTION, SM_CYSIZEFRAME,
    SW_SHOWMAXIMIZED, WINDOW_EX_STYLE, WINDOW_STYLE, WINDOWPLACEMENT, WM_ERASEBKGND, WM_NCCALCSIZE,
    WM_NCHITTEST, WS_CAPTION,
};

use crate::geometry::{Point, Rect};
use crate::hwnd::Hwnd;

use super::{hwnd_from, raw_hwnd};

mod frame;
mod geometry;

pub(crate) use frame::{
    apply_extended_frame, caption_buttons_in_window, client_mismatch, enable_extended, reframe,
    refresh_caption_inset,
};
use geometry::{FrameInsets, decide, extended_client_rect, hit_code};

/// The frame thickness of `hwnd`, with the caption height excluded from `top`.
fn frame_thickness(hwnd: HWND) -> FrameInsets {
    let dpi = super::dpi::window_dpi(hwnd_from(hwnd));
    let mut rect = RECT::default();
    // SAFETY: `hwnd` is live; the style/ex-style are read as plain integers and
    // `AdjustWindowRectExForDpi` only writes into `rect`.
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        // Drop the caption so the top inset is the border alone.
        let style = WINDOW_STYLE(style & !WS_CAPTION.0);
        if AdjustWindowRectExForDpi(&mut rect, style, false, WINDOW_EX_STYLE(ex_style), dpi)
            .is_err()
        {
            return FrameInsets::default();
        }
    }
    FrameInsets {
        left: -rect.left,
        top: -rect.top,
        right: rect.right,
        bottom: rect.bottom,
    }
}

fn is_maximized(hwnd: HWND) -> bool {
    // `IsZoomed` stays true through the de-maximize transition, so a restore
    // would be laid out as maximized. The placement's `showCmd` flips first.
    let mut placement = WINDOWPLACEMENT {
        length: size_of::<WINDOWPLACEMENT>() as u32,
        ..Default::default()
    };
    // SAFETY: `hwnd` is live; `placement` is a correctly-sized out-struct.
    let placed = unsafe { GetWindowPlacement(hwnd, &mut placement) }.is_ok();
    if placed {
        return placement.showCmd == SW_SHOWMAXIMIZED.0 as u32;
    }
    // SAFETY: `hwnd` is a live window; the call only reads its state.
    unsafe { IsZoomed(hwnd) }.as_bool()
}

/// The caption height including the top frame, in pixels.
fn caption_height(hwnd: HWND) -> i32 {
    let dpi = super::dpi::window_dpi(hwnd_from(hwnd));
    // SAFETY: `GetSystemMetricsForDpi` takes a metric index and a DPI.
    unsafe {
        GetSystemMetricsForDpi(SM_CYCAPTION, dpi)
            + GetSystemMetricsForDpi(SM_CYSIZEFRAME, dpi)
            + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi)
    }
}

/// The window's menu-bar rectangle in screen coordinates, or `None` when it has
/// no `HMENU` bar. `GetMenuBarInfo` reports `rcBar` in screen coordinates.
pub(crate) fn menu_bar_rect(hwnd: Hwnd) -> Option<Rect> {
    let mut info = MENUBARINFO {
        cbSize: size_of::<MENUBARINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `info` is a correctly-sized, initialised out-struct; `OBJID_MENU`
    // is the documented identifier for a window's menu bar.
    if unsafe { GetMenuBarInfo(raw_hwnd(hwnd), OBJID_MENU, 0, &mut info) }.is_err() {
        return None;
    }
    Some(Rect::new(
        info.rcBar.left,
        info.rcBar.top,
        info.rcBar.right,
        info.rcBar.bottom,
    ))
}

/// The window's menu-bar rectangle in client coordinates, or `None` when it has
/// no `HMENU` bar. The bar stays non-client (the system paints it) but lies in
/// the strip below the caption, inside the client rectangle.
fn menu_bar_client(hwnd: HWND) -> Option<Rect> {
    let screen = menu_bar_rect(hwnd_from(hwnd))?;
    let top_left = to_client(hwnd, Point::new(screen.left, screen.top));
    let bottom_right = to_client(hwnd, Point::new(screen.right, screen.bottom));
    Some(Rect::new(
        top_left.x,
        top_left.y,
        bottom_right.x,
        bottom_right.y,
    ))
}

/// The height of the extended strip DWM draws the caption buttons in, in
/// pixels, measured from the client's top. A maximized window's client already
/// starts below the frame it overhangs the monitor by, which the strip excludes.
/// The caption row's height in pixels: where DWM draws the caption buttons,
/// excluding any strip menu row below it.
fn caption_strip(hwnd: HWND) -> i32 {
    if is_maximized(hwnd) {
        caption_height(hwnd) - frame_thickness(hwnd).top
    } else {
        caption_height(hwnd)
    }
}

fn strip_height(hwnd: HWND) -> i32 {
    caption_strip(hwnd) + crate::window::nc::menu_row(hwnd_from(hwnd))
}

/// The caption row's height (device pixels) of an extended-frame window: the
/// part DWM draws the caption buttons in, without any strip menu row. Used to
/// lay the strip menu out.
pub(crate) fn caption_strip_height(hwnd: Hwnd) -> i32 {
    caption_strip(raw_hwnd(hwnd))
}

/// The top area an extended-frame window must reserve for its caption buttons
/// and menu bar, in pixels: the strip, extended to the bottom of the menu bar
/// when there is one. Content laid out by the app starts below it, so nothing
/// sits under the caption buttons or the menu bar.
pub(crate) fn title_bar_height(hwnd: Hwnd) -> i32 {
    let raw = raw_hwnd(hwnd);
    let menu_bottom = menu_bar_client(raw).map_or(0, |menu| menu.bottom);
    strip_height(raw).max(menu_bottom)
}

/// Reads the screen point from a mouse-message `lparam`.
fn screen_point(lparam: LPARAM) -> Point {
    let x = (lparam.0 & 0xFFFF) as i16 as i32;
    let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
    Point::new(x, y)
}

/// Converts a screen point to `hwnd`'s client coordinates.
fn to_client(hwnd: HWND, point: Point) -> Point {
    let mut raw = POINT {
        x: point.x,
        y: point.y,
    };
    // SAFETY: `hwnd` is live and `raw` is a valid in/out point.
    unsafe {
        let _ = windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut raw);
    }
    Point::new(raw.x, raw.y)
}

/// Whether the point is over a child that opted into caption clicks.
fn over_interactive(hwnd: HWND, client_point: Point) -> bool {
    let raw = POINT {
        x: client_point.x,
        y: client_point.y,
    };
    // SAFETY: `hwnd` is live and `raw` is a client point; the call only reads.
    let child = unsafe { ChildWindowFromPointEx(hwnd, raw, CWP_SKIPINVISIBLE | CWP_SKIPDISABLED) };
    !child.0.is_null() && crate::window::nc::is_caption_interactive(hwnd_from(child))
}

/// Handles `WM_NCCALCSIZE` for an extended-frame window, or `None` to let the
/// default apply.
pub(crate) fn calc_size(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    if wparam.0 == 0 || !crate::window::nc::is_extended(hwnd_from(hwnd)) {
        return None;
    }
    // Let the default compute the frame metrics first, so the resize borders
    // and the maximized placement stay native.
    // SAFETY: `hwnd` is live and the message fields follow the documented
    // `WM_NCCALCSIZE` contract.
    let _ = unsafe { DefWindowProcW(hwnd, WM_NCCALCSIZE, wparam, lparam) };
    // SAFETY: with `wparam` TRUE, `lparam` is a `NCCALCSIZE_PARAMS*` owned
    // by the system for the duration of the message; `rgrc[1]` is the window
    // rect.
    let params = unsafe { &mut *(lparam.0 as *mut NCCALCSIZE_PARAMS) };
    let window = Rect::new(
        params.rgrc[1].left,
        params.rgrc[1].top,
        params.rgrc[1].right,
        params.rgrc[1].bottom,
    );
    // Windows sends the maximize transition's `WM_NCCALCSIZE` with the *old*
    // window rectangle, so the client would stay small; the window is then
    // moved without a second `WM_NCCALCSIZE`. The mismatch is detected in
    // `WM_SIZE` (see [`client_mismatch`]) and a frame change forces the client
    // to be recomputed against the final window rectangle.
    let client = extended_client_rect(window, frame_thickness(hwnd), is_maximized(hwnd));
    params.rgrc[0] = RECT {
        left: client.left,
        top: client.top,
        right: client.right,
        bottom: client.bottom,
    };
    // DWM drops the caption buttons when the caption is removed; extending the
    // frame over the strip draws them back (and lets the backdrop show there).
    // Re-applied here because `DefWindowProc` resets the frame on `WM_NCCALCSIZE`.
    apply_extended_frame(hwnd_from(hwnd));
    Some(LRESULT(0))
}

/// Handles `WM_NCHITTEST` for an extended-frame window, or `None` to let the
/// default apply.
pub(crate) fn hit_test(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    if !crate::window::nc::is_extended(hwnd_from(hwnd)) {
        return None;
    }
    // DWM handles the caption buttons first, so hovering the maximize button
    // still opens the snap-layout flyout.
    let mut result = LRESULT(0);
    // SAFETY: `hwnd` is live; `DwmDefWindowProc` only writes through `result`.
    if unsafe { DwmDefWindowProc(hwnd, WM_NCHITTEST, wparam, lparam, &mut result) }.as_bool() {
        return Some(result);
    }
    let point = screen_point(lparam);
    // DWM stops answering the caption buttons when the window is maximized, so
    // hit-test them from the bounds DWM still reports.
    if let Some(code) = frame::caption_button_hit(hwnd, point) {
        return Some(LRESULT(code as isize));
    }
    // The menu bar stays non-client, positioned below the removed caption; the
    // default non-client hit-testing answers it (and opens the menu), so defer
    // to `DefWindowProc` for points inside it.
    if menu_bar_rect(hwnd_from(hwnd)).is_some_and(|menu| menu.contains(point)) {
        return None;
    }
    let point = to_client(hwnd, point);
    let client = super::window::client_rect(hwnd_from(hwnd));
    // A strip menu item accepts the click as client area (so the app opens its
    // popup); the free strip around it still drags the window.
    let interactive =
        over_interactive(hwnd, point) || crate::window::nc::over_menu_item(hwnd_from(hwnd), point);
    let mut frame = frame_thickness(hwnd);
    if is_maximized(hwnd) {
        frame.top = 0;
    }
    let hit = decide(point, client, frame, strip_height(hwnd), interactive);
    Some(LRESULT(hit_code(hit) as isize))
}

/// Handles `WM_ERASEBKGND` for an extended-frame window, or `None` to let the
/// default apply. The client erases to the theme background, except the menu
/// bar (the system paints that as non-client, under the client's pixels); the
/// strip is then cleared to black (DWM's "glass" colour), so DWM composes the
/// caption buttons and the backdrop material over it. The strip is cleared
/// regardless of the backdrop: DWM draws the buttons in the extended-frame
/// layer *under* the client, so an opaque strip fill would hide them.
pub(crate) fn erase_background(hwnd: HWND, wparam: WPARAM) -> Option<LRESULT> {
    let window = hwnd_from(hwnd);
    if !crate::window::nc::is_extended(window) {
        return None;
    }
    let dc = HDC(wparam.0 as *mut core::ffi::c_void);
    let saved = menu_bar_client(hwnd).map(|menu| {
        // SAFETY: `dc` is the message's paint DC, live for the duration of the
        // message; the clip is restored below.
        let saved = unsafe { SaveDC(dc) };
        unsafe { ExcludeClipRect(dc, menu.left, menu.top, menu.right, menu.bottom) };
        saved
    });
    // The default erase paints the theme background (the class brush) across
    // the client area left in the clip.
    // SAFETY: `hwnd` is live; `WM_ERASEBKGND`'s `wparam` is the paint DC and
    // `lparam` is unused.
    let _ = unsafe { DefWindowProcW(hwnd, WM_ERASEBKGND, wparam, LPARAM(0)) };
    if let Some(saved) = saved {
        // SAFETY: `saved` is the state `SaveDC` returned for this same DC.
        let _ = unsafe { RestoreDC(dc, saved) };
    }
    let client = super::window::client_rect(window);
    if let Some(brush) = crate::gdi::cache_brush(crate::color::Color::rgb(0, 0, 0)) {
        let strip = Rect::new(0, 0, client.right, strip_height(hwnd).min(client.bottom));
        super::gdi::fill_rect(dc, strip, brush);
        // The material status bar band is glass too: its pixels are cleared to
        // black so the top-level Direct2D surface can draw over it with alpha.
        let band = crate::window::nc::status_bar(window);
        if band > 0 {
            let bottom = Rect::new(
                0,
                (client.bottom - band).max(0),
                client.right,
                client.bottom,
            );
            super::gdi::fill_rect(dc, bottom, brush);
        }
    }
    Some(LRESULT(1))
}
