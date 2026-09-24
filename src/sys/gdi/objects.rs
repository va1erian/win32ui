//! GDI object lifecycle: creation, selection and text measurement.

use core::ffi::c_void;
use core::ptr::{null_mut, slice_from_raw_parts_mut};

use windows::Win32::Foundation::{COLORREF, SIZE};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateDIBSection, CreateFontW, CreatePen,
    CreateSolidBrush, DIB_RGB_COLORS, DeleteObject, FONT_CHARSET, FONT_CLIP_PRECISION,
    FONT_OUTPUT_PRECISION, FONT_QUALITY, GetDC, GetStockObject, GetTextExtentPoint32W, HBITMAP,
    HBRUSH, HDC, HFONT, HGDIOBJ, HPEN, NULL_PEN, PS_SOLID, ReleaseDC, SelectObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    NONCLIENTMETRICSW, SPI_GETNONCLIENTMETRICS, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    SystemParametersInfoW,
};
use windows::core::PCWSTR;

use crate::color::Color;
use crate::error::{Error, Result};
use crate::geometry::Size;

use crate::sys::win32_error;

/// Creates a font for `family` with the given (negative) pixel height.
pub(crate) fn create_font(family: &str, height: i32, weight: i32) -> Result<HFONT> {
    let mut face = [0u16; 32];
    let wide: Vec<u16> = family.encode_utf16().collect();
    let count = wide.len().min(31);
    face[..count].copy_from_slice(&wide[..count]);

    // SAFETY: `face` is a nul-terminated wide buffer that outlives the call;
    // the remaining parameters are plain integers.
    let font = unsafe {
        CreateFontW(
            height,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            FONT_CHARSET(1),
            FONT_OUTPUT_PRECISION(0),
            FONT_CLIP_PRECISION(0),
            FONT_QUALITY(0),
            0,
            PCWSTR(face.as_ptr()),
        )
    };
    if font.0.is_null() {
        Err(Error::Gdi("font"))
    } else {
        Ok(font)
    }
}

/// Creates a solid brush.
pub(crate) fn solid_brush(color: Color) -> Result<HBRUSH> {
    // SAFETY: `CreateSolidBrush` takes a plain colour value.
    let brush = unsafe { CreateSolidBrush(COLORREF(color.to_colorref())) };
    if brush.0.is_null() {
        Err(Error::Gdi("brush"))
    } else {
        Ok(brush)
    }
}

/// Creates a cosmetic pen.
pub(crate) fn create_pen(color: Color, width: i32) -> Result<HPEN> {
    // SAFETY: `CreatePen` takes plain values.
    let pen = unsafe { CreatePen(PS_SOLID, width, COLORREF(color.to_colorref())) };
    if pen.0.is_null() {
        Err(Error::Gdi("pen"))
    } else {
        Ok(pen)
    }
}

/// The stock null pen, used to draw fills without an outline.
pub(crate) fn null_pen() -> HGDIOBJ {
    // SAFETY: `NULL_PEN` is a documented stock-object selector.
    unsafe { GetStockObject(NULL_PEN) }
}

/// Deletes any GDI object; errors (already-deleted handles) are ignored.
pub(crate) fn delete_object(object: HGDIOBJ) {
    // SAFETY: deleting a stale handle is a documented failure, not UB.
    unsafe {
        let _ = DeleteObject(object);
    }
}

/// A 32-bit top-down DIB section filled from RGBA pixels.
pub(crate) fn create_dib(width: i32, height: i32, rgba: &[u8]) -> Result<HBITMAP> {
    let mut info = BITMAPINFO::default();
    info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    info.bmiHeader.biWidth = width;
    info.bmiHeader.biHeight = -height; // negative: top-down rows
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB.0;

    let mut bits: *mut c_void = null_mut();
    // SAFETY: `info` and `bits` are valid for the call and output respectively.
    let bitmap = unsafe { CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0) }
        .map_err(win32_error)?;
    if bits.is_null() {
        // SAFETY: `bitmap` was just created and is still owned here.
        unsafe {
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
        }
        return Err(Error::Gdi("DIB section"));
    }

    let bytes = (width.max(0) as usize) * (height.max(0) as usize) * 4;
    if rgba.len() >= bytes {
        // SAFETY: `bits` points at `bytes` writable bytes owned by the DIB, and
        // `rgba` has at least that many readable bytes.
        unsafe {
            let destination = &mut *slice_from_raw_parts_mut(bits as *mut u8, bytes);
            for pixel in 0..bytes / 4 {
                let offset = pixel * 4;
                // Windows' 32-bpp DIB is BGRA; callers supply RGBA.
                destination[offset] = rgba[offset + 2];
                destination[offset + 1] = rgba[offset + 1];
                destination[offset + 2] = rgba[offset];
                destination[offset + 3] = rgba[offset + 3];
            }
        }
    }
    Ok(bitmap)
}

