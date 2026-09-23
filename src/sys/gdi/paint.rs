//! Paint sessions, drawing primitives, clipping and the per-window back-buffer
//! cache.

use core::cell::RefCell;
use std::collections::HashMap;

use windows::Win32::Foundation::{COLORREF, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, AlphaBlend, BLENDFUNCTION, BeginPaint, BitBlt,
    CreateCompatibleBitmap, CreateCompatibleDC, DRAW_TEXT_FORMAT, DeleteDC, DeleteObject,
    DrawTextW, EndPaint, HBITMAP, HBRUSH, HDC, HGDIOBJ, IntersectClipRect, LineTo, MoveToEx,
    PAINTSTRUCT, Polygon, SRCCOPY, SelectClipRgn, SelectObject, SetBkMode, SetTextColor,
    TRANSPARENT,
};

use crate::color::Color;
use crate::geometry::{Point, Rect, Size};
use crate::hwnd::Hwnd;

use crate::sys::raw_hwnd;

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

/// An off-screen buffer kept alive for a window between paints.
struct BackBuffer {
    memory_dc: HDC,
    bitmap: HBITMAP,
    old_bitmap: HGDIOBJ,
    width: i32,
    height: i32,
    dpi: u32,
}

thread_local! {
    /// One back buffer per window, reused across paints and grown in place when
    /// the client area grows. Released on `WM_NCDESTROY` (see `sys::dispatch`),
    /// so a dead `HWND` never keeps a thread-local allocation alive.
    static BACK_BUFFERS: RefCell<HashMap<isize, BackBuffer>> = RefCell::new(HashMap::new());
}

/// Returns a back buffer for `hwnd` at least `width`×`height`, compatible with
/// `dc`, creating or growing it as needed. The buffer stays cached for the
/// window and is released by [`release_back_buffer`].
pub(crate) fn acquire_back_buffer(
    hwnd: Hwnd,
    dc: HDC,
    width: i32,
    height: i32,
    dpi: u32,
) -> Option<HDC> {
    let width = width.max(1);
    let height = height.max(1);
    BACK_BUFFERS.with(|cell| {
        let mut buffers = cell.borrow_mut();
        let key = hwnd.raw() as isize;
        if let Some(buffer) = buffers.get(&key)
            && buffer.width >= width
            && buffer.height >= height
            && buffer.dpi == dpi
        {
            return Some(buffer.memory_dc);
        }
        if let Some(stale) = buffers.remove(&key) {
            destroy(stale);
        }
        let buffer = create(dc, width, height, dpi)?;
        let memory_dc = buffer.memory_dc;
        buffers.insert(key, buffer);
        Some(memory_dc)
    })
}

/// Releases and deletes the cached back buffer for `hwnd`, if any.
pub(crate) fn release_back_buffer(hwnd: Hwnd) {
    let buffer = BACK_BUFFERS.with(|cell| cell.borrow_mut().remove(&(hwnd.raw() as isize)));
    if let Some(buffer) = buffer {
        destroy(buffer);
    }
}

fn create(dc: HDC, width: i32, height: i32, dpi: u32) -> Option<BackBuffer> {
    // SAFETY: `dc` is a live DC; the returned handles are owned by `BackBuffer`
    // and released in `destroy`.
    unsafe {
        let memory_dc = CreateCompatibleDC(Some(dc));
        if memory_dc.0.is_null() {
            return None;
        }
        let bitmap = CreateCompatibleBitmap(dc, width, height);
        if bitmap.0.is_null() {
            let _ = DeleteDC(memory_dc);
            return None;
        }
        let old_bitmap = SelectObject(memory_dc, HGDIOBJ(bitmap.0));
        Some(BackBuffer {
            memory_dc,
            bitmap,
            old_bitmap,
            width,
            height,
            dpi,
        })
    }
}

fn destroy(buffer: BackBuffer) {
    // SAFETY: the handles came from `create` and are still live; restoring the
    // original bitmap before deleting keeps the DC consistent.
    unsafe {
        SelectObject(buffer.memory_dc, buffer.old_bitmap);
        let _ = DeleteObject(HGDIOBJ(buffer.bitmap.0));
        let _ = DeleteDC(buffer.memory_dc);
    }
}

/// Removes any clip region from `hdc`.
pub(crate) fn reset_clip(hdc: HDC) {
    // SAFETY: a null region selects the DC's default (unclipped) region.
    unsafe {
        let _ = SelectClipRgn(hdc, None);
    }
}

/// Clips `hdc` to `rect`.
pub(crate) fn clip_rect(hdc: HDC, rect: Rect) {
    // SAFETY: `rect` is plain geometry.
    unsafe {
        let _ = IntersectClipRect(hdc, rect.left, rect.top, rect.right, rect.bottom);
    }
}

