//! Raw Win32 menu construction, tracking and per-item state.

use core::ffi::c_void;

use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Gdi::HBRUSH;
use windows::Win32::UI::HiDpi::GetSystemMetricsForDpi;
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CheckMenuItem, CreateMenu, CreatePopupMenu, DestroyMenu, DrawMenuBar,
    GetCursorPos, HMENU, MENU_ITEM_FLAGS, MENUINFO, MF_BYCOMMAND, MF_CHECKED, MF_GRAYED,
    MF_OWNERDRAW, MF_POPUP, MF_SEPARATOR, MF_STRING, MFT_RADIOCHECK, MIM_BACKGROUND, SM_CYMENU,
    SetMenu, SetMenuInfo, TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, TPM_TOPALIGN,
    TrackPopupMenuEx,
};
use windows::core::PCWSTR;

use crate::geometry::Point;
use crate::hwnd::Hwnd;
use crate::units::dip;

use super::raw_hwnd;

/// The native menu bar item height and per-side horizontal padding, in device
/// pixels, at `dpi`.
///
/// Owner-drawn bar items report their own size (`WM_MEASUREITEM`). The height
/// is the documented `SM_CYMENU` metric (`WinUser.h` via the `windows` crate).
/// The native bar also adds its own padding around a bar item, so the measured
/// width only needs a small symmetric pad: 4 design units per side matches the
/// item spacing of the native light bar (using the font height and a larger
/// pad made the dark bar far taller and wider than light; #67).
pub(crate) fn bar_item_metrics(dpi: u32) -> (i32, i32) {
    // SAFETY: `GetSystemMetricsForDpi` takes a metric index and a DPI value; no
    // pointers.
    let height = unsafe { GetSystemMetricsForDpi(SM_CYMENU, dpi) };
    (height.max(1), dip(4.0).to_px(dpi).value().max(1))
}

/// Creates an empty menu bar.
pub(crate) fn create_bar() -> isize {
    // SAFETY: takes no arguments.
    unsafe { CreateMenu() }
        .map(|menu| menu.0 as isize)
        .unwrap_or(0)
}

/// Creates an empty popup (drop-down) menu.
pub(crate) fn create_popup() -> isize {
    // SAFETY: takes no arguments.
    unsafe { CreatePopupMenu() }
        .map(|menu| menu.0 as isize)
        .unwrap_or(0)
}

/// Destroys a menu and everything it owns. A null/zero handle is ignored.
pub(crate) fn destroy(menu: isize) {
    if menu == 0 {
        return;
    }
    // SAFETY: `menu` came from `CreateMenu`/`CreatePopupMenu` and is not
    // referenced by a window after this call.
    unsafe {
        let _ = DestroyMenu(hm(menu));
    }
}

/// Appends a plain (native) text item carrying `command`.
pub(crate) fn append_native(menu: isize, command: u16, text: &str, enabled: bool) {
    let wide = wide(text);
    let mut bits = MF_STRING.0;
    if !enabled {
        bits |= MF_GRAYED.0;
    }
    append(menu, bits, command as usize, PCWSTR(wide.as_ptr()));
}

/// Appends a native separator.
pub(crate) fn append_separator_native(menu: isize) {
    append(menu, MF_SEPARATOR.0, 0, PCWSTR::null());
}

/// Appends a native item that opens `child`.
pub(crate) fn append_popup_native(menu: isize, child: isize, text: &str) {
    let wide = wide(text);
    append(
        menu,
        MF_POPUP.0 | MF_STRING.0,
        child as usize,
        PCWSTR(wide.as_ptr()),
    );
}

/// Appends an owner-drawn item carrying `command`; `data` is win32ui's own
/// render id, returned in `DRAWITEMSTRUCT.itemData`/`MEASUREITEMSTRUCT.itemData`.
pub(crate) fn append_owned(menu: isize, command: u16, data: usize, enabled: bool) {
    let mut bits = MF_OWNERDRAW.0;
    if !enabled {
        bits |= MF_GRAYED.0;
    }
    append(menu, bits, command as usize, data_ptr(data));
}

/// Appends an owner-drawn separator.
pub(crate) fn append_owned_separator(menu: isize, data: usize) {
    append(menu, MF_SEPARATOR.0 | MF_OWNERDRAW.0, 0, data_ptr(data));
}

