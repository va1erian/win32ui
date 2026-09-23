//! Raw tooltip helpers: the shared `tooltips_class32` window, the tools added
//! to it, and the custom draw that darkens it.
//!
//! Only documented APIs are used. `SetWindowTheme(hwnd, "DarkMode_Explorer",
//! null)` does not by itself darken a tooltip (that needs the undocumented
//! `AllowDarkModeForWindow` ordinal, which this crate deliberately avoids), and
//! `TTM_SETTIPBKCOLOR`/`TTM_SETTIPTEXTCOLOR` are ignored while visual styles
//! are on. So a dark tooltip is owner-drawn through the documented
//! `NM_CUSTOMDRAW` notification, the same escape hatch the list view uses.

use core::ptr;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{HDC, HFONT};
use windows::Win32::UI::Controls::{
    CDRF_DODEFAULT, CDRF_NOTIFYPOSTPAINT, CDRF_SKIPDEFAULT, NMTTCUSTOMDRAW, SetWindowTheme,
    TOOLTIP_FLAGS, TOOLTIPS_CLASS, TTF_IDISHWND, TTF_SUBCLASS, TTM_ADDTOOLW, TTM_DELTOOLW,
    TTM_NEWTOOLRECTW, TTM_SETMAXTIPWIDTH, TTM_SETTIPBKCOLOR, TTM_SETTIPTEXTCOLOR,
    TTM_UPDATETIPTEXTW, TTS_ALWAYSTIP, TTS_NOPREFIX, TTTOOLINFOW,
};
use windows::Win32::UI::Shell::DefSubclassProc;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, GA_ROOT, GetAncestor, WINDOW_STYLE, WM_DPICHANGED, WM_SETFONT, WS_EX_TOPMOST,
    WS_POPUP,
};
use windows::core::{PCWSTR, PWSTR};

use crate::color::Color;
use crate::error::Result;
use crate::geometry::{Point, Rect};
use crate::hwnd::Hwnd;

use super::{hwnd_from, raw_hwnd, win32_error};

/// Creates the shared tooltip window owned by `owner`.
///
/// The window is a popup with `TTS_ALWAYSTIP | TTS_NOPREFIX`, as the issue
/// specifies: it shows even while the owner is inactive, and `&` in the text is
/// literal rather than an accelerator marker.
pub(crate) fn create(owner: Hwnd) -> Result<Hwnd> {
    let style = WS_POPUP.0 | TTS_ALWAYSTIP | TTS_NOPREFIX;
    // SAFETY: `TOOLTIPS_CLASS` is a system class, for which `CreateWindowExW`
    // ignores the instance; the owner is a live window and the null title is
    // allowed. No create-params are used.
    let created = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST,
            TOOLTIPS_CLASS,
            PCWSTR::null(),
            WINDOW_STYLE(style),
            0,
            0,
            0,
            0,
            Some(raw_hwnd(owner)),
            None,
            None,
            None,
        )
    }
    .map_err(win32_error)?;
    Ok(hwnd_from(created))
}

/// Adds a whole-widget tooltip: `TTF_IDISHWND | TTF_SUBCLASS`, so the tooltip
/// control subclasses the widget and shows over its whole client area.
///
/// With `TTF_IDISHWND` the tool is identified by `uId` (the widget handle) and
/// `hwnd` is the window that contains it, so no rectangle has to be tracked.
/// Returns whether the tooltip control accepted the tool.
pub(crate) fn add_control_tool(tooltip: Hwnd, owner: Hwnd, text: &[u16]) -> bool {
    let mut info = tool_info(
        parent_window(owner),
        owner.raw(),
        TTF_IDISHWND.0 | TTF_SUBCLASS.0,
        None,
        text.as_ptr() as *mut u16,
    );
    // SAFETY: `info` is fully initialised and `text` outlives the call; the
    // tooltip control copies the text.
    super::window::send_message(
        tooltip,
        TTM_ADDTOOLW,
        0,
        &mut info as *mut TTTOOLINFOW as isize,
    ) != 0
}

