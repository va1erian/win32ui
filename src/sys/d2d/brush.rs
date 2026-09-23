//! Device-dependent brushes (RGBA solid, linear and radial gradients) and
//! stroke styles, cached per render target so a paint allocates nothing after
//! the first frame. Everything here is dropped with the `Target` on device
//! loss and recreated lazily.

use std::collections::HashMap;

use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::Direct2D::{
    D2D1_CAP_STYLE_FLAT, D2D1_CAP_STYLE_ROUND, D2D1_CAP_STYLE_SQUARE, D2D1_CAP_STYLE_TRIANGLE,
    D2D1_EXTEND_MODE_CLAMP, D2D1_EXTEND_MODE_MIRROR, D2D1_EXTEND_MODE_WRAP, D2D1_GAMMA_2_2,
    D2D1_LINE_JOIN_BEVEL, D2D1_LINE_JOIN_MITER, D2D1_LINE_JOIN_ROUND,
    D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES,
    D2D1_STROKE_STYLE_PROPERTIES, ID2D1LinearGradientBrush, ID2D1RadialGradientBrush,
    ID2D1RenderTarget, ID2D1SolidColorBrush, ID2D1StrokeStyle,
};
use windows_numerics::Vector2;

use crate::color::Color;
use crate::d2d::{
    Cap, DashStyle, ExtendMode, GradientStop, LineJoin, LinearGradient, PointF, RadialGradient,
    RectF, Rgba, Stroke,
};

use super::target::{Target, rect_f, vector};
use super::{dash_style, factory};

/// An RGBA colour as Direct2D sees it (straight alpha; premultiplication is
/// the render target's concern).
pub(crate) fn color_f(color: Rgba) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: f32::from(color.r) / 255.0,
        g: f32::from(color.g) / 255.0,
        b: f32::from(color.b) / 255.0,
        a: f32::from(color.a) / 255.0,
    }
}

fn extend_mode(mode: ExtendMode) -> windows::Win32::Graphics::Direct2D::D2D1_EXTEND_MODE {
    match mode {
        ExtendMode::Clamp => D2D1_EXTEND_MODE_CLAMP,
        ExtendMode::Wrap => D2D1_EXTEND_MODE_WRAP,
        ExtendMode::Mirror => D2D1_EXTEND_MODE_MIRROR,
    }
}

fn cap_style(cap: Cap) -> windows::Win32::Graphics::Direct2D::D2D1_CAP_STYLE {
    match cap {
        Cap::Flat => D2D1_CAP_STYLE_FLAT,
        Cap::Square => D2D1_CAP_STYLE_SQUARE,
        Cap::Round => D2D1_CAP_STYLE_ROUND,
        Cap::Triangle => D2D1_CAP_STYLE_TRIANGLE,
    }
}

fn join_style(join: LineJoin) -> windows::Win32::Graphics::Direct2D::D2D1_LINE_JOIN {
    match join {
        LineJoin::Miter => D2D1_LINE_JOIN_MITER,
        LineJoin::Bevel => D2D1_LINE_JOIN_BEVEL,
        LineJoin::Round => D2D1_LINE_JOIN_ROUND,
    }
}

/// The cache key for a stroke style: everything `ID2D1StrokeStyle` records.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct StrokeKey {
    dash: DashStyle,
    cap: Cap,
    join: LineJoin,
    offset_bits: u32,
}

/// Stroke styles, cached per render target.
pub(crate) struct Strokes {
    styles: HashMap<StrokeKey, ID2D1StrokeStyle>,
}

impl Strokes {
    pub(crate) fn new() -> Strokes {
        Strokes {
            styles: HashMap::new(),
        }
    }

    /// The stroke style for `stroke`, or `None` when Direct2D's defaults are
    /// already right (a solid line with flat caps and miter joins).
    fn get(&mut self, stroke: Stroke) -> Option<ID2D1StrokeStyle> {
        if stroke.dash == DashStyle::Solid
            && stroke.cap == Cap::Flat
            && stroke.join == LineJoin::Miter
            && stroke.dash_offset == 0.0
        {
            return None;
        }
        let key = StrokeKey {
            dash: stroke.dash,
            cap: stroke.cap,
            join: stroke.join,
            offset_bits: stroke.dash_offset.to_bits(),
        };
        if let Some(style) = self.styles.get(&key) {
            return Some(style.clone());
        }
        let properties = D2D1_STROKE_STYLE_PROPERTIES {
            startCap: cap_style(stroke.cap),
            endCap: cap_style(stroke.cap),
            dashCap: cap_style(stroke.cap),
            lineJoin: join_style(stroke.join),
            miterLimit: 10.0,
            dashStyle: dash_style(stroke.dash),
            dashOffset: stroke.dash_offset,
        };
        // SAFETY: the properties struct is valid for the call; no custom dash
        // array is passed. Stroke styles come from the (thread-local) factory.
        let style = unsafe { factory().ok()?.CreateStrokeStyle(&properties, None) }.ok()?;
        self.styles.insert(key, style.clone());
        Some(style)
    }
}

