#![forbid(unsafe_code)]

//! A retained path geometry and a builder for it: lines, arcs, cubic and
//! quadratic Béziers, for borders and shapes, plus per-corner rounded
//! rectangles and clip layers.

use std::marker::PhantomData;

use crate::error::Result;
use crate::sys::d2d::geometry::{PathGeometry, PathSink};

use super::canvas::D2dCanvas;
use super::{PointF, Rgba, RoundedRect, Stroke};

/// The direction an arc is swept.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Sweep {
    /// In the direction of increasing angle.
    #[default]
    Clockwise,
    /// Against increasing angle.
    CounterClockwise,
}

/// Whether an arc takes the shorter or longer way around the ellipse.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ArcSize {
    /// The arc that is less than 180 degrees.
    #[default]
    Small,
    /// The arc that is more than 180 degrees.
    Large,
}

/// A finished path geometry, drawable with [`D2dCanvas::fill_path`] and
/// [`D2dCanvas::stroke_path`].
///
/// A path is device-independent, so it survives a device loss; it is bound to
/// the UI thread that created it.
pub struct Path {
    geometry: PathGeometry,
    _not_send: PhantomData<*mut ()>,
}

/// Builds a [`Path`] figure by figure. Each [`move_to`](PathBuilder::move_to)
/// starts a new figure; [`close`](PathBuilder::close) closes the current one.
pub struct PathBuilder {
    geometry: PathGeometry,
    sink: PathSink,
    open: bool,
    _not_send: PhantomData<*mut ()>,
}

impl PathBuilder {
    /// Starts a new, empty path.
    pub fn new() -> Result<PathBuilder> {
        let geometry = PathGeometry::new()?;
        let sink = geometry.open()?;
        Ok(PathBuilder {
            geometry,
            sink,
            open: false,
            _not_send: PhantomData,
        })
    }

    /// Ends the current figure (if any) and starts a new one at `point`.
    pub fn move_to(&mut self, point: PointF) -> &mut Self {
        self.end_open_figure();
        self.sink.begin_figure(point);
        self.open = true;
        self
    }

    /// Adds a straight line to `point`.
    pub fn line_to(&mut self, point: PointF) -> &mut Self {
        self.sink.add_line(point);
        self
    }

    /// Adds a quadratic Bézier curve to `end` with `control`.
    pub fn quadratic_to(&mut self, control: PointF, end: PointF) -> &mut Self {
        self.sink.add_quadratic(control, end);
        self
    }

    /// Adds a cubic Bézier curve to `end` with controls `control1` and
    /// `control2`.
    pub fn cubic_to(&mut self, control1: PointF, control2: PointF, end: PointF) -> &mut Self {
        self.sink.add_cubic(control1, control2, end);
        self
    }

    /// Adds an elliptical arc to `end` with the given radii, sweep and size.
    pub fn arc_to(
        &mut self,
        end: PointF,
        radius_x: f32,
        radius_y: f32,
        sweep: Sweep,
        size: ArcSize,
    ) -> &mut Self {
        self.sink.add_arc(end, radius_x, radius_y, sweep, size);
        self
    }

    /// Closes the current figure (a straight line back to its start).
    pub fn close(&mut self) -> &mut Self {
        self.end_open_figure_closed();
        self
    }

    fn end_open_figure(&mut self) {
        if self.open {
            self.sink.end_figure(false);
            self.open = false;
        }
    }

    fn end_open_figure_closed(&mut self) {
        if self.open {
            self.sink.end_figure(true);
            self.open = false;
        }
    }

    /// Finalizes the path, leaving any open figure unclosed.
    pub fn build(mut self) -> Result<Path> {
        self.end_open_figure();
        self.sink.close()?;
        Ok(Path {
            geometry: self.geometry,
            _not_send: PhantomData,
        })
    }
}

impl<'a> D2dCanvas<'a> {
    /// Fills `path` with an RGBA colour.
    pub fn fill_path(&mut self, path: &Path, color: Rgba) {
        self.with(|target| target.fill_path(&path.geometry, color));
    }

    /// Outlines `path` with an RGBA colour.
    pub fn stroke_path(&mut self, path: &Path, color: Rgba, stroke: Stroke) {
        self.with(|target| target.stroke_path(&path.geometry, color, stroke));
    }

    /// Fills a rounded rectangle with a (possibly different) radius per corner.
    pub fn fill_rounded(&mut self, rounded: RoundedRect, color: Rgba) {
        self.with(|target| target.fill_rounded(rounded, color));
    }

    /// Outlines a rounded rectangle with a (possibly different) radius per
    /// corner.
    pub fn stroke_rounded(&mut self, rounded: RoundedRect, color: Rgba, stroke: Stroke) {
        self.with(|target| target.stroke_rounded(rounded, color, stroke));
    }

    /// Restricts drawing to `rounded` until the matching
    /// [`pop_clip`](D2dCanvas::pop_clip), using a geometric-mask layer (so the
    /// clip itself is anti-aliased).
    pub fn push_clip_rounded(&mut self, rounded: RoundedRect) -> Result<()> {
        self.with(|target| target.push_clip_rounded(rounded))
            .unwrap_or(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_builds_a_rectangle() {
        let mut builder = PathBuilder::new().expect("path");
        builder
            .move_to(PointF::new(0.0, 0.0))
            .line_to(PointF::new(10.0, 0.0))
            .line_to(PointF::new(10.0, 10.0))
            .close();
        let _path = builder.build().expect("built");
    }
}
