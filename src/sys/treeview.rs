//! Raw tree-view (`SysTreeView32`) messages: lazy insertion, in-place updates,
//! selection/expansion by handle, custom draw and image lists.
//!
//! Every tree item carries a nonzero token in its `lParam`; notifications
//! report that token, and the widget layer maps it back to the node's typed
//! key.

use windows::Win32::Foundation::COLORREF;
use windows::Win32::Graphics::Gdi::HDC;
use windows::Win32::UI::Controls::{
    CDIS_HOT, CDIS_SELECTED, CDRF_DODEFAULT, CDRF_NEWFONT, CDRF_NOTIFYITEMDRAW,
    CDRF_NOTIFYPOSTPAINT, HIMAGELIST, HTREEITEM, I_IMAGENONE, NMTREEVIEWW, NMTVCUSTOMDRAW,
    TREE_VIEW_ITEM_STATE_FLAGS, TVE_COLLAPSE, TVE_EXPAND, TVGN_CARET, TVIF_CHILDREN, TVIF_HANDLE,
    TVIF_IMAGE, TVIF_PARAM, TVIF_STATE, TVIF_TEXT, TVINSERTSTRUCTW, TVINSERTSTRUCTW_0, TVIS_BOLD,
    TVIS_EXPANDED, TVITEMEXW_CHILDREN, TVITEMW, TVM_DELETEITEM, TVM_ENSUREVISIBLE, TVM_EXPAND,
    TVM_GETCOUNT, TVM_GETITEMW, TVM_GETNEXTITEM, TVM_INSERTITEMW, TVM_SELECTITEM, TVM_SETBKCOLOR,
    TVM_SETEXTENDEDSTYLE, TVM_SETIMAGELIST, TVM_SETITEMHEIGHT, TVM_SETITEMW, TVM_SETTEXTCOLOR,
    TVSIL_NORMAL,
};
use windows::Win32::UI::Input::KeyboardAndMouse::GetFocus;
use windows::core::PWSTR;

use crate::color::Color;
use crate::geometry::Rect;
use crate::hwnd::Hwnd;

use super::control::send;
use super::hwnd_from;

/// Insert as the last root/child (`TVI_ROOT` / `TVI_LAST`, `commctrl.h`).
pub(crate) const TVI_ROOT: isize = -65536;
/// Insert before every sibling (`TVI_FIRST`, `commctrl.h`).
pub(crate) const TVI_FIRST: isize = -65535;