/// RGBA solid and gradient brushes, cached per render target.
pub(crate) struct Paints {
    solid: HashMap<Rgba, ID2D1SolidColorBrush>,
    linear: Vec<(LinearGradient, ID2D1LinearGradientBrush)>,
    radial: Vec<(RadialGradient, ID2D1RadialGradientBrush)>,
}

impl Paints {
    pub(crate) fn new() -> Paints {
        Paints {
            solid: HashMap::new(),
            linear: Vec::new(),
            radial: Vec::new(),
        }
    }

    pub(crate) fn solid(
        &mut self,
        render: &ID2D1RenderTarget,
        color: Rgba,
    ) -> Option<ID2D1SolidColorBrush> {
        if let Some(brush) = self.solid.get(&color) {
            return Some(brush.clone());
        }
        // SAFETY: the colour struct is valid for the call; default properties.
        let brush = unsafe { render.CreateSolidColorBrush(&color_f(color), None) }.ok()?;
        self.solid.insert(color, brush.clone());
        Some(brush)
    }

    pub(crate) fn linear(
        &mut self,
        render: &ID2D1RenderTarget,
        gradient: &LinearGradient,
    ) -> Option<ID2D1LinearGradientBrush> {
        if let Some((_, brush)) = self.linear.iter().find(|(cached, _)| cached == gradient) {
            return Some(brush.clone());
        }
        let stops = stops_d2d(&gradient.stops);
        // SAFETY: the stop slice and property struct are valid for the calls.
        let collection = unsafe {
            render.CreateGradientStopCollection(
                &stops,
                D2D1_GAMMA_2_2,
                extend_mode(gradient.extend),
            )
        }
        .ok()?;
        let properties = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
            startPoint: vector(gradient.start),
            endPoint: vector(gradient.end),
        };
        // SAFETY: valid properties and a stop collection from this target.
        let brush =
            unsafe { render.CreateLinearGradientBrush(&properties, None, &collection) }.ok()?;
        self.linear.push((gradient.clone(), brush.clone()));
        Some(brush)
    }

    pub(crate) fn radial(
        &mut self,
        render: &ID2D1RenderTarget,
        gradient: &RadialGradient,
    ) -> Option<ID2D1RadialGradientBrush> {
        if let Some((_, brush)) = self.radial.iter().find(|(cached, _)| cached == gradient) {
            return Some(brush.clone());
        }
        let stops = stops_d2d(&gradient.stops);
        // SAFETY: the stop slice is valid for the call.
        let collection = unsafe {
            render.CreateGradientStopCollection(
                &stops,
                D2D1_GAMMA_2_2,
                extend_mode(gradient.extend),
            )
        }
        .ok()?;
        let properties = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
            center: vector(gradient.center),
            gradientOriginOffset: Vector2 { X: 0.0, Y: 0.0 },
            radiusX: gradient.radius_x,
            radiusY: gradient.radius_y,
        };
        // SAFETY: valid properties and a stop collection from this target.
        let brush =
            unsafe { render.CreateRadialGradientBrush(&properties, None, &collection) }.ok()?;
        self.radial.push((gradient.clone(), brush.clone()));
        Some(brush)
    }
}

/// The Direct2D stop list for a gradient.
fn stops_d2d(
    stops: &[GradientStop],
) -> Vec<windows::Win32::Graphics::Direct2D::Common::D2D1_GRADIENT_STOP> {
    stops
        .iter()
        .map(
            |stop| windows::Win32::Graphics::Direct2D::Common::D2D1_GRADIENT_STOP {
                position: stop.position,
                color: color_f(stop.color),
            },
        )
        .collect()
}

/// A brush and the optional stroke style to stroke with.
pub(crate) type Pen = (ID2D1SolidColorBrush, Option<ID2D1StrokeStyle>);

impl Target {
    /// The brush+style pair for an opaque colour (used by the `Color` stroke
    /// methods on [`Target`]).
    pub(crate) fn pen(&mut self, color: Color, stroke: Stroke) -> Option<Pen> {
        let brush = self.brush(color)?;
        Some((brush, self.strokes.get(stroke)))
    }

    pub(crate) fn pen_rgba(&mut self, color: Rgba, stroke: Stroke) -> Option<Pen> {
        let brush = self.paints.solid(&self.render, color)?;
        Some((brush, self.strokes.get(stroke)))
    }

    pub(crate) fn fill_rect_rgba(&mut self, rect: RectF, color: Rgba) {
        if let Some(brush) = self.paints.solid(&self.render, color) {
            // SAFETY: valid rect and brush from this target; drawing is active.
            unsafe { self.render.FillRectangle(&rect_f(rect), &brush) }
        }
    }

