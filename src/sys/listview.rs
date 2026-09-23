//! Raw list-view (`SysListView32`) messages: owner-data requests, custom draw,
//! columns, selection state and targeted updates.

use windows::Win32::UI::Controls::{
    LIST_VIEW_ITEM_STATE_FLAGS, LVCF_FMT, LVCF_SUBITEM, LVCF_TEXT, LVCF_WIDTH, LVCFMT_LEFT,
    LVCFMT_RIGHT, LVCOLUMNW, LVIF_TEXT, LVIS_FOCUSED, LVIS_SELECTED, LVITEMW, LVM_ENSUREVISIBLE,
    LVM_GETBKCOLOR, LVM_GETCOLUMNWIDTH, LVM_GETHEADER, LVM_GETITEMRECT, LVM_GETITEMSTATE,
    LVM_GETITEMTEXTW, LVM_GETNEXTITEM, LVM_GETSUBITEMRECT, LVM_INSERTCOLUMNW, LVM_REDRAWITEMS,
    LVM_SETBKCOLOR, LVM_SETCOLUMNWIDTH, LVM_SETEXTENDEDLISTVIEWSTYLE, LVM_SETITEMCOUNT,
    LVM_SETITEMSTATE, LVM_SETTEXTBKCOLOR, LVM_SETTEXTCOLOR, LVNI_FOCUSED, LVNI_SELECTED,
    LVS_SINGLESEL, LVSICF_NOSCROLL, NMLVCUSTOMDRAW, NMLVDISPINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::{GWL_STYLE, GetWindowLongW, SetWindowLongW};
use windows::core::PWSTR;

use crate::color::Color;
use crate::hwnd::Hwnd;

use super::control::{send, write_wide};
use super::{hwnd_from, raw_hwnd};

/// Answers an `LVN_GETDISPINFO` request with text borrowed from the row.
///
/// `text(item, sub_item)` returns the cell's text, or `None` for an empty
/// cell; the borrow only has to live for the call, so accessors can return
/// `&str` straight out of the model without allocating per cell.
pub(crate) fn lv_disp_info_str<'a>(
    lparam: isize,
    mut text: impl FnMut(i32, i32) -> Option<&'a str>,
) -> isize {
    if lparam == 0 {
        return 0;
    }
    // SAFETY: called for an LVN_GETDISPINFO from one of our list views, so
    // lparam points at a valid NMLVDISPINFOW.
    let info = unsafe { &mut *(lparam as *mut NMLVDISPINFOW) };
    if info.item.mask.contains(LVIF_TEXT) {
        let value = text(info.item.iItem, info.item.iSubItem).unwrap_or("");
        write_wide(info.item.pszText.0, info.item.cchTextMax, value);
    }
    0
}

/// The owner-draw context of an `NM_CUSTOMDRAW` notification.
pub(crate) struct CustomDraw {
    /// `CDDS_*` stage.
    pub stage: u32,
    /// Item index (`-1` for the whole control).
    pub item: i32,
    /// The DC to draw into.
    pub hdc: windows::Win32::Graphics::Gdi::HDC,
}

/// What the control should do after the custom-draw callback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CustomDrawResult {
    /// Let the control draw normally (`CDRF_DODEFAULT`).
    Default,
    /// Ask for per-item notifications (`CDRF_NOTIFYITEMDRAW`).
    NotifyItemDraw,
    /// The callback painted the whole item (`CDRF_SKIPDEFAULT`).
    SkipDefault,
}

/// Runs `draw` for an `NM_CUSTOMDRAW` notification and returns the `CDRF_*`
/// code the control expects.
pub(crate) fn lv_custom_draw(
    lparam: isize,
    draw: impl FnOnce(&CustomDraw) -> CustomDrawResult,
) -> isize {
    if lparam == 0 {
        return 0;
    }
    // SAFETY: called for an NM_CUSTOMDRAW from one of our list views.
    let info = unsafe { &*(lparam as *const NMLVCUSTOMDRAW) };
    let context = CustomDraw {
        stage: info.nmcd.dwDrawStage.0,
        item: info.nmcd.dwItemSpec as i32,
        hdc: info.nmcd.hdc,
    };
    match draw(&context) {
        CustomDrawResult::Default => 0,
        CustomDrawResult::NotifyItemDraw => 32,
        CustomDrawResult::SkipDefault => 4,
    }
}

