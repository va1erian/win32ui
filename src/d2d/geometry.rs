#![forbid(unsafe_code)]

//! Floating-point geometry for Direct2D, which draws in device-independent
//! pixels (1/96 inch) rather than the integer device pixels of [`crate::gdi`].

use crate::geometry::Rect;

/// The DPI at which one device-independent pixel equals one device pixel.
pub const BASE_DPI: f32 = 96.0;

/// A point in device-independent pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PointF {
    /// Horizontal coordinate.
    pub x: f32,
    /// Vertical coordinate.
    pub y: f32,
}

impl PointF {
    /// Creates a point.
    pub const fn new(x: f32, y: f32) -> PointF {
        PointF { x, y }
    }
}

/// A rectangle in device-independent pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RectF {
    /// Left edge.
    pub left: f32,
    /// Top edge.
    pub top: f32,
    /// Right edge.
    pub right: f32,
    /// Bottom edge.
    pub bottom: f32,
}

impl RectF {
    /// Creates a rectangle from its edges.
    pub const fn new(left: f32, top: f32, right: f32, bottom: f32) -> RectF {
        RectF {
            left,
            top,
            right,
            bottom,
        }
    }

    /// Converts an integer rectangle, taking its values as device-independent
    /// pixels unchanged.
    pub fn from_rect(rect: Rect) -> RectF {
        RectF::new(
            rect.left as f32,
            rect.top as f32,
            rect.right as f32,
            rect.bottom as f32,
        )
    }

    /// The width (negative for an inverted rectangle).
    pub fn width(&self) -> f32 {
        self.right - self.left
    }

    /// The height (negative for an inverted rectangle).
    pub fn height(&self) -> f32 {
        self.bottom - self.top
    }

    /// The radius that turns this rectangle into a stadium: half its shorter
    /// side, so the ends are true semicircles.
    pub fn pill_radius(&self) -> f32 {
        (self.width().min(self.height()) / 2.0).max(0.0)
    }
}

/// How a stroked line or outline is broken up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DashStyle {
    /// An unbroken line.
    #[default]
    Solid,
    /// Dashes.
    Dashed,
    /// Dots.
    Dotted,
}

/// The pen for a stroked shape: a width in device-independent pixels and a
/// dash style.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stroke {
    /// The line width.
    pub width: f32,
    /// The dash style.
    pub dash: DashStyle,
}

impl Stroke {
    /// A solid stroke `width` wide.
    pub const fn solid(width: f32) -> Stroke {
        Stroke {
            width,
            dash: DashStyle::Solid,
        }
    }

    /// The same stroke with `dash`.
    pub const fn dash(self, dash: DashStyle) -> Stroke {
        Stroke { dash, ..self }
    }
}

/// Clamps a requested corner radius so it never exceeds half the shorter side
/// (Direct2D would otherwise draw a distorted shape) and is never negative.
pub fn clamp_radius(rect: RectF, radius: f32) -> f32 {
    radius.clamp(0.0, rect.pill_radius())
}

/// Converts a length in device pixels to device-independent pixels at `dpi`.
pub fn pixels_to_dips(pixels: i32, dpi: u32) -> f32 {
    pixels as f32 * BASE_DPI / dpi.max(1) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pill_radius_is_half_the_shorter_side() {
        assert_eq!(RectF::new(0.0, 0.0, 200.0, 8.0).pill_radius(), 4.0);
        assert_eq!(RectF::new(0.0, 0.0, 6.0, 8.0).pill_radius(), 3.0);
        assert_eq!(RectF::new(0.0, 0.0, -4.0, 8.0).pill_radius(), 0.0);
    }

    #[test]
    fn radius_is_clamped_to_the_pill() {
        let rect = RectF::new(0.0, 0.0, 100.0, 10.0);
        assert_eq!(clamp_radius(rect, 3.0), 3.0);
        assert_eq!(clamp_radius(rect, 50.0), 5.0);
        assert_eq!(clamp_radius(rect, -2.0), 0.0);
    }

    #[test]
    fn pixels_convert_at_each_common_dpi() {
        for (dpi, pixels, dips) in [(96, 8, 8.0), (120, 10, 8.0), (144, 12, 8.0), (192, 16, 8.0)] {
            assert_eq!(pixels_to_dips(pixels, dpi), dips);
        }
    }

    #[test]
    fn stroke_builder_sets_the_dash() {
        let stroke = Stroke::solid(2.0).dash(DashStyle::Dotted);
        assert_eq!((stroke.width, stroke.dash), (2.0, DashStyle::Dotted));
    }
}
