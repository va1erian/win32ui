#![forbid(unsafe_code)]

//! A double-buffered paint session and the [`Canvas`] it draws through.
//!
//! Begin with [`Paint::begin`]; all drawing goes to an off-screen buffer that
//! is blitted to the window when the `Paint` is dropped, so there is no
//! flicker even when repainting the whole client area.

use windows::Win32::Graphics::Gdi::{HDC, PAINTSTRUCT};

use crate::color::Color;
use crate::geometry::{Rect, Size};
use crate::hwnd::Hwnd;
use crate::sys;

use super::{Bitmap, Brush, Font, Pen};

// DrawText flags, mirrored here so callers never see the `windows` crate.
const DT_CENTER: u32 = 0x0000_0001;
const DT_RIGHT: u32 = 0x0000_0002;
const DT_VCENTER: u32 = 0x0000_0004;
const DT_WORDBREAK: u32 = 0x0000_0010;
const DT_SINGLELINE: u32 = 0x0000_0020;
const DT_NOPREFIX: u32 = 0x0000_0800;
const DT_END_ELLIPSIS: u32 = 0x0000_8000;

/// How [`Canvas::draw_text`] lays text out.
#[derive(Clone, Copy, Debug, Default)]
pub struct TextFormat(u32);

impl TextFormat {
    /// Left-aligned (the default).
    pub const fn left() -> TextFormat {
        TextFormat(0)
    }

    /// Horizontally centred.
    pub const fn center(self) -> TextFormat {
        TextFormat((self.0 & !DT_RIGHT) | DT_CENTER)
    }

    /// Right-aligned.
    pub const fn right(self) -> TextFormat {
        TextFormat((self.0 & !DT_CENTER) | DT_RIGHT)
    }

    /// Vertically centred within the rectangle.
    pub const fn vcenter(self) -> TextFormat {
        TextFormat(self.0 | DT_VCENTER)
    }

    /// Keep the text on a single line.
    pub const fn single_line(self) -> TextFormat {
        TextFormat(self.0 | DT_SINGLELINE)
    }

    /// Wrap on spaces when the line is too long.
    pub const fn word_wrap(self) -> TextFormat {
        TextFormat(self.0 | DT_WORDBREAK)
    }

    /// Replace a trailing overflow with an ellipsis.
    pub const fn end_ellipsis(self) -> TextFormat {
        TextFormat(self.0 | DT_END_ELLIPSIS)
    }

    /// Treat `&` literally instead of as an accelerator marker.
    pub const fn no_prefix(self) -> TextFormat {
        TextFormat(self.0 | DT_NOPREFIX)
    }

    /// The raw `DT_*` bits.
    pub const fn bits(self) -> u32 {
        self.0
    }
}

/// A drawing surface. Obtain one from [`Paint::canvas`]; it is valid only for
/// the duration of the paint.
pub struct Canvas {
    dc: HDC,
}

impl Canvas {
    pub(crate) fn new(dc: HDC) -> Canvas {
        Canvas { dc }
    }

    /// Fills `rect` with `color`.
    pub fn fill_rect(&self, rect: Rect, color: Color) {
        if let Ok(brush) = Brush::solid(color) {
            self.fill_rect_brush(rect, &brush);
        }
    }

    /// Fills `rect` with a pre-made brush (avoids recreating one per call when
    /// painting many separators).
    pub fn fill_rect_brush(&self, rect: Rect, brush: &Brush) {
        if !rect.is_empty() {
            sys::gdi::fill_rect(self.dc, rect, brush.raw());
        }
    }

    /// Draws a 1-pixel outline just inside `rect`.
    pub fn outline(&self, rect: Rect, color: Color) {
        if rect.is_empty() {
            return;
        }
        self.fill_rect(
            Rect::new(rect.left, rect.top, rect.right, rect.top + 1),
            color,
        );
        self.fill_rect(
            Rect::new(rect.left, rect.bottom - 1, rect.right, rect.bottom),
            color,
        );
        self.fill_rect(
            Rect::new(rect.left, rect.top, rect.left + 1, rect.bottom),
            color,
        );
        self.fill_rect(
            Rect::new(rect.right - 1, rect.top, rect.right, rect.bottom),
            color,
        );
    }