/// Adds a region tooltip: `TTF_SUBCLASS` with an explicit client rectangle, so
/// the tooltip control shows it only while the pointer is inside `rect`.
/// Returns whether the tooltip control accepted the tool.
pub(crate) fn add_region_tool(
    tooltip: Hwnd,
    owner: Hwnd,
    slot: usize,
    rect: Rect,
    text: &[u16],
) -> bool {
    let mut info = tool_info(
        owner,
        slot,
        TTF_SUBCLASS.0,
        Some(rect),
        text.as_ptr() as *mut u16,
    );
    // SAFETY: `info` is fully initialised and `text` outlives the call; the
    // tooltip control copies the text.
    super::window::send_message(
        tooltip,
        TTM_ADDTOOLW,
        0,
        &mut info as *mut TTTOOLINFOW as isize,
    ) != 0
}

/// Replaces the text of an existing tool.
pub(crate) fn update_tool_text(
    tooltip: Hwnd,
    owner: Hwnd,
    slot: usize,
    id_is_hwnd: bool,
    text: &[u16],
) {
    let (hwnd, id, flags) = identify(owner, slot, id_is_hwnd);
    let mut info = tool_info(hwnd, id, flags, None, text.as_ptr() as *mut u16);
    // SAFETY: `info` is fully initialised and `text` outlives the call.
    super::window::send_message(
        tooltip,
        TTM_UPDATETIPTEXTW,
        0,
        &mut info as *mut TTTOOLINFOW as isize,
    );
}

/// Moves an existing region tool's rectangle.
pub(crate) fn update_tool_rect(tooltip: Hwnd, owner: Hwnd, slot: usize, rect: Rect) {
    let mut info = tool_info(owner, slot, 0, Some(rect), ptr::null_mut());
    // SAFETY: `info` is fully initialised; the rectangle is plain geometry.
    super::window::send_message(
        tooltip,
        TTM_NEWTOOLRECTW,
        0,
        &mut info as *mut TTTOOLINFOW as isize,
    );
}

/// Removes a tool from the shared tooltip.
pub(crate) fn remove_tool(tooltip: Hwnd, owner: Hwnd, slot: usize, id_is_hwnd: bool) {
    let (hwnd, id, flags) = identify(owner, slot, id_is_hwnd);
    let mut info = tool_info(hwnd, id, flags, None, ptr::null_mut());
    // SAFETY: `info` identifies a tool the tooltip control owns; removing an
    // unknown tool is a documented no-op failure.
    super::window::send_message(
        tooltip,
        TTM_DELTOOLW,
        0,
        &mut info as *mut TTTOOLINFOW as isize,
    );
}

/// The `(hwnd, uId, flags)` that identify a tool to the tooltip control.
fn identify(owner: Hwnd, slot: usize, id_is_hwnd: bool) -> (Hwnd, usize, u32) {
    if id_is_hwnd {
        (parent_window(owner), owner.raw(), TTF_IDISHWND.0)
    } else {
        (owner, slot, 0)
    }
}

/// The text the tooltip control currently holds for a tool, for tests.
#[cfg(test)]
pub(crate) fn tool_text(
    tooltip: Hwnd,
    owner: Hwnd,
    slot: usize,
    id_is_hwnd: bool,
) -> Option<String> {
    let (hwnd, id, flags) = identify(owner, slot, id_is_hwnd);
    let mut buffer = vec![0u16; 512];
    let mut info = tool_info(hwnd, id, flags, None, buffer.as_mut_ptr());
    // SAFETY: `info` is fully initialised and `buffer` is a writable buffer the
    // control copies the text into.
    let ok = super::window::send_message(
        tooltip,
        windows::Win32::UI::Controls::TTM_GETTOOLINFOW,
        0,
        &mut info as *mut TTTOOLINFOW as isize,
    );
    if ok == 0 {
        return None;
    }
    let len = buffer.iter().position(|&unit| unit == 0).unwrap_or(0);
    Some(String::from_utf16_lossy(&buffer[..len]))
}

