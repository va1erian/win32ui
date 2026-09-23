#![forbid(unsafe_code)]

//! RGBA colours and gradient brushes.
//!
//! win32ui's [`Color`](crate::Color) is opaque; [`Rgba`] adds an alpha channel
//! for Direct2D. Gradient brushes are device-dependent, so they are cached on
//! the surface's render target and recreated transparently after a device loss
//! (see [`D2dSurface`](super::D2dSurface)).

use crate::color::Color;

use super::canvas::D2dCanvas;
use super::{PointF, RectF, Stroke};

/// An RGBA colour with an alpha channel, for the Direct2D API.
///
/// win32ui's [`Color`](crate::Color) is opaque; where a drawing call takes an
/// [`Rgba`] it accepts transparency too. The alpha is straight (not
/// premultiplied); `sys` premultiplies it before handing the colour to
/// Direct2D.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rgba {
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8,
    /// Alpha channel: `0` is fully transparent, `255` fully opaque.
    pub a: u8,
}

impl Rgba {
    /// An opaque colour from its channels.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Rgba {
        Rgba { r, g, b, a: 0xFF }
    }

    /// A colour from its channels and alpha.
    pub const fn with_alpha(r: u8, g: u8, b: u8, a: u8) -> Rgba {
        Rgba { r, g, b, a }
    }

    /// Opaque white.
    pub const WHITE: Rgba = Rgba::rgb(0xFF, 0xFF, 0xFF);
    /// Opaque black.
    pub const BLACK: Rgba = Rgba::rgb(0x00, 0x00, 0x00);
    /// Fully transparent black.
    pub const TRANSPARENT: Rgba = Rgba::with_alpha(0x00, 0x00, 0x00, 0x00);
}

/// How a gradient behaves outside its stop range.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ExtendMode {
    /// The first/last colour is repeated (the default).
    #[default]
    Clamp,
    /// The gradient repeats, restarting from the first stop.
    Wrap,
    /// The gradient repeats, alternating direction.
    Mirror,
}

/// One colour in a gradient, at `position` in `0.0..=1.0`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradientStop {
    /// The position along the gradient, in `0.0..=1.0`.
    pub position: f32,
    /// The colour at that position.
    pub color: Rgba,
}

impl GradientStop {
    /// A stop at `position` with `color`.
    pub const fn new(position: f32, color: Rgba) -> GradientStop {
        GradientStop { position, color }
    }
}

/// A linear gradient from `start` to `end`, defined by its [`GradientStop`]s.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LinearGradient {
    /// Where the gradient starts (the position of stop `0.0`).
    pub start: PointF,
    /// Where the gradient ends (the position of stop `1.0`).
    pub end: PointF,
    /// The colours at their positions; at least two.
    pub stops: Vec<GradientStop>,
    /// What happens beyond the end stops.
    pub extend: ExtendMode,
}

impl LinearGradient {
    /// A gradient from `start` to `end` over `stops`.
    pub fn new(start: PointF, end: PointF, stops: Vec<GradientStop>) -> LinearGradient {
        LinearGradient {
            start,
            end,
            stops,
            extend: ExtendMode::Clamp,
        }
    }

    /// The same gradient with `extend`.
    pub fn extend(self, extend: ExtendMode) -> LinearGradient {
        LinearGradient { extend, ..self }
    }
}

/// A radial gradient centred at `center` with elliptical radii, defined by its
/// [`GradientStop`]s.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RadialGradient {
    /// The centre of the gradient (the position of stop `0.0`).
    pub center: PointF,
    /// The horizontal radius (the position of stop `1.0` along x).
    pub radius_x: f32,
    /// The vertical radius (the position of stop `1.0` along y).
    pub radius_y: f32,
    /// The colours at their positions; at least two.
    pub stops: Vec<GradientStop>,
    /// What happens beyond the end stops.
    pub extend: ExtendMode,
}

impl RadialGradient {
    /// A gradient centred at `center` with elliptical radii `radius_x`×`radius_y`.
    pub fn new(
        center: PointF,
        radius_x: f32,
        radius_y: f32,
        stops: Vec<GradientStop>,
    ) -> RadialGradient {
        RadialGradient {
            center,
            radius_x,
            radius_y,
            stops,
            extend: ExtendMode::Clamp,
        }
    }

    /// The same gradient with `extend`.
    pub fn extend(self, extend: ExtendMode) -> RadialGradient {
        RadialGradient { extend, ..self }
    }
}

impl<'a> D2dCanvas<'a> {
    /// Fills `rect` with an RGBA colour (alpha is premultiplied inside `sys`).
    pub fn fill_rect_rgba(&mut self, rect: RectF, color: Rgba) {
        self.with(|target| target.fill_rect_rgba(rect, color));
    }

    /// Fills `rect` with rounded corners of `radius` and an RGBA colour.
    pub fn fill_rounded_rect_rgba(&mut self, rect: RectF, radius: f32, color: Rgba) {
        self.with(|target| target.fill_rounded_rect_rgba(rect, radius, color));
    }

    /// Fills an ellipse with an RGBA colour.
    pub fn fill_ellipse_rgba(&mut self, center: PointF, radius_x: f32, radius_y: f32, color: Rgba) {
        self.with(|target| target.fill_ellipse_rgba(center, radius_x, radius_y, color));
    }

    /// Outlines `rect` with an RGBA colour.
    pub fn stroke_rect_rgba(&mut self, rect: RectF, color: Rgba, stroke: Stroke) {
        self.with(|target| target.stroke_rect_rgba(rect, color, stroke));
    }

