//! Native scrollbar plumbing: `SCROLLINFO`, the `WM_VSCROLL` request codes,
//! and a subclass that forwards a child's wheel input to its scroll container.
//!
//! All `unsafe` in this crate lives under `sys`; every block below carries a
//! `// SAFETY:` note. Only documented APIs are used.

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::UpdateWindow;
use windows::Win32::UI::Controls::{SetScrollInfo, SetScrollPos};
use windows::Win32::UI::Shell::DefSubclassProc;
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_STYLE, GetScrollInfo, GetWindowLongPtrW, SB_BOTTOM, SB_ENDSCROLL, SB_LINEDOWN, SB_LINEUP,
    SB_PAGEDOWN, SB_PAGEUP, SB_THUMBPOSITION, SB_THUMBTRACK, SB_TOP, SB_VERT, SCROLLINFO,
    SIF_DISABLENOSCROLL, SIF_PAGE, SIF_POS, SIF_RANGE, SIF_TRACKPOS, SW_ERASE, SW_INVALIDATE,
    SW_SCROLLCHILDREN, SWP_FRAMECHANGED, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, ScrollWindowEx,
    SetWindowLongPtrW, SetWindowPos, WHEEL_DELTA, WM_MOUSEWHEEL, WM_VSCROLL, WS_VSCROLL,
};

use crate::hwnd::Hwnd;

use super::raw_hwnd;

/// The `WM_VSCROLL` scroll-bar request, decoded from `wparam`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScrollRequest {
    /// One line up.
    LineUp,
    /// One line down.
    LineDown,
    /// One page up.
    PageUp,
    /// One page down.
    PageDown,
    /// Jump to the top.
    Top,
    /// Jump to the bottom.
    Bottom,
    /// The user is dragging the thumb; the value is the live track position.
    ThumbTrack(i32),
    /// The thumb was released at this position.
    ThumbPosition(i32),
    /// The scroll gesture ended.
    EndScroll,
}

/// The wheel rotation unit, `WHEEL_DELTA` (from `WinUser.h`, via the `windows`
/// crate).
pub(crate) fn wheel_delta() -> i32 {
    WHEEL_DELTA as i32
}

/// Whether `code` is the `WM_VSCROLL` message id.
pub(crate) fn is_vscroll(code: u32) -> bool {
    code == WM_VSCROLL
}

/// Decodes a `WM_VSCROLL` `wparam`.
pub(crate) fn decode_vscroll(wparam: usize) -> Option<ScrollRequest> {
    let code = (wparam & 0xffff) as u16;
    Some(match code as i32 {
        c if c == SB_LINEUP.0 => ScrollRequest::LineUp,
        c if c == SB_LINEDOWN.0 => ScrollRequest::LineDown,
        c if c == SB_PAGEUP.0 => ScrollRequest::PageUp,
        c if c == SB_PAGEDOWN.0 => ScrollRequest::PageDown,
        c if c == SB_TOP.0 => ScrollRequest::Top,
        c if c == SB_BOTTOM.0 => ScrollRequest::Bottom,
        c if c == SB_THUMBTRACK.0 => {
            ScrollRequest::ThumbTrack(((wparam >> 16) & 0xffff) as i16 as i32)
        }
        c if c == SB_THUMBPOSITION.0 => {
            ScrollRequest::ThumbPosition(((wparam >> 16) & 0xffff) as i16 as i32)
        }
        c if c == SB_ENDSCROLL.0 => ScrollRequest::EndScroll,
        _ => return None,
    })
}

/// Sets the vertical scrollbar's range, page size and position in one call.
///
/// `content` is the full scrollable extent and `page` the visible extent, both
/// in device pixels; `pos` is clamped into `0..=content - page`. The bar is
/// kept visible even when the content fits (`SIF_DISABLENOSCROLL`), matching
/// the themed look.
pub(crate) fn set_vertical_info(hwnd: Hwnd, content: i32, page: i32, pos: i32) {
    let content = content.max(0);
    let page = page.max(0);
    let info = SCROLLINFO {
        cbSize: size_of::<SCROLLINFO>() as u32,
        fMask: SIF_RANGE | SIF_PAGE | SIF_POS | SIF_DISABLENOSCROLL,
        nMin: 0,
        nMax: content.saturating_sub(1).max(0),
        nPage: page.min(content).max(0) as u32,
        nPos: pos.max(0),
        nTrackPos: 0,
    };
    // SAFETY: `info` is fully initialised and `SB_VERT` selects the vertical
    // standard scrollbar; only integer values are read.
    unsafe {
        let _ = SetScrollInfo(raw_hwnd(hwnd), SB_VERT, &info, true);
    }
}

