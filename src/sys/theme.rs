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

/// Which documented `SetWindowTheme` sub-app name a control needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeControlKind {
    /// Scrollable controls (list, tree, header): `DarkMode_Explorer` darkens
    /// scroll bars (and the list header base) on Windows 10/11.
    Scrollable,
    /// Button/edit-style chrome (used by statics until the button/edit issues
    /// land): `DarkMode_CFD` when dark.
    Button,
}

/// Opts a control into its dark visual style, or back to `Explorer` when
/// light. Only documented `SetWindowTheme` names are used (`DarkMode_Explorer`,
/// `DarkMode_CFD`, `Explorer`); where a native part cannot be darkened this
/// way, the control owner-draws it.
pub(crate) fn apply_native_theme(hwnd: Hwnd, kind: NativeControlKind, is_dark: bool) {
    // SAFETY: `hwnd` is a live control; the theme names are static literals
    // documented for `SetWindowTheme`.
    unsafe {
        if !is_dark {
            let _ = SetWindowTheme(
                raw_hwnd(hwnd),
                windows::core::w!("Explorer"),
                PCWSTR::null(),
            );
        } else if kind == NativeControlKind::Scrollable {
            let _ = SetWindowTheme(
                raw_hwnd(hwnd),
                windows::core::w!("DarkMode_Explorer"),
                PCWSTR::null(),
            );
        } else {
            let _ = SetWindowTheme(
                raw_hwnd(hwnd),
                windows::core::w!("DarkMode_CFD"),
                PCWSTR::null(),
            );
        }
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
