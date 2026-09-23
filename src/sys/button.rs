//! Raw `BUTTON`-class helpers: styles, check state, default-button handling.
//!
//! The widget layer (`controls::button`, …) builds on these so the safe code
//! never names Win32 button constants itself.

use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::Controls::{BST_CHECKED, BST_UNCHECKED};
use windows::Win32::UI::WindowsAndMessaging::{
    BM_CLICK, BM_GETCHECK, BM_SETCHECK, BM_SETSTYLE, BS_AUTOCHECKBOX, BS_AUTORADIOBUTTON,
    BS_DEFPUSHBUTTON, BS_GROUPBOX, BS_PUSHBUTTON, BS_TYPEMASK, DM_SETDEFID, GWL_STYLE,
    GetWindowLongPtrW, SendMessageW, WS_GROUP,
};

use crate::hwnd::Hwnd;

use super::raw_hwnd;

/// `BUTTON` style for a push button, default or not (`Winuser.h`: `BS_*`).
pub(crate) fn button_style(is_default: bool) -> u32 {
    if is_default {
        BS_DEFPUSHBUTTON as u32
    } else {
        BS_PUSHBUTTON as u32
    }
}

/// `BUTTON` style for a two-state check box (`Winuser.h`: `BS_AUTOCHECKBOX`).
pub(crate) fn checkbox_style() -> u32 {
    BS_AUTOCHECKBOX as u32
}

/// `BUTTON` style for one radio button (`Winuser.h`: `BS_AUTORADIOBUTTON`).
/// The first button of a group also gets `WS_GROUP` so the native
/// auto-exclusive behaviour is scoped to this group.
pub(crate) fn radio_style(first: bool) -> u32 {
    let mut style = BS_AUTORADIOBUTTON as u32;
    if first {
        style |= WS_GROUP.0;
    }
    style
}

/// `BUTTON` style for a labelled frame (`Winuser.h`: `BS_GROUPBOX`).
pub(crate) fn groupbox_style() -> u32 {
    BS_GROUPBOX as u32
}

/// Whether the button is checked (`BM_GETCHECK` against `BST_CHECKED`).
pub(crate) fn is_checked(hwnd: Hwnd) -> bool {
    // SAFETY: `SendMessageW` with `BM_GETCHECK` takes no pointer and returns
    // the check state directly.
    let state = unsafe { SendMessageW(raw_hwnd(hwnd), BM_GETCHECK, None, None) };
    state.0 as u32 == BST_CHECKED.0
}

/// Sets the check state (`BM_SETCHECK` with `BST_CHECKED`/`BST_UNCHECKED`).
pub(crate) fn set_checked(hwnd: Hwnd, checked: bool) {
    let state = if checked {
        BST_CHECKED.0
    } else {
        BST_UNCHECKED.0
    };
    // SAFETY: `BM_SETCHECK` takes the state in `wparam` and no pointer.
    unsafe {
        SendMessageW(
            raw_hwnd(hwnd),
            BM_SETCHECK,
            Some(WPARAM(state as usize)),
            None,
        );
    }
}

/// Whether the push button carries the default style (`BS_DEFPUSHBUTTON`).
pub(crate) fn is_default(hwnd: Hwnd) -> bool {
    // SAFETY: `GWL_STYLE` is the documented index for the window style bits.
    let style = unsafe { GetWindowLongPtrW(raw_hwnd(hwnd), GWL_STYLE) };
    (style as i32 & BS_TYPEMASK) == BS_DEFPUSHBUTTON
}

/// Marks a push button as the window's default (or removes the mark).
///
/// The visible style is switched with `BM_SETSTYLE` and the dialog manager's
/// default id follows with `DM_SETDEFID` to the parent, so Enter activates it
/// through the `IsDialogMessageW` navigation enabled for app windows.
pub(crate) fn set_default(hwnd: Hwnd, parent: Hwnd, id: usize, is_default: bool) {
    let style = button_style(is_default);
    // SAFETY: `BM_SETSTYLE` takes the new button style in `wparam` and a
    // redraw flag in `lparam`; no pointer is involved.
    unsafe {
        SendMessageW(
            raw_hwnd(hwnd),
            BM_SETSTYLE,
            Some(WPARAM(style as usize)),
            Some(LPARAM(1)),
        );
        SendMessageW(
            raw_hwnd(parent),
            DM_SETDEFID,
            Some(WPARAM(if is_default { id } else { 0 })),
            None,
        );
    }
}

/// Simulates a user click (`BM_CLICK`), firing `BN_CLICKED` synchronously.
pub(crate) fn click(hwnd: Hwnd) {
    // SAFETY: `BM_CLICK` takes no parameters and no pointer.
    unsafe {
        SendMessageW(raw_hwnd(hwnd), BM_CLICK, None, None);
    }
}