    /// Draws a filled rounded rectangle with an optional outline.
    pub fn round_rect(&self, rect: Rect, radius: i32, fill: Color, border: Option<Color>) {
        if rect.is_empty() {
            return;
        }
        let Ok(brush) = Brush::solid(fill) else {
            return;
        };
        let old_brush = sys::gdi::select_brush(self.dc, brush.raw());
        let pen = border.and_then(|color| Pen::new(color, 1).ok());
        let old_pen = match &pen {
            Some(pen) => sys::gdi::select_pen(self.dc, pen.raw()),
            None => sys::gdi::select_object(self.dc, sys::gdi::null_pen()),
        };
        sys::gdi::round_rect(self.dc, rect, radius);
        sys::gdi::select_object(self.dc, old_pen);
        sys::gdi::select_object(self.dc, old_brush);
    }

    /// Fills a triangle inside `rect` (used for sort arrows).
    pub fn triangle(&self, rect: Rect, color: Color, pointing_up: bool) {
        if rect.is_empty() {
            return;
        }
        let Ok(brush) = Brush::solid(color) else {
            return;
        };
        let previous_brush = sys::gdi::select_brush(self.dc, brush.raw());
        let previous_pen = sys::gdi::select_object(self.dc, sys::gdi::null_pen());
        sys::gdi::triangle(self.dc, rect, pointing_up);
        sys::gdi::select_object(self.dc, previous_pen);
        sys::gdi::select_object(self.dc, previous_brush);
    }

    /// Draws `text` inside `rect`.
    pub fn draw_text(&self, rect: Rect, text: &str, color: Color, format: TextFormat) -> i32 {
        sys::gdi::draw_text(self.dc, rect, text, color, format.bits())
    }

    /// Measures `text` using the currently selected font.
    pub fn text_size(&self, text: &str) -> Size {
        sys::gdi::text_extent(self.dc, text)
    }

    /// Blits `bitmap` so its top-left is at `rect.left/top`.
    pub fn draw_bitmap(&self, bitmap: &Bitmap, rect: Rect) {
        sys::gdi::draw_bitmap(self.dc, bitmap.raw(), bitmap.size(), rect);
    }

    /// Selects `font` while running `draw`, restoring the previous font after.
    pub fn with_font<R>(&self, font: &Font, draw: impl FnOnce(&Canvas) -> R) -> R {
        let previous = sys::gdi::select_font(self.dc, font.raw());
        let result = draw(self);
        sys::gdi::select_object(self.dc, previous);
        result
    }
}

/// A double-buffered paint session. Drop it to flush the buffer to the screen
/// and end the paint.
pub struct Paint {
    hwnd: Hwnd,
    ps: PAINTSTRUCT,
    memory_dc: HDC,
    bitmap: windows::Win32::Graphics::Gdi::HBITMAP,
    old_bitmap: windows::Win32::Graphics::Gdi::HGDIOBJ,
    canvas: Canvas,
    client: Rect,
}

impl Paint {
    /// Begins painting `hwnd`, or returns `None` if the window is not paintable.
    pub fn begin(hwnd: Hwnd) -> Option<Paint> {
        let mut ps = PAINTSTRUCT::default();
        let dc = sys::gdi::begin_paint(hwnd, &mut ps);
        if dc.0.is_null() {
            sys::gdi::end_paint(hwnd, &ps);
            return None;
        }
        let client = sys::window::client_rect(hwnd);
        let (memory_dc, bitmap, old_bitmap) =
            sys::gdi::create_back_buffer(dc, client.width(), client.height());
        Some(Paint {
            hwnd,
            ps,
            memory_dc,
            bitmap,
            old_bitmap,
            canvas: Canvas::new(memory_dc),
            client,
        })
    }

    /// The drawing surface.
    pub fn canvas(&self) -> &Canvas {
        &self.canvas
    }

    /// The client rectangle being painted.
    pub fn client_rect(&self) -> Rect {
        self.client
    }
}

impl Drop for Paint {
    fn drop(&mut self) {
        sys::gdi::blit(
            self.ps.hdc,
            self.memory_dc,
            self.client.width(),
            self.client.height(),
        );
        sys::gdi::destroy_back_buffer(self.memory_dc, self.bitmap, self.old_bitmap);
        sys::gdi::end_paint(self.hwnd, &self.ps);
    }
}
