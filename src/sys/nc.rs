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
use windows::Win32::Graphics::Dwm::{
    DWMWA_CAPTION_BUTTON_BOUNDS, DwmDefWindowProc, DwmGetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::HDC;
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetSystemMetricsForDpi};
use windows::Win32::UI::WindowsAndMessaging::{
    CWP_SKIPDISABLED, CWP_SKIPINVISIBLE, ChildWindowFromPointEx, DefWindowProcW, GWL_EXSTYLE,
    GWL_STYLE, GetMenuBarInfo, GetWindowLongPtrW, HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCAPTION,
    HTCLIENT, HTLEFT, HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT, MENUBARINFO, NCCALCSIZE_PARAMS,
    OBJID_MENU, SM_CXPADDEDBORDER, SM_CYCAPTION, SM_CYSIZEFRAME, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_ERASEBKGND, WM_NCCALCSIZE, WM_NCHITTEST, WS_CAPTION,
};

use crate::geometry::{Point, Rect};
use crate::hwnd::Hwnd;

use super::{hwnd_from, raw_hwnd};

/// The frame borders around an extended-frame window's client area, in pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct FrameInsets {
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) right: i32,
    pub(crate) bottom: i32,
}

/// The client rectangle for a window whose caption has been removed: `window`
/// inset by the frame on every side.
///
/// A maximized window overhangs the monitor by the frame width, so the same
/// inset lands the client on the monitor rather than letting content hang off
/// it. Pure, so the arithmetic is unit-tested.
fn extended_client_rect(window: Rect, frame: FrameInsets) -> Rect {
    Rect::new(
        window.left + frame.left,
        window.top + frame.top,
        window.right - frame.right,
        window.bottom - frame.bottom,
    )
}

/// What a hit-test point falls on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Hit {
    Caption,
    Client,
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Decides what a hit-test `point` (client coordinates) is: a resize border,
/// the draggable caption strip, or ordinary client content. `over_interactive`
/// is true when the point is over a widget that opted into caption clicks.
pub(crate) fn decide(
    point: Point,
    client: Rect,
    frame: FrameInsets,
    caption_height: i32,
    over_interactive: bool,
) -> Hit {
    let left = point.x < client.left + frame.left;
    let right = point.x >= client.right - frame.right;
    let top = point.y < client.top + frame.top;
    let bottom = point.y >= client.bottom - frame.bottom;
    match (left, right, top, bottom) {
        (true, _, true, _) => Hit::TopLeft,
        (_, true, true, _) => Hit::TopRight,
        (true, _, _, true) => Hit::BottomLeft,
        (_, true, _, true) => Hit::BottomRight,
        (true, _, _, _) => Hit::Left,
        (_, true, _, _) => Hit::Right,
        (_, _, true, _) => Hit::Top,
        (_, _, _, true) => Hit::Bottom,
        _ if point.y < client.top + caption_height && !over_interactive => Hit::Caption,
        _ => Hit::Client,
    }
}

/// The `HT*` code Windows expects for `hit`.
fn hit_code(hit: Hit) -> u32 {
    match hit {
        Hit::Caption => HTCAPTION,
        Hit::Client => HTCLIENT,
        Hit::Left => HTLEFT,
        Hit::Right => HTRIGHT,
        Hit::Top => HTTOP,
        Hit::Bottom => HTBOTTOM,
        Hit::TopLeft => HTTOPLEFT,
        Hit::TopRight => HTTOPRIGHT,
        Hit::BottomLeft => HTBOTTOMLEFT,
        Hit::BottomRight => HTBOTTOMRIGHT,
    }
}

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

/// The caption strip's height for `hwnd`, in pixels.
fn caption_height(hwnd: HWND) -> i32 {
    let dpi = super::dpi::window_dpi(hwnd_from(hwnd));
    // SAFETY: `GetSystemMetricsForDpi` takes a metric index and a DPI.
    unsafe {
        GetSystemMetricsForDpi(SM_CYCAPTION, dpi)
            + GetSystemMetricsForDpi(SM_CYSIZEFRAME, dpi)
            + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi)
    }
}

