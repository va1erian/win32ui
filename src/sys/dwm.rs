//! DWM helpers: the system backdrop material and the themed caption.
//!
//! All `unsafe` in this crate lives under `sys`; every block below carries a
//! `// SAFETY:` note. Only documented DWM and registry APIs are used — no
//! undocumented `uxtheme` ordinals or backdrop attributes.
//!
//! The material is a best-effort feature: Windows 10 and Windows 11 builds
//! before 22621 reject `DWMWA_SYSTEMBACKDROP_TYPE`, so the fallback is decided
//! by the call's `HRESULT` (and the user's high-contrast / transparency
//! settings), never by parsing the OS version.

use core::ffi::c_void;

use windows::Win32::Foundation::{COLORREF, ERROR_SUCCESS};
use windows::Win32::Graphics::Dwm::{
    DWM_WINDOW_CORNER_PREFERENCE, DWMSBT_MAINWINDOW, DWMSBT_TABBEDWINDOW, DWMSBT_TRANSIENTWINDOW,
    DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_COLOR_NONE, DWMWA_SYSTEMBACKDROP_TYPE,
    DWMWA_TEXT_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DWMWINDOWATTRIBUTE,
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute,
};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::WindowsAndMessaging::{
    SPI_GETHIGHCONTRAST, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
};
use windows::core::w;

use crate::hwnd::Hwnd;
use crate::theme::Theme;
use crate::window::Backdrop;

use super::raw_hwnd;

/// The `DWMWA_SYSTEMBACKDROP_TYPE` value for a [`Backdrop`], or `None` for the
/// solid theme background.
fn system_backdrop(backdrop: Backdrop) -> Option<i32> {
    match backdrop {
        Backdrop::None => None,
        Backdrop::Mica => Some(DWMSBT_MAINWINDOW.0),
        Backdrop::MicaAlt => Some(DWMSBT_TABBEDWINDOW.0),
        Backdrop::Acrylic => Some(DWMSBT_TRANSIENTWINDOW.0),
    }
}

/// Whether the material may be shown, given the user's settings and the DWM
/// call's `HRESULT`. Pure, so the fallback rule is unit-tested rather than
/// depending on the machine.
fn backdrop_supported(high_contrast: bool, transparency: bool, hresult: i32) -> bool {
    !high_contrast && transparency && hresult >= 0
}

/// Whether high-contrast mode is on, in which case the material and themed
/// caption are skipped so the app keeps the system's accessible colours.
fn high_contrast() -> bool {
    let mut info = HIGHCONTRASTW {
        cbSize: size_of::<HIGHCONTRASTW>() as u32,
        ..Default::default()
    };
    // SAFETY: `info` is a correctly-sized, initialised `HIGHCONTRASTW`;
    // `SPI_GETHIGHCONTRAST` only writes into it.
    let result = unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            info.cbSize,
            Some(&mut info as *mut HIGHCONTRASTW as *mut c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    result.is_ok() && (info.dwFlags.0 & HCF_HIGHCONTRASTON.0) != 0
}

/// Whether the user's "transparency effects" setting is on. When it is off,
/// Windows draws the backdrop opaque, so the app falls back to the solid theme
/// background. The value is the one Settings writes under
/// `HKCU\...\Themes\Personalize`; a missing or unreadable value means "on".
fn transparency_effects() -> bool {
    let mut value: u32 = 1;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: `value`/`size` are valid out-pointers for a DWORD read; the
    // subkey and value name are static, nul-terminated literals.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("EnableTransparency"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut c_void),
            Some(&mut size),
        )
    };
    status == ERROR_SUCCESS && value != 0
}

/// Applies `backdrop` to a top-level window, returning whether the material is
/// active. The dark variant follows `dark`; a failure (unsupported Windows,
/// high contrast, transparency off) leaves the window on its solid theme
/// background.
pub(crate) fn apply_backdrop(hwnd: Hwnd, backdrop: Backdrop, dark: bool) -> bool {
    let Some(kind) = system_backdrop(backdrop) else {
        return false;
    };
    // The immersive-dark flag has to be set before the material, so DWM picks
    // the dark variant of it.
    super::theme::set_titlebar_dark(hwnd, dark);

    // SAFETY: `hwnd` is a live top-level window; `kind` is a correctly-sized
    // `i32` that outlives the call, and `DWMWA_SYSTEMBACKDROP_TYPE` only reads
    // it.
    let result = unsafe {
        DwmSetWindowAttribute(
            raw_hwnd(hwnd),
            DWMWA_SYSTEMBACKDROP_TYPE,
            &kind as *const i32 as *const c_void,
            size_of::<i32>() as u32,
        )
    };
    let hresult = result.as_ref().err().map_or(0, |error| error.code().0);
    if !backdrop_supported(high_contrast(), transparency_effects(), hresult) {
        return false;
    }

    // The material only shows where the frame is extended into the client
    // (the top strip of the extended title bar). Extending the whole client
    // would make every GDI fill a zero-alpha "glass" pixel, so the strip is
    // extended separately by [`extend_frame`] and only its rectangle is
    // cleared to black for transparency.
    true
}

