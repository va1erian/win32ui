//! Reads the live system theme: light/dark app mode, the accent colour, and
//! high-contrast system colours.
//!
//! All `unsafe` in this crate lives under `sys`; every block below carries a
//! `// SAFETY:` note. Only documented registry, DWM and `GetSysColor` APIs are
//! used — no undocumented `uxtheme` ordinals.

use core::ffi::c_void;

use windows::Win32::Foundation::{BOOL, ERROR_SUCCESS};
use windows::Win32::Graphics::Dwm::DwmGetColorizationColor;
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::UI::WindowsAndMessaging::{
    COLOR_BTNFACE, COLOR_GRAYTEXT, COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT, COLOR_HOTLIGHT,
    COLOR_WINDOW, COLOR_WINDOWFRAME, COLOR_WINDOWTEXT, GetSysColor, SYS_COLOR_INDEX,
};
use windows::core::w;

use crate::color::Color;

pub(crate) use super::dwm::high_contrast;

/// The `GetSysColor` values a high-contrast [`crate::Theme`] is built from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HighContrastColors {
    pub(crate) window: Color,
    pub(crate) window_text: Color,
    pub(crate) window_frame: Color,
    pub(crate) btn_face: Color,
    pub(crate) gray_text: Color,
    pub(crate) highlight: Color,
    pub(crate) highlight_text: Color,
    pub(crate) hotlight: Color,
}

fn sys_color(index: SYS_COLOR_INDEX) -> Color {
    // SAFETY: `GetSysColor` takes a colour index and returns a `COLORREF`; it
    // has no pointer or lifetime contract.
    let colorref = unsafe { GetSysColor(index) };
    Color::from_colorref(colorref)
}

/// Whether Windows is in high-contrast mode
/// (`SystemParametersInfoW(SPI_GETHIGHCONTRAST)`), and if so, the system
/// colours ([`GetSysColor`]) to theme from.
pub(crate) fn high_contrast_colors() -> Option<HighContrastColors> {
    if !high_contrast() {
        return None;
    }
    Some(HighContrastColors {
        window: sys_color(COLOR_WINDOW),
        window_text: sys_color(COLOR_WINDOWTEXT),
        window_frame: sys_color(COLOR_WINDOWFRAME),
        btn_face: sys_color(COLOR_BTNFACE),
        gray_text: sys_color(COLOR_GRAYTEXT),
        highlight: sys_color(COLOR_HIGHLIGHT),
        highlight_text: sys_color(COLOR_HIGHLIGHTTEXT),
        hotlight: sys_color(COLOR_HOTLIGHT),
    })
}

/// Whether Windows apps use the light app mode (`AppsUseLightTheme`), read
/// from `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize`.
/// A missing or unreadable value means light, matching a fresh Windows
/// install's default.
pub(crate) fn apps_use_light_theme() -> bool {
    let mut value: u32 = 1;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: `value`/`size` are valid out-pointers for a DWORD read; the
    // subkey and value name are static, nul-terminated literals.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut c_void),
            Some(&mut size),
        )
    };
    status != ERROR_SUCCESS || value != 0
}

/// The DWM colorization colour (Settings > Personalization > Colors' accent),
/// as an opaque colour.
///
/// `DwmGetColorizationColor` is used rather than the WinRT
/// `UISettings::GetColorValue`: it needs no WinRT activation, matches the
/// colour Explorer's own chrome (and a themed caption, see
/// `apply_caption_colors`) is drawn from, and stays inside the plain Win32
/// surface the rest of this module uses.
pub(crate) fn accent_color() -> Option<Color> {
    let mut colorization: u32 = 0;
    let mut opaque_blend = BOOL(0);
    // SAFETY: both out-parameters are valid, correctly-sized locals that
    // outlive the call and are only written by it.
    let result = unsafe { DwmGetColorizationColor(&mut colorization, &mut opaque_blend) };
    if result.is_err() {
        return None;
    }
    // `DwmGetColorizationColor` returns 0xAARRGGBB; the alpha is a blend
    // weight (how much shows through glass), not transparency, so it is
    // dropped for an opaque colour.
    let [_alpha, r, g, b] = colorization.to_be_bytes();
    Some(Color::rgb(r, g, b))
}