/// Moves the vertical scrollbar thumb to `pos` without changing the range.
pub(crate) fn set_vertical_pos(hwnd: Hwnd, pos: i32) {
    // SAFETY: `SB_VERT` selects the vertical standard scrollbar; only integers
    // are passed.
    unsafe {
        let _ = SetScrollPos(raw_hwnd(hwnd), SB_VERT, pos.max(0), true);
    }
}

/// Reads the live thumb position carried by a `WM_VSCROLL`/`SIF_TRACKPOS`.
pub(crate) fn track_position(hwnd: Hwnd) -> i32 {
    let mut info = SCROLLINFO {
        cbSize: size_of::<SCROLLINFO>() as u32,
        fMask: SIF_TRACKPOS,
        ..Default::default()
    };
    // SAFETY: `info` is a valid out-parameter; the call only writes it.
    if unsafe { GetScrollInfo(raw_hwnd(hwnd), SB_VERT, &mut info) }.is_ok() {
        info.nTrackPos
    } else {
        0
    }
}

/// Adds the `WS_VSCROLL` style so `hwnd` shows a standard vertical scrollbar,
/// then asks Windows to recalculate the non-client frame. Used by the custom
/// widget scroll host, which is created before the app decides whether it
/// scrolls.
pub(crate) fn enable_vertical(hwnd: Hwnd) {
    // SAFETY: reads and writes the window's style bits and triggers a frame
    // recalculation; a stale handle makes the calls fail harmlessly.
    unsafe {
        let style = GetWindowLongPtrW(raw_hwnd(hwnd), GWL_STYLE);
        let _ = SetWindowLongPtrW(raw_hwnd(hwnd), GWL_STYLE, style | WS_VSCROLL.0 as isize);
        let _ = SetWindowPos(
            raw_hwnd(hwnd),
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED,
        );
    }
}

/// Reads the vertical scrollbar's current `(nMin, nMax, nPage, nPos)`, for
/// tests and diagnostics.
#[cfg(test)]
pub(crate) fn vertical_info(hwnd: Hwnd) -> (i32, i32, u32, i32) {
    let mut info = SCROLLINFO {
        cbSize: size_of::<SCROLLINFO>() as u32,
        fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
        ..Default::default()
    };
    // SAFETY: `info` is a valid out-parameter; the call only writes it.
    if unsafe { GetScrollInfo(raw_hwnd(hwnd), SB_VERT, &mut info) }.is_ok() {
        (info.nMin, info.nMax, info.nPage, info.nPos)
    } else {
        (0, 0, 0, 0)
    }
}

/// A callback invoked with a child window's raw wheel delta.
type WheelHandler = Box<dyn Fn(i16)>;

struct WheelRefdata {
    handler: WheelHandler,
}

/// Forwards `WM_MOUSEWHEEL` from a child window to its scroll container.
///
/// The child's own window procedure never sees the message while this subclass
/// is installed; dropping it removes the subclass and frees the callback.
pub(crate) struct WheelForwarder {
    child: Hwnd,
    raw: *mut WheelRefdata,
}

impl WheelForwarder {
    /// Installs wheel forwarding on `child`. Returns `None` if subclassing
    /// fails.
    pub(crate) fn install(child: Hwnd, handler: WheelHandler) -> Option<WheelForwarder> {
        let raw = Box::into_raw(Box::new(WheelRefdata { handler }));
        if !super::window::set_subclass(child, Some(wheel_proc), WHEEL_SUBCLASS_ID, raw as usize) {
            // SAFETY: install failed before the subclass could adopt it.
            unsafe { drop(Box::from_raw(raw)) };
            return None;
        }
        Some(WheelForwarder { child, raw })
    }
}

