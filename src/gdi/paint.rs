#![forbid(unsafe_code)]

//! A double-buffered paint session and the [`Canvas`] it draws through.
//!
//! Begin with [`Paint::begin`]; all drawing goes to an off-screen buffer that
//! is blitted to the window when the `Paint` is dropped, so there is no
//! flicker even when repainting the whole client area.

use windows::Win32::Graphics::Gdi::{HDC, PAINTSTRUCT};

use crate::color::Color;
use crate::d2d::{DcCanvas, PathBuilder, PointF, RectF, Stroke};
use crate::geometry::{Point, Rect, Size};
use crate::hwnd::Hwnd;
use crate::sys;

use super::{Bitmap, Brush, Font, cache};

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
    paint: Rect,
    /// A scroll offset the surface is being drawn through (device pixels), so
    /// `paint_rect` can report the dirty band in the content's coordinates.
    scroll: std::cell::Cell<i32>,
}

impl Canvas {
    /// A canvas over `dc` with no known repaint rectangle.
    pub(crate) fn new(dc: HDC) -> Canvas {
        Canvas {
            dc,
            paint: Rect::default(),
            scroll: std::cell::Cell::new(0),
        }
    }

    /// A canvas over the raw device context `dc` (a `WM_PAINT` `wparam`).
    pub(crate) fn from_raw_dc(dc: usize) -> Canvas {
        Canvas::new(HDC(dc as *mut core::ffi::c_void))
    }

    /// A canvas over `dc`, reporting `paint` from [`Canvas::paint_rect`].
    pub(crate) fn with_paint_rect(dc: HDC, paint: Rect) -> Canvas {
        Canvas {
            dc,
            paint,
            scroll: std::cell::Cell::new(0),
        }
    }

    /// Records the scroll offset this canvas is drawn through, in device
    /// pixels, so `paint_rect` reports content coordinates.
    pub(crate) fn set_scroll(&self, offset: i32) {
        self.scroll.set(offset);
    }

    /// The rectangle being repainted: the `PAINTSTRUCT.rcPaint` of the paint
    /// that created this canvas, or an empty rectangle for a canvas obtained
    /// from [`Canvas::new`] outside a [`Paint`].
    ///
    /// In a scroll host the widget draws its document shifted up by the scroll
    /// offset, so this is returned in the widget's own (content) coordinates.
    pub fn paint_rect(&self) -> Rect {
        self.paint.offset(0, self.scroll.get())
    }

    /// Fills `rect` with `color`.
    pub fn fill_rect(&self, rect: Rect, color: Color) {
        if rect.is_empty() {
            return;
        }
        if let Some(brush) = cache::solid_brush(color) {
            sys::gdi::fill_rect(self.dc, rect, brush);
        }
    }

    /// Fills `rect` with a pre-made brush (avoids recreating one per call when
    /// painting many separators).
    pub fn fill_rect_brush(&self, rect: Rect, brush: &Brush) {
        if !rect.is_empty() {
            sys::gdi::fill_rect(self.dc, rect, brush.raw());
        }
    }

    /// Fills `rect` with the colour DWM treats as transparent inside an
    /// extended frame (pure black), so the window's
    /// [`Backdrop`](crate::Backdrop) material shows through.
    ///
    /// Only meaningful inside the extended title bar's caption strip: the rest
    /// of the client is an ordinary opaque surface and must keep the solid
    /// [`Theme::background`](crate::Theme::background). The window erases the
    /// strip itself; call this only with a rectangle inside that strip.
    pub fn clear_to_backdrop(&self, rect: Rect) {
        self.fill_rect(rect, Color::rgb(0, 0, 0));
    }

    /// A Direct2D canvas over this device context for anti-aliased shapes, or
    /// `None` when Direct2D is unavailable. Draw and [`DcCanvas::end_draw`] it
    /// before drawing with GDI on the same context.
    ///
    /// Coordinates are device pixels relative to the DC's origin, so they line
    /// up with this canvas's GDI drawing. The render target is bound from the
    /// origin to the clip box's far corner, so a partially clipped DC never
    /// shifts the shapes.
    pub fn d2d(&self) -> Option<DcCanvas> {
        let clip = sys::gdi::clip_box(self.dc);
        let rect = Rect::new(0, 0, clip.right.max(1), clip.bottom.max(1));
        DcCanvas::new(self.dc.0 as isize, rect).ok()
    }

