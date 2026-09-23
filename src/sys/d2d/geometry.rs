//! Geometry that is independent of the render target (rounded-rectangle and
//! path geometries, created from the factory) plus the target's clip/layer
//! stack. Layers are device-dependent and live on the stack, so they are
//! dropped with the `Target` on device loss.

use std::mem::ManuallyDrop;

use windows::Win32::Graphics::Direct2D::Common::{
    D2D_SIZE_F, D2D1_BEZIER_SEGMENT, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED,
    D2D1_FIGURE_END_OPEN,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_LARGE, D2D1_ARC_SIZE_SMALL,
    D2D1_LAYER_PARAMETERS, D2D1_QUADRATIC_BEZIER_SEGMENT, D2D1_ROUNDED_RECT,
    D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE, ID2D1Geometry,
    ID2D1GeometrySink, ID2D1HwndRenderTarget, ID2D1Layer, ID2D1PathGeometry,
};

use crate::d2d::{ArcSize, PointF, Radius, RectF, Rgba, RoundedRect, Stroke, Sweep};
use crate::error::{Error, Result};
use crate::sys::win32_error;

use super::factory;
use super::target::{Target, rect_f, vector};

/// One entry on the Direct2D clip/layer stack.
pub(crate) enum Clip {
    /// An axis-aligned clip (from `push_clip`).
    AxisAligned,
    /// A geometric-mask layer (from `push_clip_rounded`), owning its layer.
    Layer(ID2D1Layer),
}

/// The target's clip/layer stack, with a pool of reusable layers.
pub(crate) struct Shapes {
    clips: Vec<Clip>,
    pool: Vec<ID2D1Layer>,
}

impl Shapes {
    pub(crate) fn new() -> Shapes {
        Shapes {
            clips: Vec::new(),
            pool: Vec::new(),
        }
    }

    fn push_axis_aligned(&mut self, render: &ID2D1HwndRenderTarget, rect: RectF) {
        // SAFETY: a valid rect; matched by `pop` (or `drain` at end of frame).
        unsafe {
            render.PushAxisAlignedClip(&rect_f(rect), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }
        self.clips.push(Clip::AxisAligned);
    }

    fn push_layer(&mut self, render: &ID2D1HwndRenderTarget, rounded: RoundedRect) -> Result<()> {
        let geometry = rounded_geometry(rounded)?;
        let layer = if let Some(layer) = self.pool.pop() {
            layer
        } else {
            // SAFETY: `None` asks Direct2D to size the layer to its mask.
            unsafe { render.CreateLayer(None) }.map_err(win32_error)?
        };
        let parameters = D2D1_LAYER_PARAMETERS {
            contentBounds: rect_f(rounded.rect),
            geometricMask: ManuallyDrop::new(Some(geometry)),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: windows_numerics::Matrix3x2::identity(),
            opacity: 1.0,
            ..Default::default()
        };
        // SAFETY: valid parameters and a layer created from this target.
        unsafe { render.PushLayer(&parameters, &layer) }
        self.clips.push(Clip::Layer(layer));
        Ok(())
    }

    /// Pops the innermost clip/layer, or errors when the stack is empty.
    fn pop(&mut self, render: &ID2D1HwndRenderTarget) -> Result<()> {
        match self.clips.pop() {
            None => Err(Error::Direct2d("pop_clip without a matching push_clip")),
            Some(Clip::AxisAligned) => {
                // SAFETY: there is an open axis-aligned clip, tracked by `clips`.
                unsafe {
                    render.PopAxisAlignedClip();
                }
                Ok(())
            }
            Some(Clip::Layer(layer)) => {
                // SAFETY: there is an open layer, tracked by `clips`.
                unsafe { render.PopLayer() }
                self.pool.push(layer);
                Ok(())
            }
        }
    }

    /// Pops everything left open, for end-of-frame cleanup.
    fn drain(&mut self, render: &ID2D1HwndRenderTarget) {
        while !self.clips.is_empty() {
            let _ = self.pop(render);
        }
    }
}

/// An immutable path geometry, independent of the render target.
pub(crate) struct PathGeometry {
    pub(crate) geometry: ID2D1PathGeometry,
}

impl PathGeometry {
    /// Creates an empty path geometry.
    pub(crate) fn new() -> Result<PathGeometry> {
        // SAFETY: the factory creates a path geometry with no input pointers.
        let geometry = unsafe { factory()?.CreatePathGeometry() }.map_err(win32_error)?;
        Ok(PathGeometry { geometry })
    }