/// The parent of `hwnd`, or the null handle when it is gone.
fn parent_window(hwnd: Hwnd) -> Hwnd {
    // SAFETY: `GetParent` only walks the parent chain; a stale handle or a
    // top-level window yields an error/null, reported as `Hwnd::NULL`.
    let parent = unsafe { windows::Win32::UI::WindowsAndMessaging::GetParent(raw_hwnd(hwnd)) };
    parent.map(hwnd_from).unwrap_or(Hwnd::NULL)
}

/// Opts the tooltip into its light/dark visual style and records the tip
/// colours (which apply when visual styles are off; the dark owner-draw covers
/// the rest).
pub(crate) fn set_theme(tooltip: Hwnd, is_dark: bool, background: Color, text: Color) {
    let name = if is_dark {
        windows::core::w!("DarkMode_Explorer")
    } else {
        windows::core::w!("Explorer")
    };
    // SAFETY: `tooltip` is live; the theme names are the documented literals.
    unsafe {
        let _ = SetWindowTheme(raw_hwnd(tooltip), name, PCWSTR::null());
    }
    super::window::send_message(
        tooltip,
        TTM_SETTIPBKCOLOR,
        background.to_colorref() as usize,
        0,
    );
    super::window::send_message(tooltip, TTM_SETTIPTEXTCOLOR, text.to_colorref() as usize, 0);
}

/// Gives the tooltip the font comctl32 sizes it with and the paint draws with,
/// the margin the text is inset by, and the width past which a long tip wraps.
///
/// comctl32 autosizes the tooltip from `WM_GETFONT`, so setting the same font
/// the owner-draw paints with makes the two agree at any DPI; the margin is
/// added to that size, so the text rectangle `window - margin` always holds the
/// text. Re-sent on a DPI change (see [`subclass_dpi`]) because comctl32 resizes
/// the window but keeps a font that no longer matches.
pub(crate) fn apply_font_metrics(tooltip: Hwnd, font: HFONT, max_width: i32) {
    super::window::send_message(tooltip, WM_SETFONT, font.0 as usize, 1);
    super::window::send_message(tooltip, TTM_SETMAXTIPWIDTH, 0, max_width as isize);
}

/// The subclass id for [`dpi_proc`].
const DPI_SUBCLASS_ID: usize = 0x7774_7064; // "wtpd"

/// Makes the tooltip re-apply its font when it moves to a monitor with another
/// DPI. The tooltip is a top-level window, so it receives `WM_DPICHANGED`; the
/// subclass lets the widget layer recreate the font at the new DPI, which
/// comctl32 does not do for a font it did not create.
pub(crate) fn subclass_dpi(tooltip: Hwnd) {
    super::window::set_subclass(tooltip, Some(dpi_proc), DPI_SUBCLASS_ID, 0);
}

/// Forwards a tooltip's `WM_DPICHANGED` to the widget layer, then chains to the
/// class procedure.
///
/// # Safety
/// Called by Windows for the subclass installed by [`subclass_dpi`]; `hwnd` is
/// the live tooltip window.
unsafe extern "system" fn dpi_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _refdata: usize,
) -> LRESULT {
    if msg == WM_DPICHANGED {
        crate::controls::tooltip::reapply_dpi(hwnd_from(hwnd));
    }
    // SAFETY: `DefSubclassProc` chains to the procedure the subclass wrapped,
    // which is the documented contract for a subclass installed on a window.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

/// The top-level window `hwnd` belongs to, or the null handle when it is gone.
pub(crate) fn root_window(hwnd: Hwnd) -> Hwnd {
    // SAFETY: `GetAncestor` only walks the parent chain of a handle; a stale
    // handle returns null, which `hwnd_from` turns into `Hwnd::NULL`.
    let root = unsafe { GetAncestor(raw_hwnd(hwnd), GA_ROOT) };
    hwnd_from(root)
}

/// The cursor position, in screen coordinates.
pub(crate) fn cursor_position() -> Point {
    let mut point = windows::Win32::Foundation::POINT::default();
    // SAFETY: `point` is a valid out-pointer.
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut point);
    }
    Point::new(point.x, point.y)
}