/// Copies `rect` from `source` to `dest` at the same coordinates.
pub(crate) fn blit_rect(dest: HDC, source: HDC, rect: Rect) {
    if rect.is_empty() {
        return;
    }
    // SAFETY: both DCs are live and the rectangle lies inside both.
    unsafe {
        let _ = BitBlt(
            dest,
            rect.left,
            rect.top,
            rect.width(),
            rect.height(),
            Some(source),
            rect.left,
            rect.top,
            SRCCOPY,
        );
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

/// Draws a straight line from `from` to `to` with the DC's current pen.
pub(crate) fn line(hdc: HDC, from: Point, to: Point) {
    // SAFETY: plain coordinates; the current pen is selected by the caller.
    unsafe {
        let _ = MoveToEx(hdc, from.x, from.y, None);
        let _ = LineTo(hdc, to.x, to.y);
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

/// Blits `bitmap` at `target.left/top` (no scaling), honouring its alpha
/// channel so transparent pixels blend onto whatever is already painted
/// instead of overwriting it with black (`SRCCOPY` would ignore alpha).
pub(crate) fn draw_bitmap(hdc: HDC, bitmap: HBITMAP, source: Size, target: Rect) {
    let width = source.width.min(target.width());
    let height = source.height.min(target.height());
    if width <= 0 || height <= 0 {
        return;
    }
    // SAFETY: all handles are live; `source`/`target` are plain geometry and
    // `blend` is a plain value struct. `AC_SRC_ALPHA` asks `AlphaBlend` to use
    // the per-pixel alpha of the 32-bpp DIB section.
    unsafe {
        let memory_dc = CreateCompatibleDC(Some(hdc));
        let old = SelectObject(memory_dc, HGDIOBJ(bitmap.0));
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let _ = AlphaBlend(
            hdc,
            target.left,
            target.top,
            width,
            height,
            memory_dc,
            0,
            0,
            width,
            height,
            blend,
        );
        SelectObject(memory_dc, old);
        let _ = DeleteDC(memory_dc);
    }
}

#[cfg(test)]
mod tests {
    use windows::Win32::Graphics::Gdi::{GetDC, ReleaseDC};

    use super::{acquire_back_buffer, release_back_buffer};
    use crate::hwnd::Hwnd;

    /// A screen DC is enough to create compatible buffers, so these tests do
    /// not need a window or the message loop.
    fn screen_dc() -> windows::Win32::Graphics::Gdi::HDC {
        // SAFETY: a null window asks for the screen DC.
        unsafe { GetDC(None) }
    }

    fn release_dc(dc: windows::Win32::Graphics::Gdi::HDC) {
        // SAFETY: `dc` came from the matching `screen_dc`.
        unsafe {
            let _ = ReleaseDC(None, dc);
        }
    }

    #[test]
    fn back_buffer_is_reused_then_grown() {
        let dc = screen_dc();
        if dc.0.is_null() {
            return;
        }
        let hwnd = Hwnd::from_raw(0x1234);
        let first = acquire_back_buffer(hwnd, dc, 100, 100, 96).expect("buffer");
        let again = acquire_back_buffer(hwnd, dc, 100, 100, 96).expect("reused");
        assert_eq!(first.0, again.0, "the buffer was not reused");
        let grown = acquire_back_buffer(hwnd, dc, 200, 100, 96).expect("grown");
        assert_ne!(first.0, grown.0, "the buffer was not grown");
        release_back_buffer(hwnd);
        let fresh = acquire_back_buffer(hwnd, dc, 100, 100, 96).expect("fresh");
        assert_ne!(grown.0, fresh.0, "the released buffer was reused");
        release_back_buffer(hwnd);
        release_dc(dc);
    }

    #[test]
    fn dpi_change_recreates() {
        let dc = screen_dc();
        if dc.0.is_null() {
            return;
        }
        let hwnd = Hwnd::from_raw(0x5678);
        let normal = acquire_back_buffer(hwnd, dc, 100, 100, 96).expect("buffer");
        let scaled = acquire_back_buffer(hwnd, dc, 100, 100, 144).expect("scaled");
        assert_ne!(normal.0, scaled.0, "the buffer survived a DPI change");
        release_back_buffer(hwnd);
        release_dc(dc);
    }

    #[test]
    fn draw_bitmap_honours_per_pixel_alpha() {
        use windows::Win32::Graphics::Gdi::{GetPixel, HGDIOBJ};

        use crate::color::Color;
        use crate::geometry::{Rect, Size};
        use crate::sys::gdi::{create_dib, delete_object, solid_brush};

        let dc = screen_dc();
        if dc.0.is_null() {
            return;
        }
        let hwnd = Hwnd::from_raw(0x9ABC);
        let buffer = acquire_back_buffer(hwnd, dc, 8, 8, 96).expect("buffer");

        // Paint the buffer an opaque background colour.
        let background = Color::rgb(0x11, 0x22, 0x33);
        let brush = solid_brush(background).expect("brush");
        super::fill_rect(buffer, Rect::new(0, 0, 8, 8), brush);

        // A fully transparent red pixel. `AlphaBlend` must leave the background
        // untouched; the old `SRCCOPY` blit stamped red over it.
        let bitmap = create_dib(1, 1, &[0xFF, 0x00, 0x00, 0x00]).expect("dib");
        super::draw_bitmap(buffer, bitmap, Size::new(1, 1), Rect::new(0, 0, 1, 1));

        // SAFETY: `buffer` is live with its compatible bitmap selected.
        let pixel = unsafe { GetPixel(buffer, 0, 0) };
        assert_eq!(
            Color::from_colorref(pixel.0),
            background,
            "a transparent pixel was not left as the background"
        );

        delete_object(HGDIOBJ(bitmap.0));
        release_back_buffer(hwnd);
        release_dc(dc);
    }
}
