//! Raw plumbing for the native tab control (`SysTabControl32`).
//!
//! The control's tabs ignore dark mode, so the widget creates it with
//! `TCS_OWNERDRAWFIXED` and paints each tab from theme tokens on `WM_DRAWITEM`.
//! This module owns creation, the `TCM_*` messages the layout needs, and the
//! small subclass ([`TabHost`]) that gives the owner-drawn tabs hover and
//! `Ctrl+Tab` behaviour; [`draw`] holds the painting and [`host`] the subclass.
//!
//! All `unsafe` in this crate lives under `sys`; every block below carries a
//! `// SAFETY:` note. Constants come from the `windows` crate (generated from
//! `CommCtrl.h`).

mod draw;
mod host;

pub(crate) use draw::{TabPaint, TabVisual, decode_state, draw_tab, fill};
pub(crate) use host::{TabEvent, TabHost};

use windows::Win32::Foundation::{LPARAM, POINT, RECT};
use windows::Win32::UI::Controls::{
    TAB_CONTROL_ITEM_STATE, TCHITTESTINFO, TCHITTESTINFO_FLAGS, TCIF_TEXT, TCITEMHEADERA_MASK,
    TCITEMW, TCM_ADJUSTRECT, TCM_GETCURSEL, TCM_GETITEMRECT, TCM_HITTEST, TCM_INSERTITEMW,
    TCM_SETCURSEL, TCM_SETITEMSIZE, TCN_SELCHANGE, TCS_FIXEDWIDTH, TCS_OWNERDRAWFIXED,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetFocus, VK_TAB};
use windows::Win32::UI::WindowsAndMessaging::{WS_CHILD, WS_CLIPSIBLINGS, WS_TABSTOP, WS_VISIBLE};
use windows::core::PWSTR;

use crate::error::Result;
use crate::geometry::Rect;
use crate::hwnd::Hwnd;

/// `TCN_SELCHANGE` (`CommCtrl.h` via the `windows` crate): the user picked a
/// different tab. Programmatic `TCM_SETCURSEL` does not raise it.
pub(crate) fn sel_change_code() -> u32 {
    TCN_SELCHANGE
}

/// Whether a virtual key is Tab, so the safe layer can decode `Ctrl+Tab`.
pub(crate) fn is_tab_key(key: u32) -> bool {
    key == VK_TAB.0 as u32
}

/// Creates the native tab control as a child of `parent`, in the owner-drawn
/// style whose tabs the widget paints itself.
pub(crate) fn create(parent: Hwnd, id: usize, bounds: Rect) -> Result<Hwnd> {
    crate::sys::control::init_common_controls()?;
    let style = WS_CHILD.0
        | WS_VISIBLE.0
        | WS_TABSTOP.0
        | WS_CLIPSIBLINGS.0
        | TCS_OWNERDRAWFIXED
        | TCS_FIXEDWIDTH;
    crate::sys::window::create_control("SysTabControl32", style, 0, parent, id, bounds)
        .map(crate::sys::hwnd_from)
}

/// Inserts a tab whose label is `text` at `index`.
pub(crate) fn insert_item(hwnd: Hwnd, index: i32, text: &str) {
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    wide.push(0);
    let item = TCITEMW {
        mask: TCITEMHEADERA_MASK(TCIF_TEXT.0),
        dwState: TAB_CONTROL_ITEM_STATE(0),
        dwStateMask: TAB_CONTROL_ITEM_STATE(0),
        pszText: PWSTR(wide.as_mut_ptr()),
        cchTextMax: wide.len() as i32,
        iImage: 0,
        lParam: LPARAM(0),
    };
    crate::sys::control::send(
        hwnd,
        TCM_INSERTITEMW,
        index as usize,
        &item as *const TCITEMW as isize,
    );
}

/// Sizes every tab so its label fits: a uniform item size measured from the
/// widest title in `font`, plus padding. Owner-drawn fixed tabs otherwise get
/// the control's small default width, which truncates the labels
/// (`TCM_SETITEMSIZE`).
pub(crate) fn fit_items(hwnd: Hwnd, font: windows::Win32::Graphics::Gdi::HFONT, titles: &[String]) {
    let mut width = 0;
    let mut height = 0;
    for title in titles {
        let size = crate::sys::gdi::measure_text(font, title);
        width = width.max(size.width);
        height = height.max(size.height);
    }
    let width = width + 24;
    let height = height + 8;
    let packed = (width & 0xffff) | ((height & 0xffff) << 16);
    crate::sys::control::send(hwnd, TCM_SETITEMSIZE, 0, packed as isize);
}

/// Selects the tab at `index` without raising `TCN_SELCHANGE`.
pub(crate) fn set_cur_sel(hwnd: Hwnd, index: usize) {
    crate::sys::control::send(hwnd, TCM_SETCURSEL, index, 0);
}

/// The selected tab index, or `None` when the control has no selection.
pub(crate) fn cur_sel(hwnd: Hwnd) -> Option<usize> {
    let selected = crate::sys::control::send(hwnd, TCM_GETCURSEL, 0, 0);
    (selected >= 0).then_some(selected as usize)
}

/// The page (display) rectangle for a tab control occupying `bounds`, via
/// `TCM_ADJUSTRECT`. `bounds` is in the parent's coordinates; the returned
/// rectangle is too.
pub(crate) fn page_rect(hwnd: Hwnd, bounds: Rect) -> Rect {
    let mut raw = RECT {
        left: 0,
        top: 0,
        right: bounds.width(),
        bottom: bounds.height(),
    };
    // SAFETY: `raw` is a valid in/out `RECT`; `wparam = FALSE` (`TabCtrl_
    // AdjustRect`'s `bLarger`) converts the given window rectangle into the
    // display rectangle.
    let _ = crate::sys::control::send(hwnd, TCM_ADJUSTRECT, 0, &mut raw as *mut RECT as isize);
    Rect::new(
        bounds.left + raw.left,
        bounds.top + raw.top,
        bounds.left + raw.right,
        bounds.top + raw.bottom,
    )
}

/// The client rectangle of one tab, via `TCM_GETITEMRECT`.
pub(crate) fn tab_rect(hwnd: Hwnd, index: usize) -> Option<Rect> {
    let mut raw = RECT::default();
    let ok =
        crate::sys::control::send(hwnd, TCM_GETITEMRECT, index, &mut raw as *mut RECT as isize);
    (ok != 0).then(|| Rect::new(raw.left, raw.top, raw.right, raw.bottom))
}

/// The tab under the client point `(x, y)`, or `None` for no tab.
pub(crate) fn hit_test(hwnd: Hwnd, x: i32, y: i32) -> Option<usize> {
    let mut info = TCHITTESTINFO {
        pt: POINT { x, y },
        flags: TCHITTESTINFO_FLAGS(0),
    };
    let hit = crate::sys::control::send(
        hwnd,
        TCM_HITTEST,
        0,
        &mut info as *mut TCHITTESTINFO as isize,
    );
    (hit >= 0).then_some(hit as usize)
}

/// Whether the tab control currently has the keyboard focus.
pub(crate) fn has_focus(hwnd: Hwnd) -> bool {
    // SAFETY: `GetFocus` only reads the calling thread's focus window.
    unsafe { GetFocus() == crate::sys::raw_hwnd(hwnd) }
}