impl Drop for WheelForwarder {
    fn drop(&mut self) {
        super::window::remove_subclass(self.child, Some(wheel_proc), WHEEL_SUBCLASS_ID);
        // SAFETY: allocated in `install` and reclaimed exactly once.
        unsafe { drop(Box::from_raw(self.raw)) };
    }
}

const WHEEL_SUBCLASS_ID: usize = 0x7777_686c; // "wwhl"

/// The subclass procedure that swallows a child's wheel message.
///
/// # Safety
/// Called by Windows for a subclass installed by [`WheelForwarder::install`];
/// `refdata` is the `WheelRefdata` pointer.
unsafe extern "system" fn wheel_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    refdata: usize,
) -> LRESULT {
    if msg == WM_MOUSEWHEEL {
        // SAFETY: `refdata` is the live `WheelRefdata` installed by `install`;
        // the wheel delta is the signed high word of `wparam`.
        unsafe {
            let data = &*(refdata as *const WheelRefdata);
            let delta = ((wparam.0 >> 16) & 0xffff) as u16 as i16;
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                (data.handler)(delta);
            }));
        }
        // Consume the wheel so the child never scrolls itself.
        return LRESULT(0);
    }
    // SAFETY: forward to the subclass chain's original window procedure.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

/// Scrolls `hwnd`'s client area and every child window in it by `dy` pixels,
/// then paints the uncovered strip at once.
///
/// The already-drawn pixels are blitted rather than repainted, so a scroll
/// step only redraws the band it exposes instead of erasing and repainting
/// the whole content (which flashes the background through it).
pub(crate) fn scroll_children(hwnd: Hwnd, dy: i32) {
    if dy == 0 {
        return;
    }
    // SAFETY: `hwnd` is a live window owned by the caller; null scroll/clip
    // rectangles mean the whole client area, no update region or rectangle is
    // requested, and only documented flags are passed.
    unsafe {
        let _ = ScrollWindowEx(
            raw_hwnd(hwnd),
            0,
            dy,
            None,
            None,
            None,
            None,
            SW_SCROLLCHILDREN | SW_INVALIDATE | SW_ERASE,
        );
        let _ = UpdateWindow(raw_hwnd(hwnd));
    }
}

#[cfg(test)]
mod tests {
    use super::{ScrollRequest, decode_vscroll};
    use windows::Win32::UI::WindowsAndMessaging::{
        SB_BOTTOM, SB_ENDSCROLL, SB_LINEDOWN, SB_LINEUP, SB_PAGEDOWN, SB_PAGEUP, SB_THUMBPOSITION,
        SB_THUMBTRACK, SB_TOP,
    };

    fn packed(code: i32, pos: i16) -> usize {
        (code as usize & 0xffff) | ((pos as u16 as usize) << 16)
    }

    #[test]
    fn decodes_line_and_page_requests() {
        assert_eq!(
            decode_vscroll(SB_LINEUP.0 as usize),
            Some(ScrollRequest::LineUp)
        );
        assert_eq!(
            decode_vscroll(SB_LINEDOWN.0 as usize),
            Some(ScrollRequest::LineDown)
        );
        assert_eq!(
            decode_vscroll(SB_PAGEUP.0 as usize),
            Some(ScrollRequest::PageUp)
        );
        assert_eq!(
            decode_vscroll(SB_PAGEDOWN.0 as usize),
            Some(ScrollRequest::PageDown)
        );
        assert_eq!(decode_vscroll(SB_TOP.0 as usize), Some(ScrollRequest::Top));
        assert_eq!(
            decode_vscroll(SB_BOTTOM.0 as usize),
            Some(ScrollRequest::Bottom)
        );
        assert_eq!(
            decode_vscroll(SB_ENDSCROLL.0 as usize),
            Some(ScrollRequest::EndScroll)
        );
    }

    #[test]
    fn decodes_thumb_position_from_the_high_word() {
        assert_eq!(
            decode_vscroll(packed(SB_THUMBTRACK.0, 321)),
            Some(ScrollRequest::ThumbTrack(321))
        );
        assert_eq!(
            decode_vscroll(packed(SB_THUMBPOSITION.0, -4)),
            Some(ScrollRequest::ThumbPosition(-4))
        );
    }

    #[test]
    fn rejects_an_unknown_code() {
        assert_eq!(decode_vscroll(0x1234_9999), None);
    }
}