/// Inserts one item and returns its handle.
pub(crate) fn tv_insert(
    hwnd: Hwnd,
    parent: isize,
    after: isize,
    text: &str,
    token: i64,
    has_children: bool,
    image: i32,
) -> isize {
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    wide.push(0);
    let mut item = TVINSERTSTRUCTW {
        hParent: HTREEITEM(parent),
        hInsertAfter: HTREEITEM(after),
        Anonymous: TVINSERTSTRUCTW_0 {
            item: TVITEMW {
                mask: TVIF_TEXT | TVIF_PARAM | TVIF_CHILDREN | TVIF_IMAGE,
                pszText: PWSTR(wide.as_mut_ptr()),
                cchTextMax: wide.len() as i32,
                lParam: windows::Win32::Foundation::LPARAM(token as isize),
                cChildren: TVITEMEXW_CHILDREN(i32::from(has_children)),
                iImage: image,
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

/// Updates one item in place: its text, children flag and/or image. Only the
/// fields passed as `Some` are touched.
pub(crate) fn tv_set_item(
    hwnd: Hwnd,
    handle: isize,
    text: Option<&str>,
    has_children: Option<bool>,
    image: Option<i32>,
) {
    let mut wide: Vec<u16> = Vec::new();
    let mut mask = TVIF_HANDLE;
    if let Some(text) = text {
        wide = text.encode_utf16().collect();
        wide.push(0);
        mask |= TVIF_TEXT;
    }
    if has_children.is_some() {
        mask |= TVIF_CHILDREN;
    }
    if image.is_some() {
        mask |= TVIF_IMAGE;
    }
    let mut item = TVITEMW {
        mask,
        hItem: HTREEITEM(handle),
        pszText: PWSTR(wide.as_mut_ptr()),
        cchTextMax: wide.len() as i32,
        cChildren: TVITEMEXW_CHILDREN(i32::from(has_children.unwrap_or(false))),
        iImage: image.unwrap_or(I_IMAGENONE),
        ..Default::default()
    };
    send(hwnd, TVM_SETITEMW, 0, &mut item as *mut TVITEMW as isize);
}

/// Deletes one item and its whole subtree (`TVI_ROOT` deletes everything).
pub(crate) fn tv_delete(hwnd: Hwnd, handle: isize) {
    send(hwnd, TVM_DELETEITEM, 0, handle);
}

/// Total number of inserted items.
pub(crate) fn tv_count(hwnd: Hwnd) -> i32 {
    send(hwnd, TVM_GETCOUNT, 0, 0) as i32
}

/// Enables tree-view extended styles (e.g. `TVS_EX_DOUBLEBUFFER`).
pub(crate) fn tv_set_extended_style(hwnd: Hwnd, style: u32) {
    send(hwnd, TVM_SETEXTENDEDSTYLE, style as usize, style as isize);
}

/// Sets the height of every item (device pixels).
pub(crate) fn tv_set_item_height(hwnd: Hwnd, height: i32) {
    send(hwnd, TVM_SETITEMHEIGHT, height.max(0) as usize, 0);
}

/// Sets the tree view's background and text colours.
pub(crate) fn tv_set_colors(hwnd: Hwnd, background: Color, text: Color) {
    send(hwnd, TVM_SETBKCOLOR, 0, background.to_colorref() as isize);
    send(hwnd, TVM_SETTEXTCOLOR, 0, text.to_colorref() as isize);
}

/// Attaches (or clears, with `None`) the normal image list.
pub(crate) fn tv_set_image_list(hwnd: Hwnd, list: Option<HIMAGELIST>) {
    let raw = list.map(|list| list.0).unwrap_or(0);
    send(hwnd, TVM_SETIMAGELIST, TVSIL_NORMAL as usize, raw);
}

/// Makes `handle` the selection and caret.
pub(crate) fn tv_select(hwnd: Hwnd, handle: isize) {
    send(hwnd, TVM_SELECTITEM, TVGN_CARET as usize, handle);
}

/// Expands or collapses `handle`. Both send `TVN_ITEMEXPANDING` first.
pub(crate) fn tv_expand(hwnd: Hwnd, handle: isize, expand: bool) {
    let action = if expand { TVE_EXPAND } else { TVE_COLLAPSE };
    send(hwnd, TVM_EXPAND, action.0 as usize, handle);
}

/// Scrolls `handle` into view.
pub(crate) fn tv_ensure_visible(hwnd: Hwnd, handle: isize) {
    send(hwnd, TVM_ENSUREVISIBLE, 0, handle);
}

/// Whether `handle` is currently expanded.
pub(crate) fn tv_is_expanded(hwnd: Hwnd, handle: isize) -> bool {
    let mut item = TVITEMW {
        mask: TVIF_HANDLE | TVIF_STATE,
        hItem: HTREEITEM(handle),
        stateMask: TVIS_EXPANDED,
        ..Default::default()
    };
    let ok = send(hwnd, TVM_GETITEMW, 0, &mut item as *mut TVITEMW as isize);
    ok != 0 && item.state.0 & TVIS_EXPANDED.0 != 0
}

/// The caret item as `(handle, token)`, if any.
pub(crate) fn tv_selected(hwnd: Hwnd) -> Option<(isize, i64)> {
    let handle = send(hwnd, TVM_GETNEXTITEM, TVGN_CARET as usize, 0);
    if handle == 0 {
        return None;
    }
    let token = tv_item_token(hwnd, handle)?;
    Some((handle, token))
}

/// Reads one item's `lParam` token.
pub(crate) fn tv_item_token(hwnd: Hwnd, handle: isize) -> Option<i64> {
    let mut item = TVITEMW {
        mask: TVIF_HANDLE | TVIF_PARAM,
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

/// Whether the tree view itself has the keyboard focus; selection is drawn
/// with the focused `selection` colour only while it does.
pub(crate) fn tv_has_focus(hwnd: Hwnd) -> bool {
    // SAFETY: `GetFocus` takes no arguments and only returns the calling
    // thread's focused window (or null); comparing handles touches nothing.
    unsafe { hwnd_from(GetFocus()) == hwnd }
}

/// Reads the item being expanded from a `TVN_ITEMEXPANDING` notification as
/// `(handle, token)`.
pub(crate) fn tv_expanding(lparam: isize) -> Option<(isize, i64)> {
    if lparam == 0 {
        return None;
    }
    // SAFETY: called for a TVN_ITEMEXPANDING from one of our tree views.
    let info = unsafe { &*(lparam as *const NMTREEVIEWW) };
    Some((info.itemNew.hItem.0, info.itemNew.lParam.0 as i64))
}

/// The owner-draw context of a tree `NM_CUSTOMDRAW`.
pub(crate) struct CustomDraw {
    /// `CDDS_*` stage.
    pub stage: u32,
    /// The item's `lParam` token.
    pub token: i64,
    /// The DC to draw into.
    pub hdc: HDC,
    /// Whether the item is selected (`CDIS_SELECTED`).
    pub selected: bool,
    /// Whether the pointer is hovering the item (`CDIS_HOT`).
    pub hot: bool,
    /// The item's rectangle.
    pub rect: Rect,
    /// Set to override the item's text colour (`clrText`).
    pub text: Option<Color>,
    /// Set to override the item's background (`clrTextBk`).
    pub background: Option<Color>,
}

/// What the control should do after the custom-draw callback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CustomDrawResult {
    /// Let the control draw normally (`CDRF_DODEFAULT`).
    Default,
    /// Ask for per-item notifications (`CDRF_NOTIFYITEMDRAW`).
    NotifyItemDraw,
    /// Use the callback's colours/font (`CDRF_NEWFONT`).
    NewFont,
    /// Use the callback's colours/font and ask for a post-paint pass
    /// (`CDRF_NEWFONT | CDRF_NOTIFYPOSTPAINT`).
    NewFontAndPostPaint,
}

/// Runs `draw` for a tree `NM_CUSTOMDRAW` and returns the `CDRF_*` code the
/// control expects. The callback fills `text`/`background` to recolour the
/// item; the callback owns any font selection it makes into `hdc`.
pub(crate) fn tv_custom_draw(
    lparam: isize,
    draw: impl FnOnce(&mut CustomDraw) -> CustomDrawResult,
) -> isize {
    if lparam == 0 {
        return CDRF_DODEFAULT as isize;
    }
    // SAFETY: called for an NM_CUSTOMDRAW from one of our tree views.
    let info = unsafe { &mut *(lparam as *mut NMTVCUSTOMDRAW) };
    let state = info.nmcd.uItemState.0;
    let rc = info.nmcd.rc;
    let mut context = CustomDraw {
        stage: info.nmcd.dwDrawStage.0,
        token: info.nmcd.lItemlParam.0 as i64,
        hdc: info.nmcd.hdc,
        selected: state & CDIS_SELECTED.0 != 0,
        hot: state & CDIS_HOT.0 != 0,
        rect: Rect::new(rc.left, rc.top, rc.right, rc.bottom),
        text: None,
        background: None,
    };
    let result = draw(&mut context);
    if let Some(color) = context.text {
        info.clrText = COLORREF(color.to_colorref());
    }
    if let Some(color) = context.background {
        info.clrTextBk = COLORREF(color.to_colorref());
    }
    match result {
        CustomDrawResult::Default => CDRF_DODEFAULT as isize,
        CustomDrawResult::NotifyItemDraw => CDRF_NOTIFYITEMDRAW as isize,
        CustomDrawResult::NewFont => CDRF_NEWFONT as isize,
        CustomDrawResult::NewFontAndPostPaint => (CDRF_NEWFONT | CDRF_NOTIFYPOSTPAINT) as isize,
    }
}

/// Turns the native bold state (`TVIS_BOLD`) of `handle` on or off. The control
/// measures a bold item with its own bold font, so the label is never clipped
/// the way text drawn in a wider font than the one it was measured in is.
pub(crate) fn tv_set_bold(hwnd: Hwnd, handle: isize, bold: bool) {
    let mut item = TVITEMW {
        mask: TVIF_HANDLE | TVIF_STATE,
        hItem: HTREEITEM(handle),
        state: TREE_VIEW_ITEM_STATE_FLAGS(if bold { TVIS_BOLD.0 } else { 0 }),
        stateMask: TVIS_BOLD,
        ..Default::default()
    };
    send(hwnd, TVM_SETITEMW, 0, &mut item as *mut TVITEMW as isize);
}