    /// Draws a straight line from `from` to `to`, `width` pixels wide.
    pub fn line(&self, from: Point, to: Point, color: Color, width: i32) {
        if let Some(mut d2d) = self.d2d() {
            let half = if width % 2 == 1 { 0.5 } else { 0.0 };
            d2d.draw_line(
                PointF::new(from.x as f32 + half, from.y as f32 + half),
                PointF::new(to.x as f32 + half, to.y as f32 + half),
                color,
                Stroke::solid(width.max(1) as f32),
            );
            let _ = d2d.end_draw();
            return;
        }
        if let Some(pen) = cache::pen(color, width.max(1)) {
            let previous = sys::gdi::select_pen(self.dc, pen);
            sys::gdi::line(self.dc, from, to);
            sys::gdi::select_object(self.dc, previous);
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
        if let Some(mut d2d) = self.d2d() {
            let shape = RectF::from_rect(rect);
            let radius = radius.max(1) as f32;
            d2d.fill_rounded_rect(shape, radius, fill);
            if let Some(color) = border {
                // Inset by half a pixel so the 1px outline lands inside the
                // shape and stays crisp instead of straddling the edge.
                let inner = RectF::new(
                    shape.left + 0.5,
                    shape.top + 0.5,
                    shape.right - 0.5,
                    shape.bottom - 0.5,
                );
                d2d.stroke_rounded_rect(inner, radius, color, Stroke::solid(1.0));
            }
            let _ = d2d.end_draw();
            return;
        }
        let Some(brush) = cache::solid_brush(fill) else {
            return;
        };
        let old_brush = sys::gdi::select_brush(self.dc, brush);
        let old_pen = match border.and_then(|color| cache::pen(color, 1)) {
            Some(pen) => sys::gdi::select_pen(self.dc, pen),
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
        if let Some(mut d2d) = self.d2d()
            && let Some(path) = triangle_path(rect, pointing_up)
        {
            d2d.fill_path(&path, color.into());
            let _ = d2d.end_draw();
            return;
        }
        let Some(brush) = cache::solid_brush(color) else {
            return;
        };
        let previous_brush = sys::gdi::select_brush(self.dc, brush);
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

/// The Direct2D path for a sort-arrow triangle inside `rect`.
fn triangle_path(rect: Rect, pointing_up: bool) -> Option<crate::d2d::Path> {
    let shape = RectF::from_rect(rect);
    let middle = (shape.left + shape.right) / 2.0;
    let mut builder = PathBuilder::new().ok()?;
    if pointing_up {
        builder
            .move_to(PointF::new(middle, shape.top))
            .line_to(PointF::new(shape.left, shape.bottom))
            .line_to(PointF::new(shape.right, shape.bottom));
    } else {
        builder
            .move_to(PointF::new(shape.left, shape.top))
            .line_to(PointF::new(shape.right, shape.top))
            .line_to(PointF::new(middle, shape.bottom));
    }
    builder.close();
    builder.build().ok()
}

/// A double-buffered paint session. Drop it to flush the dirty rectangle to the
/// screen and end the paint.
///
/// The off-screen buffer is kept per window between paints (and released when
/// the window is destroyed), so repeated paints do not allocate a bitmap each
/// time; it is only recreated when the client area grows or the DPI changes.
pub struct Paint {
    hwnd: Hwnd,
    ps: PAINTSTRUCT,
    memory_dc: HDC,
    paint: Rect,
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
        let paint = Rect::new(
            ps.rcPaint.left,
            ps.rcPaint.top,
            ps.rcPaint.right,
            ps.rcPaint.bottom,
        );
        let dpi = sys::dpi::window_dpi(hwnd);
        let Some(memory_dc) =
            sys::gdi::acquire_back_buffer(hwnd, dc, client.width(), client.height(), dpi)
        else {
            sys::gdi::end_paint(hwnd, &ps);
            return None;
        };
        // Clip the off-screen buffer to the dirty rectangle so callers that
        // paint everything do not pay for off-screen work.
        sys::gdi::reset_clip(memory_dc);
        sys::gdi::clip_rect(memory_dc, paint);
        Some(Paint {
            hwnd,
            ps,
            memory_dc,
            paint,
            canvas: Canvas::with_paint_rect(memory_dc, paint),
            client,
        })
    }

    /// The drawing surface.
    pub fn canvas(&self) -> &Canvas {
        &self.canvas
    }

    /// Draws the widget through the scroll `offset` (device pixels): the back
    /// buffer is clipped to the viewport, so content above it is shifted out
    /// while `Canvas::paint_rect` reports the dirty band in content
    /// coordinates. Call before the widget paints.
    pub fn set_scroll_offset(&mut self, offset: i32) {
        self.canvas.set_scroll(offset);
        sys::gdi::set_viewport_origin(self.memory_dc, 0, -offset);
    }

    /// The rectangle Windows asked to repaint (`PAINTSTRUCT.rcPaint`).
    pub fn paint_rect(&self) -> Rect {
        self.paint
    }

    /// The client rectangle being painted.
    pub fn client_rect(&self) -> Rect {
        self.client
    }
}

impl Drop for Paint {
    fn drop(&mut self) {
        // Undo any scroll origin so the back buffer is blitted in device
        // coordinates (`blit_rect`'s source rectangle is in the DC's space).
        sys::gdi::set_viewport_origin(self.memory_dc, 0, 0);
        sys::gdi::blit_rect(self.ps.hdc, self.memory_dc, self.paint);
        sys::gdi::end_paint(self.hwnd, &self.ps);
    }
}
