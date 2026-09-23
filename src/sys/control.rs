//! Common-control initialisation and the raw control-specific messages.

use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::HFONT;
use windows::Win32::UI::Controls::{
    HTREEITEM, ICC_BAR_CLASSES, ICC_LISTVIEW_CLASSES, ICC_STANDARD_CLASSES, ICC_TREEVIEW_CLASSES,
    INITCOMMONCONTROLSEX, INITCOMMONCONTROLSEX_ICC, InitCommonControlsEx, NMTREEVIEWW, TVGN_CARET,
    TVIF_CHILDREN, TVIF_HANDLE, TVIF_PARAM, TVIF_TEXT, TVINSERTSTRUCTW, TVINSERTSTRUCTW_0,
    TVITEMEXW_CHILDREN, TVITEMW, TVM_GETCOUNT, TVM_GETITEMW, TVM_GETNEXTITEM, TVM_INSERTITEMW,
    TVM_SETBKCOLOR, TVM_SETEXTENDEDSTYLE, TVM_SETITEMHEIGHT, TVM_SETTEXTCOLOR,
};
use windows::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_SETFONT};
use windows::core::PWSTR;

use crate::color::Color;
use crate::error::{Error, Result};
use crate::hwnd::Hwnd;

use super::raw_hwnd;

/// Registers the common-control classes (ListView, TreeView, status bar…).
pub(crate) fn init_common_controls() -> Result<()> {
    let flags = ICC_LISTVIEW_CLASSES.0
        | ICC_TREEVIEW_CLASSES.0
        | ICC_BAR_CLASSES.0
        | ICC_STANDARD_CLASSES.0;
    let classes = INITCOMMONCONTROLSEX {
        dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: INITCOMMONCONTROLSEX_ICC(flags),
    };
    // SAFETY: `classes` is fully initialised for the call.
    unsafe { InitCommonControlsEx(&classes) }
        .ok()
        .map_err(|_| Error::ControlsUnavailable)
}

pub(crate) fn send(hwnd: Hwnd, msg: u32, wparam: usize, lparam: isize) -> isize {
    // SAFETY: only integer values are forwarded; the caller guarantees any
    // pointer in `lparam` points at a valid struct for the duration.
    unsafe {
        SendMessageW(
            raw_hwnd(hwnd),
            msg,
            Some(WPARAM(wparam)),
            Some(LPARAM(lparam)),
        )
        .0
    }
}

pub(crate) fn write_wide(destination: *mut u16, capacity: i32, value: &str) {
    if destination.is_null() || capacity <= 0 {
        return;
    }
    let wide: Vec<u16> = value.encode_utf16().collect();
    let capacity = capacity as usize;
    let count = wide.len().min(capacity - 1);
    // SAFETY: the destination points at `capacity` writable u16s owned by the
    // sender; `count + 1 <= capacity` and the source is a live slice.
    unsafe {
        std::ptr::copy_nonoverlapping(wide.as_ptr(), destination, count);
        *destination.add(count) = 0;
    }
}

/// Gives a control a font (and asks it to repaint).
pub(crate) fn set_control_font(hwnd: Hwnd, font: HFONT) {
    send(hwnd, WM_SETFONT, font.0 as usize, 1);
}

// ---------------------------------------------------------------------------
// TreeView
// ---------------------------------------------------------------------------

/// Inserts a tree item, returning its handle value.
pub(crate) fn tv_insert(
    hwnd: Hwnd,
    parent: isize,
    after: isize,
    text: &str,
    data: i64,
    has_children: bool,
) -> isize {
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    wide.push(0);
    let mut item = TVINSERTSTRUCTW {
        hParent: HTREEITEM(parent),
        hInsertAfter: HTREEITEM(after),
        Anonymous: TVINSERTSTRUCTW_0 {
            item: TVITEMW {
                mask: TVIF_TEXT | TVIF_PARAM | TVIF_CHILDREN,
                pszText: PWSTR(wide.as_mut_ptr()),
                cchTextMax: wide.len() as i32,
                lParam: LPARAM(data as isize),
                cChildren: TVITEMEXW_CHILDREN(i32::from(has_children)),
                ..Default::default()
            },
        },
    };
    send(
        hwnd,
        TVM_INSERTITEMW,
        0,
        &mut item as *mut TVINSERTSTRUCTW as isize,
    )
}

/// Total number of inserted tree items.
pub(crate) fn tv_count(hwnd: Hwnd) -> i32 {
    send(hwnd, TVM_GETCOUNT, 0, 0) as i32
}

/// Enables tree-view extended styles (e.g. `TVS_EX_DOUBLEBUFFER`).
pub(crate) fn tv_set_extended_style(hwnd: Hwnd, style: u32) {
    send(hwnd, TVM_SETEXTENDEDSTYLE, style as usize, style as isize);
}

/// Sets the height of every tree item.
pub(crate) fn tv_set_item_height(hwnd: Hwnd, height: i32) {
    send(hwnd, TVM_SETITEMHEIGHT, height.max(0) as usize, 0);
}

/// Sets the tree view's background and text colours.
pub(crate) fn tv_set_colors(hwnd: Hwnd, background: Color, text: Color) {
    send(hwnd, TVM_SETBKCOLOR, 0, background.to_colorref() as isize);
    send(hwnd, TVM_SETTEXTCOLOR, 0, text.to_colorref() as isize);
}

/// The data (`lParam`) of the currently selected item.
pub(crate) fn tv_selected(hwnd: Hwnd) -> Option<i64> {
    let handle = send(hwnd, TVM_GETNEXTITEM, TVGN_CARET as usize, 0);
    if handle == 0 {
        return None;
    }
    let mut item = TVITEMW {
        mask: TVIF_PARAM | TVIF_HANDLE,
        hItem: HTREEITEM(handle),
        ..Default::default()
    };
    let ok = send(hwnd, TVM_GETITEMW, 0, &mut item as *mut TVITEMW as isize);
    if ok == 0 || item.lParam.0 == 0 {
        None
    } else {
        Some(item.lParam.0 as i64)
    }
}

/// Reads the item being expanded from a `TVN_ITEMEXPANDING` notification.
pub(crate) fn tv_expanding(lparam: isize) -> Option<(isize, i64)> {
    if lparam == 0 {
        return None;
    }
    // SAFETY: called for a TVN_ITEMEXPANDING from one of our tree views.
    let info = unsafe { &*(lparam as *const NMTREEVIEWW) };
    Some((info.itemNew.hItem.0, info.itemNew.lParam.0 as i64))
}