/// Extends the frame into the top `top` pixels of `hwnd`'s client area, so DWM
/// draws the caption buttons there and the backdrop material shows through that
/// strip. The rest of the client stays an ordinary opaque surface.
///
/// `top` is the caption height including the top frame, in device pixels.
/// Returns whether DWM accepted the margins; a failure leaves the client opaque
/// (and the caption buttons undrawn), which is the pre-#76 behaviour.
pub(crate) fn extend_frame(hwnd: Hwnd, top: i32) -> bool {
    let margins = MARGINS {
        cxLeftWidth: 0,
        cxRightWidth: 0,
        cyTopHeight: top,
        cyBottomHeight: 0,
    };
    // SAFETY: `hwnd` is a live top-level window and `margins` is a correctly
    // sized struct read by DWM for the duration of the call.
    unsafe { DwmExtendFrameIntoClientArea(raw_hwnd(hwnd), &margins) }.is_ok()
}

/// Colours the extended strip for the theme, returning whether both attributes
/// were accepted.
///
/// With the material active the caption colour is set to "none": otherwise a
/// user who shows the accent colour on title bars gets the accent painted over
/// the material (and never sees it). Without a material the strip takes the
/// theme background, so the caption buttons sit on the same surface as the
/// client.
pub(crate) fn apply_extended_colors(hwnd: Hwnd, theme: &Theme, backdrop_active: bool) -> bool {
    if high_contrast() {
        return false;
    }
    let caption = if backdrop_active {
        DWMWA_COLOR_NONE
    } else {
        theme.background.to_colorref()
    };
    let caption = set_color(hwnd, DWMWA_CAPTION_COLOR, caption);
    let border = set_color(hwnd, DWMWA_BORDER_COLOR, theme.border.to_colorref());
    caption && border
}

fn set_color(hwnd: Hwnd, attribute: DWMWINDOWATTRIBUTE, colorref: u32) -> bool {
    let value = COLORREF(colorref);
    // SAFETY: `hwnd` is live; `value` is a correctly-sized `COLORREF` that
    // outlives the call and is only read by DWM.
    unsafe {
        DwmSetWindowAttribute(
            raw_hwnd(hwnd),
            attribute,
            &value as *const COLORREF as *const c_void,
            size_of::<COLORREF>() as u32,
        )
    }
    .is_ok()
}

/// Paints the standard caption from `theme` (Windows 11 only), returning
/// whether every attribute was accepted. A failure leaves the system colours.
pub(crate) fn apply_caption_colors(hwnd: Hwnd, theme: &Theme) -> bool {
    if high_contrast() {
        return false;
    }
    let caption = COLORREF(theme.accent.to_colorref());
    let text = COLORREF(theme.text_on_accent.to_colorref());
    let border = COLORREF(theme.border.to_colorref());
    let corner = DWMWCP_ROUND;
    // SAFETY: `hwnd` is live; each attribute value is a correctly-sized struct
    // that outlives its call and is only read by DWM.
    unsafe {
        let caption = DwmSetWindowAttribute(
            raw_hwnd(hwnd),
            DWMWA_CAPTION_COLOR,
            &caption as *const COLORREF as *const c_void,
            size_of::<COLORREF>() as u32,
        );
        let text = DwmSetWindowAttribute(
            raw_hwnd(hwnd),
            DWMWA_TEXT_COLOR,
            &text as *const COLORREF as *const c_void,
            size_of::<COLORREF>() as u32,
        );
        let border = DwmSetWindowAttribute(
            raw_hwnd(hwnd),
            DWMWA_BORDER_COLOR,
            &border as *const COLORREF as *const c_void,
            size_of::<COLORREF>() as u32,
        );
        let corner = DwmSetWindowAttribute(
            raw_hwnd(hwnd),
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &corner as *const DWM_WINDOW_CORNER_PREFERENCE as *const c_void,
            size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
        );
        caption.is_ok() && text.is_ok() && border.is_ok() && corner.is_ok()
    }
}

#[cfg(test)]
mod tests {
    use windows::Win32::Foundation::E_INVALIDARG;

    use super::{Backdrop, backdrop_supported, system_backdrop};

    #[test]
    fn each_material_maps_to_its_dwm_kind() {
        assert_eq!(system_backdrop(Backdrop::None), None);
        assert!(system_backdrop(Backdrop::Mica).is_some());
        assert_ne!(
            system_backdrop(Backdrop::Mica),
            system_backdrop(Backdrop::MicaAlt)
        );
        assert_ne!(
            system_backdrop(Backdrop::MicaAlt),
            system_backdrop(Backdrop::Acrylic)
        );
    }

    #[test]
    fn fallback_follows_the_settings_and_the_hresult() {
        assert!(backdrop_supported(false, true, 0));
        assert!(
            !backdrop_supported(false, true, E_INVALIDARG.0),
            "a rejected DWM attribute must fall back"
        );
        assert!(
            !backdrop_supported(true, true, 0),
            "high contrast falls back"
        );
        assert!(
            !backdrop_supported(false, false, 0),
            "transparency off falls back"
        );
    }
}