    /// Opens a sink to describe the path with.
    pub(crate) fn open(&self) -> Result<PathSink> {
        // SAFETY: opening a sink on a live geometry.
        let sink = unsafe { self.geometry.Open() }.map_err(win32_error)?;
        Ok(PathSink { sink })
    }
}

/// A sink describing a path's figures and segments.
pub(crate) struct PathSink {
    pub(crate) sink: ID2D1GeometrySink,
}

impl PathSink {
    pub(crate) fn begin_figure(&self, start: PointF) {
        // SAFETY: a valid start point.
        unsafe {
            self.sink
                .BeginFigure(vector(start), D2D1_FIGURE_BEGIN_FILLED)
        }
    }

    pub(crate) fn end_figure(&self, closed: bool) {
        let end = if closed {
            D2D1_FIGURE_END_CLOSED
        } else {
            D2D1_FIGURE_END_OPEN
        };
        // SAFETY: a figure is open.
        unsafe { self.sink.EndFigure(end) }
    }

    pub(crate) fn add_line(&self, point: PointF) {
        // SAFETY: a valid point.
        unsafe { self.sink.AddLine(vector(point)) }
    }

    pub(crate) fn add_quadratic(&self, control: PointF, end: PointF) {
        let segment = D2D1_QUADRATIC_BEZIER_SEGMENT {
            point1: vector(control),
            point2: vector(end),
        };
        // SAFETY: a valid segment.
        unsafe { self.sink.AddQuadraticBezier(&segment) }
    }

    pub(crate) fn add_cubic(&self, control1: PointF, control2: PointF, end: PointF) {
        let segment = D2D1_BEZIER_SEGMENT {
            point1: vector(control1),
            point2: vector(control2),
            point3: vector(end),
        };
        // SAFETY: a valid segment.
        unsafe { self.sink.AddBezier(&segment) }
    }

    pub(crate) fn add_arc(
        &self,
        end: PointF,
        radius_x: f32,
        radius_y: f32,
        sweep: Sweep,
        size: ArcSize,
    ) {
        let segment = D2D1_ARC_SEGMENT {
            point: vector(end),
            size: D2D_SIZE_F {
                width: radius_x * 2.0,
                height: radius_y * 2.0,
            },
            rotationAngle: 0.0,
            sweepDirection: sweep_direction(sweep),
            arcSize: arc_size(size),
        };
        // SAFETY: a valid arc segment.
        unsafe { self.sink.AddArc(&segment) }
    }