/// The rectangle of one cell, used for custom-drawn column separators.
pub(crate) fn lv_subitem_rect(hwnd: Hwnd, item: i32, sub_item: i32) -> crate::geometry::Rect {
    let mut rect = windows::Win32::Foundation::RECT::default();
    let ok = if sub_item <= 0 {
        // `LVIR_BOUNDS` (0) is passed in `rect.left`.
        send(
            hwnd,
            LVM_GETITEMRECT,
            item as usize,
            &mut rect as *mut windows::Win32::Foundation::RECT as isize,
        )
    } else {
        rect.top = sub_item;
        send(
            hwnd,
            LVM_GETSUBITEMRECT,
            item as usize,
            &mut rect as *mut windows::Win32::Foundation::RECT as isize,
        )
    };
    if ok == 0 {
        crate::geometry::Rect::default()
    } else {
        crate::geometry::Rect::new(rect.left, rect.top, rect.right, rect.bottom)
    }
}

/// Reads back a cell's text, driving the owner-data request path.
pub(crate) fn lv_item_text(hwnd: Hwnd, item: i32, sub_item: i32) -> String {
    let mut buffer = vec![0u16; 1024];
    let mut info = windows::Win32::UI::Controls::LVITEMW {
        iSubItem: sub_item,
        pszText: PWSTR(buffer.as_mut_ptr()),
        cchTextMax: buffer.len() as i32,
        ..Default::default()
    };
    let count = send(
        hwnd,
        LVM_GETITEMTEXTW,
        item as usize,
        &mut info as *mut _ as isize,
    );
    if count <= 0 {
        return String::new();
    }
    let length = (count as usize).min(buffer.len());
    String::from_utf16_lossy(&buffer[..length])
}

/// Inserts a report-mode column.
pub(crate) fn lv_insert_column(
    hwnd: Hwnd,
    index: i32,
    title: &str,
    width: i32,
    right_aligned: bool,
) {
    let mut wide: Vec<u16> = title.encode_utf16().collect();
    wide.push(0);
    let mut column = LVCOLUMNW {
        mask: LVCF_TEXT | LVCF_WIDTH | LVCF_SUBITEM | LVCF_FMT,
        cx: width,
        pszText: PWSTR(wide.as_mut_ptr()),
        cchTextMax: wide.len() as i32,
        iSubItem: index,
        fmt: if right_aligned {
            LVCFMT_RIGHT
        } else {
            LVCFMT_LEFT
        },
        ..Default::default()
    };
    send(
        hwnd,
        LVM_INSERTCOLUMNW,
        index as usize,
        &mut column as *mut LVCOLUMNW as isize,
    );
}

/// Sets a virtual list view's item count without scrolling.
pub(crate) fn lv_set_item_count(hwnd: Hwnd, count: usize) {
    send(hwnd, LVM_SETITEMCOUNT, count, LVSICF_NOSCROLL as isize);
}

/// Enables extended list-view styles (e.g. `LVS_EX_FULLROWSELECT`).
pub(crate) fn lv_set_extended_style(hwnd: Hwnd, style: u32) {
    send(
        hwnd,
        LVM_SETEXTENDEDLISTVIEWSTYLE,
        style as usize,
        style as isize,
    );
}

/// Sets the list view's background, text-background and text colours.
///
/// The control-level colours are the fallback; per-row colours come from the
/// custom-draw hook.
pub(crate) fn lv_set_colors(hwnd: Hwnd, background: Color, text: Color) {
    let colorref = background.to_colorref() as isize;
    send(hwnd, LVM_SETBKCOLOR, 0, colorref);
    send(hwnd, LVM_SETTEXTBKCOLOR, 0, colorref);
    send(hwnd, LVM_SETTEXTCOLOR, 0, text.to_colorref() as isize);
}

/// The list view's current background colour.
pub(crate) fn lv_background(hwnd: Hwnd) -> Color {
    Color::from_colorref(send(hwnd, LVM_GETBKCOLOR, 0, 0) as u32)
}

/// Whether the row is selected (the ListView does not report this through
/// `NMCUSTOMDRAW.uItemState`, so it must be queried).
pub(crate) fn lv_is_selected(hwnd: Hwnd, item: i32) -> bool {
    let state = send(
        hwnd,
        LVM_GETITEMSTATE,
        item as usize,
        LVIS_SELECTED.0 as isize,
    ) as u32;
    state & LVIS_SELECTED.0 != 0
}