/// The screen position of `hwnd`'s client origin.
pub(crate) fn client_origin(hwnd: Hwnd) -> Point {
    let mut point = windows::Win32::Foundation::POINT::default();
    // SAFETY: `point` is a valid out-pointer and `hwnd` is live for the caller.
    unsafe {
        let _ = windows::Win32::Graphics::Gdi::ClientToScreen(raw_hwnd(hwnd), &mut point);
    }
    Point::new(point.x, point.y)
}

/// The owner-draw context of a tooltip `NM_CUSTOMDRAW` notification.
pub(crate) struct TooltipDraw {
    /// `CDDS_*` stage.
    pub stage: u32,
    /// The DC to draw into.
    pub hdc: HDC,
    /// The rectangle being drawn (the whole tooltip for the global stages).
    pub rect: Rect,
}

/// What the tooltip should do after the custom-draw callback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TooltipDrawResult {
    /// Let the tooltip draw normally (`CDRF_DODEFAULT`).
    Default,
    /// Ask for a post-paint notification (`CDRF_NOTIFYPOSTPAINT`).
    NotifyPostPaint,
    /// The callback painted the whole tooltip (`CDRF_SKIPDEFAULT`).
    SkipDefault,
}

/// Runs `draw` for a tooltip `NM_CUSTOMDRAW` and returns the `CDRF_*` code the
/// control expects.
pub(crate) fn custom_draw(
    lparam: isize,
    draw: impl FnOnce(&TooltipDraw) -> TooltipDrawResult,
) -> isize {
    if lparam == 0 {
        return CDRF_DODEFAULT as isize;
    }
    // SAFETY: called for an NM_CUSTOMDRAW from one of our tooltips, so lparam
    // points at a valid NMTTCUSTOMDRAW.
    let info = unsafe { &*(lparam as *const NMTTCUSTOMDRAW) };
    let context = TooltipDraw {
        stage: info.nmcd.dwDrawStage.0,
        hdc: info.nmcd.hdc,
        rect: Rect::new(
            info.nmcd.rc.left,
            info.nmcd.rc.top,
            info.nmcd.rc.right,
            info.nmcd.rc.bottom,
        ),
    };
    match draw(&context) {
        TooltipDrawResult::Default => CDRF_DODEFAULT as isize,
        TooltipDrawResult::NotifyPostPaint => CDRF_NOTIFYPOSTPAINT as isize,
        TooltipDrawResult::SkipDefault => CDRF_SKIPDEFAULT as isize,
    }
}

/// The `cbSize` a `TOOLINFO` must carry.
///
/// `commctrl.h` defines `TTTOOLINFOW_V2_SIZE` as `CCSIZEOF_STRUCT(TOOLINFOW,
/// lParam)`; it excludes the `lpReserved` tail member that
/// `TTTOOLINFOW_V3_SIZE` (the full `sizeof`) includes. Older common-control
/// versions reject the V3 size, and every version accepts V2, so V2 is what we
/// send. `lParam` is the last member before `lpReserved`.
const fn tool_info_size() -> u32 {
    (core::mem::offset_of!(TTTOOLINFOW, lParam) + size_of::<isize>()) as u32
}

/// Builds the `TOOLINFO` that identifies a tool. `lpszText` is a raw pointer
/// into the caller's UTF-16 buffer, which must outlive the message.
fn tool_info(hwnd: Hwnd, id: usize, flags: u32, rect: Option<Rect>, text: *mut u16) -> TTTOOLINFOW {
    TTTOOLINFOW {
        cbSize: tool_info_size(),
        uFlags: TOOLTIP_FLAGS(flags),
        hwnd: raw_hwnd(hwnd),
        uId: id,
        rect: rect.map_or_else(RECT::default, |rect| RECT {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        }),
        lpszText: PWSTR(text),
        ..Default::default()
    }
}

#[cfg(test)]
pub(crate) mod tests;
