//! GDI primitives: fonts, brushes, pens, DIB sections, off-screen buffers and
//! text.

use core::ffi::c_void;
use core::ptr::{null_mut, slice_from_raw_parts_mut};

use windows::Win32::Foundation::{COLORREF, HANDLE, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BeginPaint, BitBlt, CreateCompatibleBitmap,
    CreateCompatibleDC, CreateDIBSection, CreateFontW, CreatePen, CreateSolidBrush, DIB_RGB_COLORS,
    DRAW_TEXT_FORMAT, DeleteDC, DeleteObject, DrawTextW, EndPaint, GetDC, GetStockObject,
    GetTextExtentPoint32W, HBITMAP, HBRUSH, HDC, HFONT, HGDIOBJ, HPEN, NULL_PEN, PAINTSTRUCT,
    PS_SOLID, Polygon, ReleaseDC, SRCCOPY, SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows::core::PCWSTR;

use crate::color::Color;
use crate::error::{Error, Result};
use crate::geometry::{Rect, Size};
use crate::hwnd::Hwnd;

use super::raw_hwnd;

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
            1,
            0,
            0,
            0,
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
    let bitmap = unsafe {
        CreateDIBSection(
            HDC::default(),
            &info,
            DIB_RGB_COLORS,
            &mut bits,
            HANDLE::default(),
            0,
        )
    }?;
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

/// Begins painting into `ps` and returns the paint DC.
pub(crate) fn begin_paint(hwnd: Hwnd, ps: *mut PAINTSTRUCT) -> HDC {
    // SAFETY: `ps` is a valid out-pointer and `hwnd` a live window.
    unsafe { BeginPaint(raw_hwnd(hwnd), ps) }
}

/// Ends the paint session started by [`begin_paint`].
pub(crate) fn end_paint(hwnd: Hwnd, ps: *const PAINTSTRUCT) {
    // SAFETY: `ps` came from the matching `begin_paint`.
    unsafe {
        let _ = EndPaint(raw_hwnd(hwnd), ps);
    }
}

/// Creates an off-screen buffer compatible with `dc`.
pub(crate) fn create_back_buffer(dc: HDC, width: i32, height: i32) -> (HDC, HBITMAP, HGDIOBJ) {
    // SAFETY: `dc` is a live DC; the returned handles are tracked by the
    // caller and released in `destroy_back_buffer`.
    unsafe {
        let memory_dc = CreateCompatibleDC(dc);
        let bitmap = CreateCompatibleBitmap(dc, width.max(1), height.max(1));
        let old = SelectObject(memory_dc, HGDIOBJ(bitmap.0));
        (memory_dc, bitmap, old)
    }
}

/// Releases an off-screen buffer created by [`create_back_buffer`].
pub(crate) fn destroy_back_buffer(memory_dc: HDC, bitmap: HBITMAP, old: HGDIOBJ) {
    // SAFETY: all three handles came from `create_back_buffer` and are still
    // live; restoring `old` before deleting keeps the DC consistent.
    unsafe {
        SelectObject(memory_dc, old);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(memory_dc);
    }
}

/// Copies `width`×`height` pixels from `source` to `dest`.
pub(crate) fn blit(dest: HDC, source: HDC, width: i32, height: i32) {
    // SAFETY: both DCs are live and the rectangle is clipped by the caller.
    unsafe {
        let _ = BitBlt(dest, 0, 0, width, height, source, 0, 0, SRCCOPY);
    }
}

/// Fills a rectangle with a brush.
pub(crate) fn fill_rect(hdc: HDC, rect: Rect, brush: HBRUSH) {
    // SAFETY: `rect` is converted to a valid RECT and `brush` is live.
    unsafe {
        let raw = RECT {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        };
        windows::Win32::Graphics::Gdi::FillRect(hdc, &raw, brush);
    }
}

/// Draws a rounded rectangle using the DC's current pen and brush.
pub(crate) fn round_rect(hdc: HDC, rect: Rect, radius: i32) {
    // SAFETY: plain geometry; current pen/brush are selected by the caller.
    unsafe {
        let _ = windows::Win32::Graphics::Gdi::RoundRect(
            hdc,
            rect.left,
            rect.top,
            rect.right,
            rect.bottom,
            radius.max(1),
            radius.max(1),
        );
    }
}

/// Fills a triangle inside `rect`, using the DC's current brush (the caller
/// selects a null pen first).
pub(crate) fn triangle(hdc: HDC, rect: Rect, pointing_up: bool) {
    let middle = (rect.left + rect.right) / 2;
    let points = if pointing_up {
        [
            POINT {
                x: middle,
                y: rect.top,
            },
            POINT {
                x: rect.left,
                y: rect.bottom,
            },
            POINT {
                x: rect.right,
                y: rect.bottom,
            },
        ]
    } else {
        [
            POINT {
                x: rect.left,
                y: rect.top,
            },
            POINT {
                x: rect.right,
                y: rect.top,
            },
            POINT {
                x: middle,
                y: rect.bottom,
            },
        ]
    };
    // SAFETY: `points` is a valid slice for the call.
    unsafe {
        let _ = Polygon(hdc, &points);
    }
}

/// Draws text inside `rect`, returning the drawn height.
pub(crate) fn draw_text(hdc: HDC, rect: Rect, text: &str, color: Color, format: u32) -> i32 {
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    let mut raw = RECT {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
    };
    // SAFETY: `wide` and `raw` are valid for the call; colour/mode are values.
    unsafe {
        SetTextColor(hdc, COLORREF(color.to_colorref()));
        SetBkMode(hdc, TRANSPARENT);
        DrawTextW(hdc, &mut wide, &mut raw, DRAW_TEXT_FORMAT(format))
    }
}

/// Blits `bitmap` at `target.left/top` (no scaling).
pub(crate) fn draw_bitmap(hdc: HDC, bitmap: HBITMAP, source: Size, target: Rect) {
    // SAFETY: all handles are live; `source`/`target` are plain geometry.
    unsafe {
        let memory_dc = CreateCompatibleDC(hdc);
        let old = SelectObject(memory_dc, HGDIOBJ(bitmap.0));
        let _ = BitBlt(
            hdc,
            target.left,
            target.top,
            source.width.min(target.width()),
            source.height.min(target.height()),
            memory_dc,
            0,
            0,
            SRCCOPY,
        );
        SelectObject(memory_dc, old);
        let _ = DeleteDC(memory_dc);
    }
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
        let dc = GetDC(HWND::default());
        if dc.0.is_null() {
            return Size::default();
        }
        let old = SelectObject(dc, HGDIOBJ(font.0));
        let wide: Vec<u16> = text.encode_utf16().collect();
        let mut size = SIZE::default();
        let _ = GetTextExtentPoint32W(dc, &wide, &mut size);
        SelectObject(dc, old);
        let _ = ReleaseDC(HWND::default(), dc);
        Size::new(size.cx, size.cy)
    }
}
