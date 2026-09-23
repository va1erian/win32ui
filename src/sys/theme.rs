//! Native theming helpers: `SetWindowTheme` variants, the DWM dark title
//! bar, `WM_CTLCOLOR*` painting and class-background updates.
//!
//! All `unsafe` in this crate lives under `sys`; every block below carries a
//! `// SAFETY:` note. Only documented APIs are used: no `uxtheme` ordinals
//! (`SetPreferredAppMode`, `AllowDarkModeForWindow`). Where a native part
//! cannot be darkened this way, the control owner-draws it.

use core::ffi::c_void;

use windows::Win32::Foundation::COLORREF;
use windows::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
use windows::Win32::Graphics::Gdi::{HBRUSH, HDC, SetBkColor, SetTextColor};
use windows::Win32::UI::Controls::SetWindowTheme;
use windows::Win32::UI::WindowsAndMessaging::{
    GCLP_HBRBACKGROUND, SetClassLongPtrW, WM_CTLCOLORBTN, WM_CTLCOLORDLG, WM_CTLCOLOREDIT,
    WM_CTLCOLORLISTBOX, WM_CTLCOLORSTATIC,
};
use windows::core::PCWSTR;

use crate::color::Color;
use crate::hwnd::Hwnd;

use super::raw_hwnd;

/// `WM_CTLCOLOREDIT` id (from `WinUser.h` via the `windows` crate).
pub(crate) const fn ctlcolor_msg_edit() -> u32 {
    WM_CTLCOLOREDIT
}

/// `WM_CTLCOLORSTATIC` id.
pub(crate) const fn ctlcolor_msg_static() -> u32 {
    WM_CTLCOLORSTATIC
}

/// `WM_CTLCOLORBTN` id.
pub(crate) const fn ctlcolor_msg_btn() -> u32 {
    WM_CTLCOLORBTN
}

/// `WM_CTLCOLORLISTBOX` id.
pub(crate) const fn ctlcolor_msg_listbox() -> u32 {
    WM_CTLCOLORLISTBOX
}

/// `WM_CTLCOLORDLG` id.
pub(crate) const fn ctlcolor_msg_dlg() -> u32 {
    WM_CTLCOLORDLG
}

/// Which family a control belongs to, selecting its `SetWindowTheme` sub-app
/// name. Every bundled control currently darkens through `DarkMode_Explorer`,
/// so this only documents the family (and stays the seam if a later control
/// needs another name): `DarkMode_CFD` was tried for button chrome but left
/// buttons rendering classic-light, so it was dropped. Parts
/// `DarkMode_Explorer` still misses (radio text, the group box frame) are
/// owner-drawn by the widget instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeControlKind {
    /// Scrollable controls (list, tree, header).
    Scrollable,
    /// Button-chrome controls (push buttons, check boxes, radios, group
    /// boxes, statics).
    Button,
}

/// Opts a control into its dark visual style, or back to `Explorer` when
/// light. Only documented `SetWindowTheme` names are used
/// (`DarkMode_Explorer`, `Explorer`); where a native part cannot be darkened
/// this way, the control owner-draws it.
pub(crate) fn apply_native_theme(hwnd: Hwnd, _kind: NativeControlKind, is_dark: bool) {
    let name = if is_dark {
        windows::core::w!("DarkMode_Explorer")
    } else {
        windows::core::w!("Explorer")
    };
    // SAFETY: `hwnd` is a live control; the theme names are static literals
    // documented for `SetWindowTheme`.
    unsafe {
        let _ = SetWindowTheme(raw_hwnd(hwnd), name, PCWSTR::null());
    }
}

/// Applies (or clears) the DWM dark title bar for a top-level window.
///
/// `DWMWA_USE_IMMERSIVE_DARK_MODE` is the documented attribute; a failure
/// (pre-20H1 DWM) is ignored so theming still works without it.
pub(crate) fn set_titlebar_dark(hwnd: Hwnd, dark: bool) {
    let value: i32 = i32::from(dark);
    // SAFETY: `hwnd` is a live top-level window and `value` outlives the call;
    // `DwmSetWindowAttribute` only reads the flag.
    unsafe {
        let _ = DwmSetWindowAttribute(
            raw_hwnd(hwnd),
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &value as *const i32 as *const c_void,
            size_of::<i32>() as u32,
        );
    }
}

/// Points the window class's background brush at the cached brush for
/// `background`, so resizes after a live theme switch paint the new colour
/// instead of flashing the old class brush.
pub(crate) fn set_class_background(hwnd: Hwnd, background: Color) {
    let Some(brush) = crate::gdi::cache_brush(background) else {
        return;
    };
    // SAFETY: `hwnd` is live and `GCLP_HBRBACKGROUND` is the documented index
    // for the class background brush; the brush itself stays alive in the
    // bounded per-thread GDI cache.
    unsafe {
        SetClassLongPtrW(raw_hwnd(hwnd), GCLP_HBRBACKGROUND, brush.0 as isize);
    }
}

/// Selects the `WM_CTLCOLOR*` text/background colours into `hdc` (a raw `HDC`
/// value from `wparam`). Returns `None` when the DC is invalid.
pub(crate) fn set_ctlcolor(hdc: isize, text: Color, background: Color) -> Option<()> {
    if hdc == 0 {
        return None;
    }
    let dc = HDC(hdc as *mut c_void);
    // `CLR_INVALID` (`0xFFFFFFFF`) signals failure for both calls (from
    // `Wingdi.h` via the `windows` crate's `Graphics::Gdi` module).
    const INVALID: u32 = 0xFFFF_FFFF;
    // SAFETY: `dc` is the `WM_CTLCOLOR*` device context owned by the system
    // for the duration of the message; only plain colour values are passed.
    unsafe {
        if SetTextColor(dc, COLORREF(text.to_colorref())).0 == INVALID {
            return None;
        }
        if SetBkColor(dc, COLORREF(background.to_colorref())).0 == INVALID {
            return None;
        }
    }
    Some(())
}

/// The cached brush handle for a `WM_CTLCOLOR*` background, as an `isize` for
/// the window-procedure return value.
pub(crate) fn ctlcolor_brush(background: Color) -> Option<isize> {
    let brush: HBRUSH = crate::gdi::cache_brush(background)?;
    Some(brush.0 as isize)
}
