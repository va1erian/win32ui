//! Paint sessions, drawing primitives, clipping and the per-window back-buffer
//! cache.

use core::cell::RefCell;
use std::collections::HashMap;

use windows::Win32::Foundation::{COLORREF, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, AlphaBlend, BLENDFUNCTION, BeginPaint, BitBlt,
    CreateCompatibleBitmap, CreateCompatibleDC, DRAW_TEXT_FORMAT, DeleteDC, DeleteObject,
    DrawTextW, EndPaint, GetClipBox, HBITMAP, HBRUSH, HDC, HGDIOBJ, IntersectClipRect, LineTo,
    MoveToEx, PAINTSTRUCT, Polygon, SRCCOPY, SelectClipRgn, SelectObject, SetBkMode, SetTextColor,
    SetViewportOrgEx, TRANSPARENT,
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

/// Sets the mapping origin of `hdc`, so subsequent logical coordinates are
/// offset by `(x, y)` before hitting the device. Used to scroll a custom
/// widget's content inside its viewport-sized back buffer.
pub(crate) fn set_viewport_origin(hdc: HDC, x: i32, y: i32) {
    // SAFETY: `hdc` is live; the previous origin out-pointer is optional.
    unsafe {
        let _ = SetViewportOrgEx(hdc, x, y, None);
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

/// The bounding box of `hdc`'s current clip region, in device pixels (empty
/// when the region is empty or the DC is stale). A Direct2D DC render target
/// binds to this so its origin matches the DC's.
pub(crate) fn clip_box(hdc: HDC) -> Rect {
    let mut raw = RECT::default();
    // SAFETY: `raw` is a valid out-pointer; a stale handle returns `ERROR` (0)
    // and leaves `raw` untouched.
    let kind = unsafe { GetClipBox(hdc, &mut raw) };
    if kind.0 == 0 {
        return Rect::default();
    }
    Rect::new(raw.left, raw.top, raw.right, raw.bottom)
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
    // `DrawTextW` faults on an empty buffer: the slice is passed as a
    // (dangling) `PCWSTR` with a zero length, which user32 dereferences.
    if text.is_empty() {
        return 0;
    }
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
mod tests;