    pub(crate) fn close(&self) -> Result<()> {
        // SAFETY: closes the sink, finalizing the geometry.
        unsafe { self.sink.Close() }.map_err(win32_error)
    }
}

fn sweep_direction(sweep: Sweep) -> windows::Win32::Graphics::Direct2D::D2D1_SWEEP_DIRECTION {
    match sweep {
        Sweep::Clockwise => D2D1_SWEEP_DIRECTION_CLOCKWISE,
        Sweep::CounterClockwise => D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE,
    }
}

fn arc_size(size: ArcSize) -> windows::Win32::Graphics::Direct2D::D2D1_ARC_SIZE {
    match size {
        ArcSize::Small => D2D1_ARC_SIZE_SMALL,
        ArcSize::Large => D2D1_ARC_SIZE_LARGE,
    }
}

/// The geometry for `rounded`: the built-in rounded-rectangle geometry when the
/// corners are uniform, a path geometry otherwise.
fn rounded_geometry(rounded: RoundedRect) -> Result<ID2D1Geometry> {
    if rounded.is_uniform() {
        let radius = rounded.radii[0];
        let d2d = D2D1_ROUNDED_RECT {
            rect: rect_f(rounded.rect),
            radiusX: radius.x,
            radiusY: radius.y,
        };
        // SAFETY: a valid rounded rectangle.
        let geometry =
            unsafe { factory()?.CreateRoundedRectangleGeometry(&d2d) }.map_err(win32_error)?;
        Ok(ID2D1Geometry::from(geometry))
    } else {
        let path = rounded_path(rounded)?;
        Ok(ID2D1Geometry::from(path.geometry))
    }
}

/// Builds a per-corner rounded rectangle as a path geometry.
fn rounded_path(rounded: RoundedRect) -> Result<PathGeometry> {
    let rect = rounded.rect;
    let max_x = (rect.width() / 2.0).max(0.0);
    let max_y = (rect.height() / 2.0).max(0.0);
    let clamp =
        |radius: Radius| Radius::new(radius.x.clamp(0.0, max_x), radius.y.clamp(0.0, max_y));
    let [tl, tr, br, bl] = rounded.radii.map(clamp);

    let path = PathGeometry::new()?;
    let sink = path.open()?;
    let (left, top, right, bottom) = (rect.left, rect.top, rect.right, rect.bottom);

    sink.begin_figure(PointF::new(left + tl.x, top));
    sink.add_line(PointF::new(right - tr.x, top));
    add_corner(&sink, PointF::new(right, top + tr.y), tr);
    sink.add_line(PointF::new(right, bottom - br.y));
    add_corner(&sink, PointF::new(right - br.x, bottom), br);
    sink.add_line(PointF::new(left + bl.x, bottom));
    add_corner(&sink, PointF::new(left, bottom - bl.y), bl);
    sink.add_line(PointF::new(left, top + tl.y));
    add_corner(&sink, PointF::new(left + tl.x, top), tl);
    sink.end_figure(true);
    sink.close()?;
    Ok(path)
}

/// Adds a corner arc (or a straight line when the radius is zero).
fn add_corner(sink: &PathSink, end: PointF, radius: Radius) {
    if radius.x > 0.0 && radius.y > 0.0 {
        sink.add_arc(end, radius.x, radius.y, Sweep::Clockwise, ArcSize::Small);
    } else {
        sink.add_line(end);
    }
}

impl Target {
    pub(crate) fn push_clip(&mut self, rect: RectF) {
        self.shapes.push_axis_aligned(&self.render, rect);
    }

    /// Pops the innermost clip/layer; errors when nothing is open.
    pub(crate) fn pop_clip(&mut self) -> Result<()> {
        self.shapes.pop(&self.render)
    }

    /// Pops every clip/layer left open (end-of-frame cleanup).
    pub(crate) fn drain_clips(&mut self) {
        self.shapes.drain(&self.render);
    }

    pub(crate) fn push_clip_rounded(&mut self, rounded: RoundedRect) -> Result<()> {
        self.shapes.push_layer(&self.render, rounded)
    }

    /// Fills a rounded rectangle with per-corner radii.
    pub(crate) fn fill_rounded(&mut self, rounded: RoundedRect, color: Rgba) {
        if rounded.is_uniform() {
            self.fill_rounded_rect_rgba(rounded.rect, rounded.radii[0].x, color);
            return;
        }
        if let (Ok(geometry), Some(brush)) = (
            rounded_geometry(rounded),
            self.paints.solid(&self.render, color),
        ) {
            // SAFETY: valid geometry and brush from this target.
            unsafe { self.render.FillGeometry(&geometry, &brush, None) }
        }
    }

    /// Outlines a rounded rectangle with per-corner radii.
    pub(crate) fn stroke_rounded(&mut self, rounded: RoundedRect, color: Rgba, stroke: Stroke) {
        if rounded.is_uniform() {
            self.stroke_rounded_rect_rgba(rounded.rect, rounded.radii[0].x, color, stroke);
            return;
        }
        if let (Ok(geometry), Some((brush, style))) =
            (rounded_geometry(rounded), self.pen_rgba(color, stroke))
        {
            // SAFETY: valid geometry, brush and stroke style from this target.
            unsafe {
                self.render
                    .DrawGeometry(&geometry, &brush, stroke.width, style.as_ref())
            }
        }
    }

    pub(crate) fn fill_path(&mut self, path: &PathGeometry, color: Rgba) {
        if let Some(brush) = self.paints.solid(&self.render, color) {
            // SAFETY: valid geometry and brush from this target.
            unsafe { self.render.FillGeometry(&path.geometry, &brush, None) }
        }
    }

    pub(crate) fn stroke_path(&mut self, path: &PathGeometry, color: Rgba, stroke: Stroke) {
        if let Some((brush, style)) = self.pen_rgba(color, stroke) {
            // SAFETY: valid geometry, brush and stroke style from this target.
            unsafe {
                self.render
                    .DrawGeometry(&path.geometry, &brush, stroke.width, style.as_ref())
            }
        }
    }
}
