#![forbid(unsafe_code)]

//! [`D2dCanvas`]: the anti-aliased drawing surface for one frame.

use crate::color::Color;
use crate::error::Result;
use crate::sys;
use crate::sys::d2d::{EndDraw, Target};

use super::{D2dSurface, PointF, RectF, Stroke};

/// One frame of Direct2D drawing, from [`D2dSurface::begin_draw`].
///
/// Coordinates are device-independent pixels. Shapes are anti-aliased;
/// solid brushes are cached per colour for the life of the render target, so
/// a paint allocates nothing after the first frame. Call
/// [`end_draw`](D2dCanvas::end_draw) to present; dropping the canvas presents
/// too, but discards any error.
pub struct D2dCanvas<'a> {
    pub(super) surface: &'a D2dSurface,
    finished: bool,
}

impl<'a> D2dCanvas<'a> {
    /// Begins the frame on the surface's (already prepared) target.
    pub(super) fn begin(surface: &'a D2dSurface) -> D2dCanvas<'a> {
        let canvas = D2dCanvas {
            surface,
            finished: false,
        };
        canvas.with(Target::begin_draw);
        canvas
    }

    pub(in crate::d2d) fn with<R>(&self, draw: impl FnOnce(&mut Target) -> R) -> Option<R> {
        self.surface.target.borrow_mut().as_mut().map(draw)
    }

    /// The drawable area, from the origin, in device-independent pixels.
    pub fn bounds(&self) -> RectF {
        let (width, height) = self
            .surface
            .target
            .borrow()
            .as_ref()
            .map_or((0.0, 0.0), Target::size);
        RectF::new(0.0, 0.0, width, height)
    }

    /// Fills the whole surface with `color`, or — inside a clip, as during a
    /// rect-scoped frame — only the clipped part. Unlike `Clear`, this honours
    /// the current clip, so a partial repaint does not wipe untouched pixels.
    pub fn clear(&mut self, color: Color) {
        let bounds = self.bounds();
        self.with(|target| target.fill_rect(bounds, color));
    }

    /// Fills `rect`.
    pub fn fill_rect(&mut self, rect: RectF, color: Color) {
        self.with(|target| target.fill_rect(rect, color));
    }

    /// Fills `rect` with corners of `radius` (clamped to half the shorter
    /// side; [`RectF::pill_radius`] gives fully round ends).
    pub fn fill_rounded_rect(&mut self, rect: RectF, radius: f32, color: Color) {
        self.with(|target| target.fill_rounded_rect(rect, radius, color));
    }

    /// Fills an ellipse.
    pub fn fill_ellipse(&mut self, center: PointF, radius_x: f32, radius_y: f32, color: Color) {
        self.with(|target| target.fill_ellipse(center, radius_x, radius_y, color));
    }

    /// Outlines `rect`.
    pub fn stroke_rect(&mut self, rect: RectF, color: Color, stroke: Stroke) {
        self.with(|target| target.stroke_rect(rect, color, stroke));
    }

    /// Outlines a rounded rectangle.
    pub fn stroke_rounded_rect(&mut self, rect: RectF, radius: f32, color: Color, stroke: Stroke) {
        self.with(|target| target.stroke_rounded_rect(rect, radius, color, stroke));
    }

    /// Outlines an ellipse.
    pub fn stroke_ellipse(
        &mut self,
        center: PointF,
        radius_x: f32,
        radius_y: f32,
        color: Color,
        stroke: Stroke,
    ) {
        self.with(|target| target.stroke_ellipse(center, radius_x, radius_y, color, stroke));
    }

    /// Draws a line.
    pub fn draw_line(&mut self, from: PointF, to: PointF, color: Color, stroke: Stroke) {
        self.with(|target| target.line(from, to, color, stroke));
    }

    /// Restricts drawing to `rect` until the matching
    /// [`pop_clip`](D2dCanvas::pop_clip). Clips nest; any left open are popped
    /// when the frame ends.
    pub fn push_clip(&mut self, rect: RectF) {
        self.with(|target| target.push_clip(rect));
    }

    /// Ends the innermost clip. Popping with nothing open is an error, not a
    /// crash.
    pub fn pop_clip(&mut self) -> Result<()> {
        self.with(Target::pop_clip).unwrap_or(Ok(()))
    }

    /// Offsets everything drawn afterwards by `(x, y)` (replacing any earlier
    /// offset), for example to scroll content.
    pub fn set_translation(&mut self, x: f32, y: f32) {
        self.with(|target| target.set_translation(x, y));
    }

    /// Presents the frame. A lost device is not an error: the render target
    /// is rebuilt and the window repainted on the next frame.
    pub fn end_draw(mut self) -> Result<()> {
        self.finish()
    }

    fn finish(&mut self) -> Result<()> {
        if std::mem::replace(&mut self.finished, true) {
            return Ok(());
        }
        let outcome = self
            .with(Target::end_draw)
            .unwrap_or(Ok(EndDraw::Presented));
        self.surface.drawing.set(false);
        sys::d2d::validate(self.surface.hwnd(), self.surface.frame());
        if outcome? == EndDraw::TargetLost {
            self.surface.recreate_later();
        }
        Ok(())
    }
}

impl Drop for D2dCanvas<'_> {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}