/// The list view's header control.
pub(crate) fn lv_header(hwnd: Hwnd) -> Hwnd {
    hwnd_from(windows::Win32::Foundation::HWND(
        send(hwnd, LVM_GETHEADER, 0, 0) as *mut core::ffi::c_void,
    ))
}

/// The first selected item, if any.
pub(crate) fn lv_selected(hwnd: Hwnd) -> Option<i32> {
    let index = send(hwnd, LVM_GETNEXTITEM, usize::MAX, LVNI_SELECTED as isize) as i32;
    if index < 0 { None } else { Some(index) }
}

/// Every selected row, in ascending order.
///
/// Selection changes are reported per gesture (not per row), so reading the
/// whole set here is what coalesces an owner-data range change into one
/// selection event. This allocates one small `Vec` per selection change —
/// user-paced, never in a paint path.
pub(crate) fn lv_selected_all(hwnd: Hwnd) -> Vec<usize> {
    let mut rows = Vec::new();
    let mut index = -1i32;
    loop {
        index = send(
            hwnd,
            LVM_GETNEXTITEM,
            index as usize,
            LVNI_SELECTED as isize,
        ) as i32;
        if index < 0 {
            break;
        }
        rows.push(index as usize);
    }
    rows
}

/// The focused row, if any.
pub(crate) fn lv_focused(hwnd: Hwnd) -> Option<usize> {
    let index = send(hwnd, LVM_GETNEXTITEM, usize::MAX, LVNI_FOCUSED as isize) as i32;
    if index < 0 {
        None
    } else {
        Some(index as usize)
    }
}

/// Makes `row` the selection, deselecting everything else, and focuses it.
///
/// `LVM_SETITEMSTATE` notifies per item while this runs; the widget mutes
/// those and reports a single selection event itself.
pub(crate) fn lv_set_selection(hwnd: Hwnd, rows: &[usize]) {
    let cleared = LVITEMW {
        state: LIST_VIEW_ITEM_STATE_FLAGS(0),
        stateMask: LIST_VIEW_ITEM_STATE_FLAGS(LVIS_SELECTED.0),
        ..Default::default()
    };
    let mut cleared = cleared;
    send(
        hwnd,
        LVM_SETITEMSTATE,
        usize::MAX,
        &mut cleared as *mut LVITEMW as isize,
    );
    for (position, &row) in rows.iter().enumerate() {
        let mut flags = LVIS_SELECTED.0;
        if position == 0 {
            flags |= LVIS_FOCUSED.0;
        }
        let state = LIST_VIEW_ITEM_STATE_FLAGS(flags);
        let mut item = LVITEMW {
            state,
            stateMask: state,
            ..Default::default()
        };
        send(
            hwnd,
            LVM_SETITEMSTATE,
            row,
            &mut item as *mut LVITEMW as isize,
        );
    }
}

/// Scrolls `row` into view (fully; an already partially visible row stays).
pub(crate) fn lv_ensure_visible(hwnd: Hwnd, row: usize) {
    send(hwnd, LVM_ENSUREVISIBLE, row, 1);
}

/// Enables or disables multi-select by flipping `LVS_SINGLESEL`
/// (`commctrl.h`) on the live control.
pub(crate) fn lv_set_single_select(hwnd: Hwnd, single: bool) {
    // SAFETY: a `GWL_STYLE` read-modify-write on our own control; no pointer
    // is involved and the style bits fit in an `i32`.
    unsafe {
        let style = GetWindowLongW(raw_hwnd(hwnd), GWL_STYLE) as u32;
        let updated = if single {
            style | LVS_SINGLESEL
        } else {
            style & !LVS_SINGLESEL
        };
        if updated != style {
            SetWindowLongW(raw_hwnd(hwnd), GWL_STYLE, updated as i32);
        }
    }
}

/// Repaints the rows in `first..=last` without touching the rest.
pub(crate) fn lv_redraw_items(hwnd: Hwnd, first: usize, last: usize) {
    send(hwnd, LVM_REDRAWITEMS, first, last as isize);
}

/// Sets one column's width, in device pixels.
pub(crate) fn lv_set_column_width(hwnd: Hwnd, column: usize, width: i32) {
    send(hwnd, LVM_SETCOLUMNWIDTH, column, width as isize);
}

/// One column's current width, in device pixels.
pub(crate) fn lv_column_width(hwnd: Hwnd, column: usize) -> i32 {
    send(hwnd, LVM_GETCOLUMNWIDTH, column, 0) as i32
}
