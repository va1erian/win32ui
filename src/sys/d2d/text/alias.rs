//! Legacy GDI family names ("Segoe UI Semibold", "Arial Black").
//!
//! DirectWrite's collections group fonts by typographic family ("Segoe UI"
//! with a semibold weight), so a name that only GDI knows has to be mapped
//! back to a family plus a face through GDI interop.

use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FONT_STRETCH, DWRITE_FONT_STYLE, DWRITE_FONT_WEIGHT,
    DWRITE_INFORMATIONAL_STRING_WIN32_FAMILY_NAMES, IDWriteFactory, IDWriteFont,
    IDWriteLocalizedStrings,
};
use windows::Win32::Graphics::Gdi::{DEFAULT_CHARSET, FW_NORMAL, LOGFONTW};
use windows::core::BOOL;

/// The typographic family and face behind a GDI family name.
pub(super) struct GdiAlias {
    pub(super) family: String,
    pub(super) weight: DWRITE_FONT_WEIGHT,
    pub(super) style: DWRITE_FONT_STYLE,
    pub(super) stretch: DWRITE_FONT_STRETCH,
}

/// Maps the GDI family `name` to its typographic family and face, or `None`
/// when GDI has no font by that name. GDI interop silently substitutes a
/// similar font for unknown names, so the answer is checked against the found
/// font's own GDI family name.
pub(super) fn resolve(factory: &IDWriteFactory, name: &str) -> Option<GdiAlias> {
    let mut logfont = LOGFONTW {
        lfWeight: FW_NORMAL.0 as i32,
        lfCharSet: DEFAULT_CHARSET,
        ..Default::default()
    };
    let wide: Vec<u16> = name.encode_utf16().collect();
    // The face name is a NUL-terminated array, so one slot stays free.
    if wide.len() >= logfont.lfFaceName.len() {
        return None;
    }
    logfont.lfFaceName[..wide.len()].copy_from_slice(&wide);
    // SAFETY: `logfont` is a valid, initialised struct for the call.
    let font = unsafe {
        factory
            .GetGdiInterop()
            .ok()?
            .CreateFontFromLOGFONT(&logfont)
    }
    .ok()?;
    if !win32_family(&font)?.eq_ignore_ascii_case(name) {
        return None;
    }
    // SAFETY: getters on a live font.
    let (family, weight, style, stretch) = unsafe {
        (
            first_string(&font.GetFontFamily().ok()?.GetFamilyNames().ok()?)?,
            font.GetWeight(),
            font.GetStyle(),
            font.GetStretch(),
        )
    };
    Some(GdiAlias {
        family,
        weight,
        style,
        stretch,
    })
}

/// The font's GDI (name ID 1) family name.
fn win32_family(font: &IDWriteFont) -> Option<String> {
    let mut names = None;
    let mut exists = BOOL(0);
    // SAFETY: the out pointers are valid locals.
    unsafe {
        font.GetInformationalStrings(
            DWRITE_INFORMATIONAL_STRING_WIN32_FAMILY_NAMES,
            &mut names,
            &mut exists,
        )
    }
    .ok()?;
    first_string(&names?)
}

fn first_string(strings: &IDWriteLocalizedStrings) -> Option<String> {
    // SAFETY: the buffer is sized from `GetStringLength` plus the terminator.
    unsafe {
        let mut buffer = vec![0u16; strings.GetStringLength(0).ok()? as usize + 1];
        strings.GetString(0, &mut buffer).ok()?;
        buffer.pop();
        String::from_utf16(&buffer).ok()
    }
}
