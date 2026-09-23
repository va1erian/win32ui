#![forbid(unsafe_code)]

//! [`DcCanvas`]: anti-aliased Direct2D drawing onto a raw `HDC`.
//!
//! Owner-drawn controls receive an `HDC` from `WM_DRAWITEM`/`WM_PAINT`, not a
//! window, so they cannot use a [`D2dSurface`](super::D2dSurface)
//! (`ID2D1HwndRenderTarget`). A `DcCanvas` binds an
//! `ID2D1DCRenderTarget` to that device context for one paint and exposes the
//! same primitives as [`D2dCanvas`](super::D2dCanvas); the shapes are
//! anti-aliased and composite over whatever GDI already painted (background,
//! text).
//!
//! The render target and its brushes are created once per thread and re-bound
//! per draw, so a control pays no device-resource cost per paint. Call
//! [`end_draw`](DcCanvas::end_draw) (or drop the canvas) before drawing with
//! GDI on the same device context.

use crate::color::Color;
use crate::error::Result;
use crate::geometry::Rect;
use crate::sys;
use crate::sys::d2d::{EndDraw, Target};

use super::{PointF, RectF, Stroke};

/// One anti-aliased paint onto a device context.
///
/// Bind it with [`DcCanvas::new`], draw, then [`end_draw`](DcCanvas::end_draw);
/// a device loss is transparent (the target is rebuilt on the next bind).
/// Coordinates are device-independent pixels relative to the bound `rect`.
pub struct DcCanvas {
    rect: RectF,
    finished: bool,
}

impl DcCanvas {
    /// Binds the thread's DC render target to `dc` over `rect` (device pixels)
    /// and starts a frame. Fails when Direct2D is unavailable, so a caller can
    /// fall back to GDI.
    pub fn new(dc: isize, rect: Rect) -> Result<DcCanvas> {
        if dc == 0 || rect.is_empty() {
            return Err(crate::error::Error::Direct2d(
                "no device context to draw on",
            ));
        }
        sys::d2d::dc::begin(dc, rect)?;
        Ok(DcCanvas {
            rect: RectF::from_rect(rect),
            finished: false,
        })
    }

    pub(in crate::d2d) fn with<R>(&self, draw: impl FnOnce(&mut Target) -> R) -> Option<R> {
        sys::d2d::dc::with(draw)
    }

    /// The bound rectangle, from the origin, in device-independent pixels.
    pub fn bounds(&self) -> RectF {
        self.rect
    }

    /// Fills the whole bound rectangle with `color`.
    pub fn clear(&mut self, color: Color) {
        self.with(|target| target.clear(color));
    }

    /// Fills `rect`.
    pub fn fill_rect(&mut self, rect: RectF, color: Color) {
        self.with(|target| target.fill_rect(rect, color));
    }

    /// Fills `rect` with corners of `radius`.
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
    /// [`pop_clip`](DcCanvas::pop_clip).
    pub fn push_clip(&mut self, rect: RectF) {
        self.with(|target| target.push_clip(rect));
    }

    /// Ends the innermost clip. Popping with nothing open is an error, not a
    /// crash.
    pub fn pop_clip(&mut self) -> Result<()> {
        self.with(Target::pop_clip).unwrap_or(Ok(()))
    }

    /// Offsets everything drawn afterwards by `(x, y)`.
    pub fn set_translation(&mut self, x: f32, y: f32) {
        self.with(|target| target.set_translation(x, y));
    }

    /// Ends the frame. A lost device is not an error: the target is rebuilt on
    /// the next bind.
    pub fn end_draw(mut self) -> Result<()> {
        self.finish()
    }

    fn finish(&mut self) -> Result<()> {
        if std::mem::replace(&mut self.finished, true) {
            return Ok(());
        }
        match sys::d2d::dc::end()? {
            EndDraw::Presented | EndDraw::TargetLost => Ok(()),
        }
    }
}

impl Drop for DcCanvas {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Binding to no device context must fail cleanly so the GDI fallback can
    /// run, rather than panicking or leaving a half-open frame.
    #[test]
    fn a_missing_dc_reports_an_error() {
        assert!(DcCanvas::new(0, Rect::new(0, 0, 10, 10)).is_err());
    }
}