    /// Outlines a rounded rectangle with an RGBA colour.
    pub fn stroke_rounded_rect_rgba(
        &mut self,
        rect: RectF,
        radius: f32,
        color: Rgba,
        stroke: Stroke,
    ) {
        self.with(|target| target.stroke_rounded_rect_rgba(rect, radius, color, stroke));
    }

    /// Outlines an ellipse with an RGBA colour.
    pub fn stroke_ellipse_rgba(
        &mut self,
        center: PointF,
        radius_x: f32,
        radius_y: f32,
        color: Rgba,
        stroke: Stroke,
    ) {
        self.with(|target| target.stroke_ellipse_rgba(center, radius_x, radius_y, color, stroke));
    }

    /// Draws a line with an RGBA colour.
    pub fn draw_line_rgba(&mut self, from: PointF, to: PointF, color: Rgba, stroke: Stroke) {
        self.with(|target| target.line_rgba(from, to, color, stroke));
    }

    /// Fills `rect` with a linear gradient.
    pub fn fill_rect_linear(&mut self, rect: RectF, gradient: &LinearGradient) {
        self.with(|target| target.fill_rect_linear(rect, gradient));
    }

    /// Fills `rect` with a radial gradient.
    pub fn fill_rect_radial(&mut self, rect: RectF, gradient: &RadialGradient) {
        self.with(|target| target.fill_rect_radial(rect, gradient));
    }

    /// Fills `rect` with rounded corners of `radius` and a linear gradient.
    pub fn fill_rounded_rect_linear(
        &mut self,
        rect: RectF,
        radius: f32,
        gradient: &LinearGradient,
    ) {
        self.with(|target| target.fill_rounded_rect_linear(rect, radius, gradient));
    }

    /// Fills an ellipse with a linear gradient.
    pub fn fill_ellipse_linear(
        &mut self,
        center: PointF,
        radius_x: f32,
        radius_y: f32,
        gradient: &LinearGradient,
    ) {
        self.with(|target| target.fill_ellipse_linear(center, radius_x, radius_y, gradient));
    }

    /// Fills an ellipse with a radial gradient.
    pub fn fill_ellipse_radial(
        &mut self,
        center: PointF,
        radius_x: f32,
        radius_y: f32,
        gradient: &RadialGradient,
    ) {
        self.with(|target| target.fill_ellipse_radial(center, radius_x, radius_y, gradient));
    }
}

impl From<Color> for Rgba {
    fn from(color: Color) -> Rgba {
        Rgba::rgb(color.r, color.g, color.b)
    }
}

impl super::DcCanvas {
    /// Fills `rect` with an RGBA colour.
    pub fn fill_rect_rgba(&mut self, rect: RectF, color: Rgba) {
        self.with(|target| target.fill_rect_rgba(rect, color));
    }

    /// Fills `rect` with rounded corners of `radius` and an RGBA colour.
    pub fn fill_rounded_rect_rgba(&mut self, rect: RectF, radius: f32, color: Rgba) {
        self.with(|target| target.fill_rounded_rect_rgba(rect, radius, color));
    }

    /// Fills an ellipse with an RGBA colour.
    pub fn fill_ellipse_rgba(&mut self, center: PointF, radius_x: f32, radius_y: f32, color: Rgba) {
        self.with(|target| target.fill_ellipse_rgba(center, radius_x, radius_y, color));
    }

    /// Outlines `rect` with an RGBA colour.
    pub fn stroke_rect_rgba(&mut self, rect: RectF, color: Rgba, stroke: Stroke) {
        self.with(|target| target.stroke_rect_rgba(rect, color, stroke));
    }

    /// Outlines a rounded rectangle with an RGBA colour.
    pub fn stroke_rounded_rect_rgba(
        &mut self,
        rect: RectF,
        radius: f32,
        color: Rgba,
        stroke: Stroke,
    ) {
        self.with(|target| target.stroke_rounded_rect_rgba(rect, radius, color, stroke));
    }

    /// Outlines an ellipse with an RGBA colour.
    pub fn stroke_ellipse_rgba(
        &mut self,
        center: PointF,
        radius_x: f32,
        radius_y: f32,
        color: Rgba,
        stroke: Stroke,
    ) {
        self.with(|target| target.stroke_ellipse_rgba(center, radius_x, radius_y, color, stroke));
    }

    /// Draws a line with an RGBA colour.
    pub fn draw_line_rgba(&mut self, from: PointF, to: PointF, color: Rgba, stroke: Stroke) {
        self.with(|target| target.line_rgba(from, to, color, stroke));
    }

    /// Fills `rect` with a linear gradient.
    pub fn fill_rect_linear(&mut self, rect: RectF, gradient: &LinearGradient) {
        self.with(|target| target.fill_rect_linear(rect, gradient));
    }

    /// Fills `rect` with a radial gradient.
    pub fn fill_rect_radial(&mut self, rect: RectF, gradient: &RadialGradient) {
        self.with(|target| target.fill_rect_radial(rect, gradient));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_converts_to_opaque_rgba() {
        let rgba: Rgba = Color::rgb(0x12, 0x34, 0x56).into();
        assert_eq!((rgba.r, rgba.g, rgba.b, rgba.a), (0x12, 0x34, 0x56, 0xFF));
    }

    #[test]
    fn gradient_extend_builder_overrides_the_default() {
        let gradient = LinearGradient::new(
            PointF::new(0.0, 0.0),
            PointF::new(10.0, 0.0),
            vec![
                GradientStop::new(0.0, Rgba::WHITE),
                GradientStop::new(1.0, Rgba::BLACK),
            ],
        )
        .extend(ExtendMode::Wrap);
        assert_eq!(gradient.extend, ExtendMode::Wrap);
    }
}
