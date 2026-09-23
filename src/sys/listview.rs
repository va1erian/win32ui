//! Raw list-view (`SysListView32`) messages: owner-data requests, custom draw,
//! columns, selection state and the header subclass that feeds the
//! owner-drawn header.

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::HDC;
use windows::Win32::UI::Controls::{
    LVCF_FMT, LVCF_SUBITEM, LVCF_TEXT, LVCF_WIDTH, LVCFMT_LEFT, LVCFMT_RIGHT, LVCOLUMNW, LVIF_TEXT,
    LVIS_FOCUSED, LVIS_SELECTED, LVM_GETBKCOLOR, LVM_GETHEADER, LVM_GETITEMRECT, LVM_GETITEMSTATE,
    LVM_GETITEMTEXTW, LVM_GETNEXTITEM, LVM_GETSUBITEMRECT, LVM_INSERTCOLUMNW, LVM_SETBKCOLOR,
    LVM_SETEXTENDEDLISTVIEWSTYLE, LVM_SETITEMCOUNT, LVM_SETITEMSTATE, LVM_SETTEXTBKCOLOR,
    LVM_SETTEXTCOLOR, LVNI_SELECTED, LVSICF_NOSCROLL, NM_CUSTOMDRAW, NMCUSTOMDRAW, NMHDR,
    NMLVCUSTOMDRAW, NMLVDISPINFOW,
};
use windows::Win32::UI::Shell::DefSubclassProc;
use windows::Win32::UI::WindowsAndMessaging::WM_NOTIFY;
use windows::core::PWSTR;

use crate::color::Color;
use crate::geometry::Rect;
use crate::hwnd::Hwnd;

use super::control::{send, write_wide};
use super::hwnd_from;

/// Answers an `LVN_GETDISPINFO` request by calling `text(item, sub_item)`.
pub(crate) fn lv_disp_info(lparam: isize, text: impl FnOnce(i32, i32) -> String) -> isize {
    if lparam == 0 {
        return 0;
    }
    // SAFETY: called for an LVN_GETDISPINFO from one of our list views, so
    // lparam points at a valid NMLVDISPINFOW.
    let info = unsafe { &mut *(lparam as *mut NMLVDISPINFOW) };
    if info.item.mask.contains(LVIF_TEXT) {
        let value = text(info.item.iItem, info.item.iSubItem);
        write_wide(info.item.pszText.0, info.item.cchTextMax, &value);
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

/// One `NM_CUSTOMDRAW` notification from a list view's header control.
pub(crate) struct HeaderDraw {
    /// The `CDDS_*` stage.
    pub stage: u32,
    /// The header item (column) index.
    pub item: i32,
    /// The DC to paint into.
    pub hdc: HDC,
    /// The item rectangle.
    pub rect: Rect,
}

/// Implemented by the owner of a list view's header to paint it.
pub(crate) trait HeaderPainter {
    /// Handles one header custom-draw stage, returning the `CDRF_*` code to
    /// send back, or `None` to let the header draw itself.
    fn draw_header(&self, draw: &HeaderDraw) -> Option<isize>;
}

struct HeaderRefdata {
    header: Hwnd,
    painter: Box<dyn HeaderPainter>,
}

/// Owns a header painter and the subclass that feeds it header notifications.
pub(crate) struct HeaderSubclass {
    listview: Hwnd,
    raw: *mut HeaderRefdata,
}

impl HeaderSubclass {
    /// Subclasses `listview` so header `NM_CUSTOMDRAW` notifications reach
    /// `painter`. Returns `None` if subclassing fails.
    pub(crate) fn install(
        listview: Hwnd,
        header: Hwnd,
        painter: Box<dyn HeaderPainter>,
    ) -> Option<HeaderSubclass> {
        let raw = Box::into_raw(Box::new(HeaderRefdata { header, painter }));
        if !super::window::set_subclass(
            listview,
            Some(header_proc),
            HEADER_SUBCLASS_ID,
            raw as usize,
        ) {
            // SAFETY: install failed before the subclass could adopt it.
            unsafe { drop(Box::from_raw(raw)) };
            return None;
        }
        Some(HeaderSubclass { listview, raw })
    }
}

impl Drop for HeaderSubclass {
    fn drop(&mut self) {
        super::window::remove_subclass(self.listview, Some(header_proc), HEADER_SUBCLASS_ID);
        // SAFETY: installed in `install` and reclaimed exactly once.
        unsafe { drop(Box::from_raw(self.raw)) };
    }
}

const HEADER_SUBCLASS_ID: usize = 0x7768_6472; // "whdr"

/// The subclass procedure that routes header custom-draw notifications.
///
/// # Safety
/// Called by Windows for the list view subclass installed by
/// [`HeaderSubclass::install`]; `refdata` is the `HeaderRefdata` pointer.
unsafe extern "system" fn header_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    refdata: usize,
) -> LRESULT {
    if msg == WM_NOTIFY && lparam.0 != 0 {
        // SAFETY: WM_NOTIFY's lparam points at an NMHDR for the duration of the
        // call; `refdata` is the live `HeaderRefdata` installed by `install`,
        // and an NM_CUSTOMDRAW's lparam is an NMCUSTOMDRAW.
        unsafe {
            let header = &*(lparam.0 as *const NMHDR);
            if header.code == NM_CUSTOMDRAW {
                let data = &*(refdata as *const HeaderRefdata);
                if hwnd_from(header.hwndFrom) == data.header {
                    let draw = &*(lparam.0 as *const NMCUSTOMDRAW);
                    let request = HeaderDraw {
                        stage: draw.dwDrawStage.0,
                        item: draw.dwItemSpec as i32,
                        hdc: draw.hdc,
                        rect: Rect::new(draw.rc.left, draw.rc.top, draw.rc.right, draw.rc.bottom),
                    };
                    // A panic unwinding across this `extern "system"` boundary
                    // is undefined behaviour; isolate it instead.
                    let painted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        data.painter.draw_header(&request)
                    }))
                    .unwrap_or(None);
                    if let Some(result) = painted {
                        return LRESULT(result);
                    }
                }
            }
        }
    }
    // SAFETY: forward to the subclass chain's original window procedure.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

/// The first selected item, if any.
pub(crate) fn lv_selected(hwnd: Hwnd) -> Option<i32> {
    let index = send(hwnd, LVM_GETNEXTITEM, usize::MAX, LVNI_SELECTED as isize) as i32;
    if index < 0 { None } else { Some(index) }
}

/// Selects and focuses `index`.
pub(crate) fn lv_select(hwnd: Hwnd, index: i32) {
    let state =
        windows::Win32::UI::Controls::LIST_VIEW_ITEM_STATE_FLAGS(LVIS_SELECTED.0 | LVIS_FOCUSED.0);
    let mut item = windows::Win32::UI::Controls::LVITEMW {
        state,
        stateMask: state,
        ..Default::default()
    };
    send(
        hwnd,
        LVM_SETITEMSTATE,
        index as usize,
        &mut item as *mut _ as isize,
    );
}