/// The window's menu-bar height in pixels, or 0 when it has no `HMENU` bar.
fn menu_bar_height(hwnd: HWND) -> i32 {
    let mut info = MENUBARINFO {
        cbSize: size_of::<MENUBARINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `info` is a correctly-sized, initialised out-struct; `OBJID_MENU`
    // is the documented identifier for a window's menu bar.
    if unsafe { GetMenuBarInfo(hwnd, OBJID_MENU, 0, &mut info) }.is_err() {
        return 0;
    }
    info.rcBar.bottom - info.rcBar.top
}

/// The top strip an extended-frame window must reserve for its caption buttons
/// and menu bar, in pixels: the caption (incl. its top frame) plus the menu-bar
/// height. Content laid out by the app starts below it.
pub(crate) fn title_bar_height(hwnd: Hwnd) -> i32 {
    let raw = raw_hwnd(hwnd);
    caption_height(raw) + menu_bar_height(raw)
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
    // SAFETY: with `wparam` TRUE, `lparam` is a `NCCALCSIZE_PARAMS*` owned by
    // the system for the duration of the message; `rgrc[1]` is the window rect.
    let params = unsafe { &mut *(lparam.0 as *mut NCCALCSIZE_PARAMS) };
    let window = Rect::new(
        params.rgrc[1].left,
        params.rgrc[1].top,
        params.rgrc[1].right,
        params.rgrc[1].bottom,
    );
    let client = extended_client_rect(window, frame_thickness(hwnd));
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
    let point = to_client(hwnd, screen_point(lparam));
    let client = super::window::client_rect(hwnd_from(hwnd));
    let interactive = over_interactive(hwnd, point);
    let hit = decide(
        point,
        client,
        frame_thickness(hwnd),
        caption_height(hwnd),
        interactive,
    );
    Some(LRESULT(hit_code(hit) as isize))
}

/// Re-reads the caption buttons' bounds from DWM and records them (client
/// coordinates) for `hwnd`. Returns the bounds, or `None` when DWM has none
/// (a standard window, or a platform without the attribute).
pub(crate) fn refresh_caption_inset(hwnd: Hwnd) -> Option<Rect> {
    let mut raw = RECT::default();
    // SAFETY: `hwnd` is live and `raw` is a correctly-sized out-pointer for
    // `DWMWA_CAPTION_BUTTON_BOUNDS`, which returns screen coordinates.
    let result = unsafe {
        DwmGetWindowAttribute(
            raw_hwnd(hwnd),
            DWMWA_CAPTION_BUTTON_BOUNDS,
            &mut raw as *mut RECT as *mut core::ffi::c_void,
            size_of::<RECT>() as u32,
        )
    };
    if result.is_err() {
        return None;
    }
    let bounds = Rect::new(raw.left, raw.top, raw.right, raw.bottom);
    // The bounds are in screen coordinates; store them relative to the client.
    let top_left = to_client(raw_hwnd(hwnd), Point::new(bounds.left, bounds.top));
    let bottom_right = to_client(raw_hwnd(hwnd), Point::new(bounds.right, bounds.bottom));
    let client = Rect::new(top_left.x, top_left.y, bottom_right.x, bottom_right.y);
    crate::window::nc::set_caption_inset(hwnd, client);
    Some(client)
}

/// Marks `hwnd` as using the extended title bar, reads its caption inset and
/// extends the frame over the caption strip.
pub(crate) fn enable_extended(hwnd: Hwnd) {
    crate::window::nc::set_extended(hwnd, true);
    apply_extended_frame(hwnd);
    let _ = refresh_caption_inset(hwnd);
}

/// Extends DWM's frame over the caption strip of an extended-frame `window` and
/// records the strip height. The strip is where DWM draws the caption buttons
/// and, with a backdrop, the material. Re-apply after `WM_NCCALCSIZE`, on
/// resize and on DPI change (the strip height is DPI-dependent).
pub(crate) fn apply_extended_frame(window: Hwnd) {
    if !crate::window::nc::is_extended(window) {
        return;
    }
    let height = caption_height(raw_hwnd(window));
    crate::window::nc::set_strip_height(window, height);
    super::dwm::extend_frame(window, height);
}

/// Handles `WM_ERASEBKGND` for an extended-frame window, or `None` to let the
/// default apply. The client erases to the theme background; the caption strip
/// is then cleared to black (DWM's "glass" colour) so a backdrop material shows
/// through it. Without a backdrop the strip stays the solid theme background.
pub(crate) fn erase_background(hwnd: HWND, wparam: WPARAM) -> Option<LRESULT> {
    let window = hwnd_from(hwnd);
    if !crate::window::nc::is_extended(window) {
        return None;
    }
    // The default erase paints the theme background (the class brush) across
    // the whole client.
    // SAFETY: `hwnd` is live; `WM_ERASEBKGND`'s `wparam` is the paint DC and
    // `lparam` is unused.
    let _ = unsafe { DefWindowProcW(hwnd, WM_ERASEBKGND, wparam, LPARAM(0)) };
    if crate::theme::backdrop_active(window) {
        let height = crate::window::nc::strip_height(window);
        if height > 0 {
            let client = super::window::client_rect(window);
            let strip = Rect::new(0, 0, client.right, height.min(client.bottom));
            if let Some(brush) = crate::gdi::cache_brush(crate::color::Color::rgb(0, 0, 0)) {
                // SAFETY: `wparam` is the message's paint DC, live for the
                // duration of the message; `strip` is inside the client.
                super::gdi::fill_rect(HDC(wparam.0 as *mut core::ffi::c_void), strip, brush);
            }
        }
    }
    Some(LRESULT(1))
}

#[cfg(test)]
mod tests {
    use crate::geometry::{Point, Rect};

    use super::{FrameInsets, Hit, decide, extended_client_rect};

    fn frame() -> FrameInsets {
        FrameInsets {
            left: 8,
            top: 4,
            right: 8,
            bottom: 8,
        }
    }

    #[test]
    fn client_rect_removes_the_caption_and_insets_every_border() {
        let window = Rect::new(0, 0, 800, 600);
        assert_eq!(
            extended_client_rect(window, frame()),
            Rect::new(8, 4, 792, 592)
        );
    }

    #[test]
    fn maximized_overhang_is_inset_away() {
        // A maximized window overhangs the monitor by the frame on every side.
        let window = Rect::new(-8, -4, 1928, 1044);
        assert_eq!(
            extended_client_rect(window, frame()),
            Rect::new(0, 0, 1920, 1036)
        );
    }

    #[test]
    fn borders_win_over_the_caption_strip() {
        let client = Rect::new(8, 4, 792, 592);
        let frame = frame();
        let height = 32;
        // The top-left corner is a resize handle, not the caption.
        assert_eq!(
            decide(Point::new(2, 2), client, frame, height, false),
            Hit::TopLeft
        );
        assert_eq!(
            decide(Point::new(400, 2), client, frame, height, false),
            Hit::Top
        );
        assert_eq!(
            decide(Point::new(2, 300), client, frame, height, false),
            Hit::Left
        );
        assert_eq!(
            decide(Point::new(790, 300), client, frame, height, false),
            Hit::Right
        );
        assert_eq!(
            decide(Point::new(400, 590), client, frame, height, false),
            Hit::Bottom
        );
    }

    #[test]
    fn free_strip_drags_and_widgets_stay_client() {
        let client = Rect::new(8, 4, 792, 592);
        let frame = frame();
        let height = 32;
        assert_eq!(
            decide(Point::new(400, 20), client, frame, height, false),
            Hit::Caption
        );
        assert_eq!(
            decide(Point::new(400, 20), client, frame, height, true),
            Hit::Client,
            "an interactive widget accepts the click instead of dragging"
        );
        assert_eq!(
            decide(Point::new(400, 100), client, frame, height, false),
            Hit::Client,
            "content below the strip is client area"
        );
    }
}