    pub(crate) fn fill_rounded_rect_rgba(&mut self, rect: RectF, radius: f32, color: Rgba) {
        if let Some(brush) = self.paints.solid(&self.render, color) {
            // SAFETY: valid geometry and brush from this target.
            unsafe {
                self.render
                    .FillRoundedRectangle(&super::target::rounded(rect, radius), &brush)
            }
        }
    }

    pub(crate) fn fill_ellipse_rgba(&mut self, center: PointF, rx: f32, ry: f32, color: Rgba) {
        if let Some(brush) = self.paints.solid(&self.render, color) {
            // SAFETY: valid geometry and brush from this target.
            unsafe {
                self.render
                    .FillEllipse(&super::target::ellipse(center, rx, ry), &brush)
            }
        }
    }

    pub(crate) fn stroke_rect_rgba(&mut self, rect: RectF, color: Rgba, stroke: Stroke) {
        if let Some((brush, style)) = self.pen_rgba(color, stroke) {
            // SAFETY: valid rect, brush and stroke style from this target.
            unsafe {
                self.render
                    .DrawRectangle(&rect_f(rect), &brush, stroke.width, style.as_ref())
            }
        }
    }

    pub(crate) fn stroke_rounded_rect_rgba(
        &mut self,
        rect: RectF,
        radius: f32,
        color: Rgba,
        stroke: Stroke,
    ) {
        if let Some((brush, style)) = self.pen_rgba(color, stroke) {
            // SAFETY: valid geometry, brush and stroke style from this target.
            unsafe {
                self.render.DrawRoundedRectangle(
                    &super::target::rounded(rect, radius),
                    &brush,
                    stroke.width,
                    style.as_ref(),
                )
            }
        }
    }

    pub(crate) fn stroke_ellipse_rgba(
        &mut self,
        center: PointF,
        rx: f32,
        ry: f32,
        color: Rgba,
        stroke: Stroke,
    ) {
        if let Some((brush, style)) = self.pen_rgba(color, stroke) {
            // SAFETY: valid geometry, brush and stroke style from this target.
            unsafe {
                self.render.DrawEllipse(
                    &super::target::ellipse(center, rx, ry),
                    &brush,
                    stroke.width,
                    style.as_ref(),
                )
            }
        }
    }

    pub(crate) fn line_rgba(&mut self, from: PointF, to: PointF, color: Rgba, stroke: Stroke) {
        if let Some((brush, style)) = self.pen_rgba(color, stroke) {
            // SAFETY: valid points, brush and stroke style from this target.
            unsafe {
                self.render.DrawLine(
                    vector(from),
                    vector(to),
                    &brush,
                    stroke.width,
                    style.as_ref(),
                )
            }
        }
    }

    pub(crate) fn fill_rect_linear(&mut self, rect: RectF, gradient: &LinearGradient) {
        if let Some(brush) = self.paints.linear(&self.render, gradient) {
            // SAFETY: valid rect and brush from this target; drawing is active.
            unsafe { self.render.FillRectangle(&rect_f(rect), &brush) }
        }
    }

    pub(crate) fn fill_rect_radial(&mut self, rect: RectF, gradient: &RadialGradient) {
        if let Some(brush) = self.paints.radial(&self.render, gradient) {
            // SAFETY: valid rect and brush from this target; drawing is active.
            unsafe { self.render.FillRectangle(&rect_f(rect), &brush) }
        }
    }

    pub(crate) fn fill_rounded_rect_linear(
        &mut self,
        rect: RectF,
        radius: f32,
        gradient: &LinearGradient,
    ) {
        if let Some(brush) = self.paints.linear(&self.render, gradient) {
            // SAFETY: valid geometry and brush from this target.
            unsafe {
                self.render
                    .FillRoundedRectangle(&super::target::rounded(rect, radius), &brush)
            }
        }
    }

    pub(crate) fn fill_ellipse_linear(
        &mut self,
        center: PointF,
        rx: f32,
        ry: f32,
        gradient: &LinearGradient,
    ) {
        if let Some(brush) = self.paints.linear(&self.render, gradient) {
            // SAFETY: valid geometry and brush from this target.
            unsafe {
                self.render
                    .FillEllipse(&super::target::ellipse(center, rx, ry), &brush)
            }
        }
    }

    pub(crate) fn fill_ellipse_radial(
        &mut self,
        center: PointF,
        rx: f32,
        ry: f32,
        gradient: &RadialGradient,
    ) {
        if let Some(brush) = self.paints.radial(&self.render, gradient) {
            // SAFETY: valid geometry and brush from this target.
            unsafe {
                self.render
                    .FillEllipse(&super::target::ellipse(center, rx, ry), &brush)
            }
        }
    }
}