/// Selects a font and returns the previously selected object.
pub(crate) fn select_font(hdc: HDC, font: HFONT) -> HGDIOBJ {
    select_object(hdc, HGDIOBJ(font.0))
}

/// Selects a brush and returns the previously selected object.
pub(crate) fn select_brush(hdc: HDC, brush: HBRUSH) -> HGDIOBJ {
    select_object(hdc, HGDIOBJ(brush.0))
}

/// Selects a pen and returns the previously selected object.
pub(crate) fn select_pen(hdc: HDC, pen: HPEN) -> HGDIOBJ {
    select_object(hdc, HGDIOBJ(pen.0))
}

/// Selects a raw GDI object and returns the previously selected one.
pub(crate) fn select_object(hdc: HDC, object: HGDIOBJ) -> HGDIOBJ {
    // SAFETY: `hdc` is a live DC and `object` a live GDI object.
    unsafe { SelectObject(hdc, object) }
}

/// Measures `text` with the DC's current font.
pub(crate) fn text_extent(hdc: HDC, text: &str) -> Size {
    let wide: Vec<u16> = text.encode_utf16().collect();
    let mut size = SIZE::default();
    // SAFETY: both the input slice and the out-pointer are valid.
    unsafe {
        let _ = GetTextExtentPoint32W(hdc, &wide, &mut size);
    }
    Size::new(size.cx, size.cy)
}

/// Measures `text` in `font` using a temporary screen DC (used for layout
/// before any paint happens).
pub(crate) fn measure_text(font: HFONT, text: &str) -> Size {
    // SAFETY: a null window asks for the screen DC, which is always available.
    unsafe {
        let dc = GetDC(None);
        if dc.0.is_null() {
            return Size::default();
        }
        let old = SelectObject(dc, HGDIOBJ(font.0));
        let wide: Vec<u16> = text.encode_utf16().collect();
        let mut size = SIZE::default();
        let _ = GetTextExtentPoint32W(dc, &wide, &mut size);
        SelectObject(dc, old);
        let _ = ReleaseDC(None, dc);
        Size::new(size.cx, size.cy)
    }
}

/// Gets the system UI font face name and height (in points),
/// falling back to Segoe UI 9pt if the system call fails.
/// Returns (face_name, point_size).
pub(crate) fn system_ui_font_metrics(_dpi: u32) -> (String, f32) {
    let mut metrics: NONCLIENTMETRICSW = unsafe { std::mem::zeroed() };
    metrics.cbSize = std::mem::size_of::<NONCLIENTMETRICSW>() as u32;

    // SAFETY: `metrics` is fully initialized and the call is safe.
    let success = unsafe {
        SystemParametersInfoW(
            SPI_GETNONCLIENTMETRICS,
            metrics.cbSize,
            Some(&mut metrics as *mut _ as *mut c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };

    if success.is_ok() {
        // Extract the message font (used for UI text)
        let face_len = metrics
            .lfMessageFont
            .lfFaceName
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(32);
        let face =
            String::from_utf16_lossy(&metrics.lfMessageFont.lfFaceName[..face_len]).to_string();

        // Convert height from logical units to points
        // The lfHeight is negative and in device units; convert to points
        // using standard 96 DPI as the baseline
        let height_logical = metrics.lfMessageFont.lfHeight.abs() as f32;
        let point_size = (height_logical * 72.0) / 96.0;

        if !face.is_empty() && point_size > 0.0 {
            return (face, point_size);
        }
    }

    // Fallback to Segoe UI 9pt
    ("Segoe UI".to_string(), 9.0)
}
