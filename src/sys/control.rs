//! Common-control initialisation and the raw control-specific messages.

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::HFONT;
use windows::Win32::UI::Controls::{
    ICC_BAR_CLASSES, ICC_LISTVIEW_CLASSES, ICC_STANDARD_CLASSES, ICC_TREEVIEW_CLASSES,
    INITCOMMONCONTROLSEX, INITCOMMONCONTROLSEX_ICC, InitCommonControlsEx,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, SendMessageW, WM_GETFONT, WM_SETFONT,
};
use windows::core::BOOL;

use crate::error::{Error, Result};
use crate::gdi::Font;
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

/// Copies `value` into the sender-owned UTF-16 buffer as a NUL-terminated
/// string, truncating at a `char` boundary when it does not fit.
///
/// Nothing is allocated: each `char` encodes to at most two units, copied
/// straight into the destination. The hot paths (`LVN_GETDISPINFO`) must not
/// allocate per cell, so callers pass text borrowed from the row.
pub(crate) fn write_wide(destination: *mut u16, capacity: i32, value: &str) {
    if destination.is_null() || capacity <= 0 {
        return;
    }
    let capacity = capacity as usize;
    let mut written = 0usize;
    let mut encoded = [0u16; 2];
    for ch in value.chars() {
        let units = ch.encode_utf16(&mut encoded);
        if written + units.len() >= capacity {
            break;
        }
        // SAFETY: `written + units.len() < capacity`, so the range lies inside
        // the `capacity` writable units owned by the sender.
        unsafe {
            std::ptr::copy_nonoverlapping(units.as_ptr(), destination.add(written), units.len());
        }
        written += units.len();
    }
    // SAFETY: `written <= capacity - 1`, so the terminator lands inside the
    // sender's buffer.
    unsafe {
        *destination.add(written) = 0;
    }
}

/// Gives a control a font (and asks it to repaint).
pub(crate) fn set_control_font(hwnd: Hwnd, font: HFONT) {
    send(hwnd, WM_SETFONT, font.0 as usize, 1);
}

/// The font a control currently uses, if it has one (a control on the stock
/// font answers `WM_GETFONT` with null).
pub(crate) fn current_font(hwnd: Hwnd) -> Option<HFONT> {
    let raw = send(hwnd, WM_GETFONT, 0, 0);
    (raw != 0).then_some(HFONT(raw as *mut _))
}

/// Gives a control the shared UI font for `dpi`. Every control created through
/// `create_child` gets this, so no control is left on the stock font.
pub(crate) fn apply_ui_font(hwnd: Hwnd, dpi: u32) {
    if let Ok(font) = Font::shared_ui(dpi) {
        set_control_font(hwnd, font.raw());
    }
}

/// Runs `change` (a `SetWindowTheme` call, say) and restores the control's font
/// afterwards: comctl32 resets it to the stock font when the visual style
/// changes.
pub(crate) fn keep_font(hwnd: Hwnd, change: impl FnOnce()) {
    let font = current_font(hwnd);
    change();
    if let Some(font) = font {
        set_control_font(hwnd, font);
    }
}

/// Moves every descendant of `root` that uses the shared UI font to the one for
/// `dpi`. A font a caller chose with `set_font` is left alone.
pub(crate) fn refresh_ui_fonts(root: Hwnd, dpi: u32) {
    let Ok(font) = Font::shared_ui(dpi) else {
        return;
    };
    let mut targets: Vec<HWND> = Vec::new();
    // SAFETY: `collect` only pushes into `targets`, which outlives the
    // synchronous enumeration.
    unsafe {
        let _ = EnumChildWindows(
            Some(raw_hwnd(root)),
            Some(collect),
            LPARAM(&mut targets as *mut Vec<HWND> as isize),
        );
    }
    for target in targets {
        let hwnd = super::hwnd_from(target);
        if current_font(hwnd).is_some_and(Font::is_shared_ui) {
            set_control_font(hwnd, font.raw());
        }
    }
}

unsafe extern "system" fn collect(hwnd: HWND, out: LPARAM) -> BOOL {
    // SAFETY: `out` is the `Vec<HWND>` `refresh_ui_fonts` passed; the
    // enumeration is synchronous on the same thread.
    unsafe { &mut *(out.0 as *mut Vec<HWND>) }.push(hwnd);
    BOOL(1)
}