/// Appends an owner-drawn item that opens `child`.
pub(crate) fn append_owned_popup(menu: isize, child: isize, data: usize) {
    append(
        menu,
        MF_POPUP.0 | MF_OWNERDRAW.0,
        child as usize,
        data_ptr(data),
    );
}

fn append(menu: isize, bits: u32, item: usize, new_item: PCWSTR) {
    // SAFETY: `menu` is a live menu and `new_item` either a nul-terminated wide
    // string alive for the call or an application-defined data value.
    unsafe {
        let _ = AppendMenuW(hm(menu), MENU_ITEM_FLAGS(bits), item, new_item);
    }
}

/// Checks or unchecks an item by command id. `radio` asks for a radio-style
/// check mark (a dot) instead of a tick.
pub(crate) fn check_item(menu: isize, command: u16, checked: bool, radio: bool) {
    let mut bits = MF_BYCOMMAND.0;
    if checked {
        bits |= MF_CHECKED.0;
    }
    if radio {
        bits |= MFT_RADIOCHECK.0;
    }
    // SAFETY: `menu` is live; the flags/ids are plain values.
    unsafe {
        let _ = CheckMenuItem(hm(menu), command as u32, bits);
    }
}

/// Sets the menu's background brush (documented `SetMenuInfo`), so the gaps
/// between owner-drawn items are themed too.
///
/// The one-pixel highlight the system draws along the bottom of a menu bar is
/// *not* covered by `MIM_BACKGROUND`: it is part of the system menu frame, so
/// [`menu_seam`](super::menu_seam) paints over it after each non-client paint.
pub(crate) fn set_background(menu: isize, brush: HBRUSH) {
    let info = MENUINFO {
        cbSize: size_of::<MENUINFO>() as u32,
        fMask: MIM_BACKGROUND,
        hbrBack: brush,
        ..Default::default()
    };
    // SAFETY: `menu` is live, `info` is fully initialised, and the brush is
    // kept alive by the menu's owner for as long as the menu uses it.
    unsafe {
        let _ = SetMenuInfo(hm(menu), &info);
    }
}

/// Installs `menu` as `hwnd`'s menu bar and asks for it to be redrawn.
pub(crate) fn set_bar(hwnd: Hwnd, menu: isize) {
    // SAFETY: both handles are live; the window borrows the menu, which its
    // owner keeps alive.
    unsafe {
        let _ = SetMenu(raw_hwnd(hwnd), Some(hm(menu)));
        let _ = DrawMenuBar(raw_hwnd(hwnd));
    }
}

/// Shows `menu` as a popup at the screen position `at` and returns the chosen
/// command id, if any (`TPM_RETURNCMD`, so no `WM_COMMAND` is posted).
pub(crate) fn track_popup(menu: isize, owner: Hwnd, at: Point) -> Option<u16> {
    let flags = TPM_RETURNCMD.0 | TPM_LEFTALIGN.0 | TPM_TOPALIGN.0 | TPM_RIGHTBUTTON.0;
    // SAFETY: `menu` and `owner` are live and the coordinates are values; the
    // call pumps messages until the menu closes.
    let result = unsafe { TrackPopupMenuEx(hm(menu), flags, at.x, at.y, raw_hwnd(owner), None) };
    let command = result.0;
    if command == 0 {
        None
    } else {
        Some((command & 0xffff) as u16)
    }
}

/// The cursor position in screen coordinates.
pub(crate) fn cursor_position() -> Point {
    let mut point = POINT::default();
    // SAFETY: `point` is a valid out-pointer.
    unsafe {
        let _ = GetCursorPos(&mut point);
    }
    Point::new(point.x, point.y)
}

/// Converts a raw menu handle value into the `windows` newtype.
fn hm(menu: isize) -> HMENU {
    HMENU(menu as *mut c_void)
}

/// Converts an application data value into the pointer slot `AppendMenuW` takes
/// for an owner-drawn item (the value is never dereferenced).
fn data_ptr(data: usize) -> PCWSTR {
    PCWSTR(data as *const u16)
}

/// A nul-terminated UTF-16 copy of `text`.
fn wide(text: &str) -> Vec<u16> {
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    wide.push(0);
    wide
}
