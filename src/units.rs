#![forbid(unsafe_code)]

//! Typed length units: [`Dip`] for design values and [`Px`] for device pixels.
//!
//! Keeping the two kinds of value apart is what stops DPI bugs: a design value
//! can only reach Win32 through [`Dip::to_px`], so it can neither be forgotten
//! nor scaled twice. [`Rect`](crate::geometry::Rect), [`Point`](crate::geometry::Point)
//! and [`Size`](crate::geometry::Size) stay in device pixels; design values are
//! converted once, at the boundary.
//!
//! ```
//! use win32ui::prelude::*;
//!
//! let margin = dip(8.0);
//! assert_eq!(margin.to_px(96), Px(8));
//! assert_eq!(margin.to_px(192), Px(16));
//! ```

use std::ops::{Add, Mul, Sub};

/// A device-independent pixel: 1/96 inch, the unit design values are written
/// in. Convert to device pixels with [`Dip::to_px`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Dip(pub f32);

/// A device pixel: what Win32 takes. Convert from design values with
/// [`Dip::to_px`], or back with [`Px::to_dip`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Px(pub i32);

/// A design value in device-independent pixels; a terse [`Dip::new`].
pub const fn dip(value: f32) -> Dip {
    Dip(value)
}

impl Dip {
    /// Creates a design value.
    pub const fn new(value: f32) -> Dip {
        Dip(value)
    }

    /// The raw value, in device-independent pixels.
    pub const fn value(self) -> f32 {
        self.0
    }

    /// Converts to device pixels at `dpi`.
    ///
    /// Rounds to the nearest whole pixel, with halves rounded away from zero
    /// (`f32::round`), so `dip(0.5).to_px(96)` is one pixel and the same input
    /// always maps to the same output.
    pub fn to_px(self, dpi: u32) -> Px {
        Px((self.0 * dpi as f32 / 96.0).round() as i32)
    }
}

impl Px {
    /// Creates a device-pixel value.
    pub const fn new(value: i32) -> Px {
        Px(value)
    }

    /// The raw value, in device pixels.
    pub const fn value(self) -> i32 {
        self.0
    }

    /// Converts to a design value at `dpi`.
    pub fn to_dip(self, dpi: u32) -> Dip {
        Dip(self.0 as f32 * 96.0 / dpi as f32)
    }
}

impl Add for Dip {
    type Output = Dip;

    fn add(self, rhs: Dip) -> Dip {
        Dip(self.0 + rhs.0)
    }
}

impl Sub for Dip {
    type Output = Dip;

    fn sub(self, rhs: Dip) -> Dip {
        Dip(self.0 - rhs.0)
    }
}

impl Mul<f32> for Dip {
    type Output = Dip;

    fn mul(self, rhs: f32) -> Dip {
        Dip(self.0 * rhs)
    }
}

impl Add for Px {
    type Output = Px;

    fn add(self, rhs: Px) -> Px {
        Px(self.0 + rhs.0)
    }
}

impl Sub for Px {
    type Output = Px;

    fn sub(self, rhs: Px) -> Px {
        Px(self.0 - rhs.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dip_rounds_to_nearest_pixel() {
        assert_eq!(dip(10.0).to_px(96), Px(10));
        assert_eq!(dip(10.0).to_px(120), Px(13));
        assert_eq!(dip(10.0).to_px(144), Px(15));
        assert_eq!(dip(10.0).to_px(192), Px(20));
    }

    #[test]
    fn dip_rounds_halves_away_from_zero() {
        assert_eq!(dip(0.5).to_px(96), Px(1));
        assert_eq!(dip(1.5).to_px(96), Px(2));
        assert_eq!(dip(-0.5).to_px(96), Px(-1));
        assert_eq!(dip(2.0).to_px(120), Px(3));
    }

    #[test]
    fn px_converts_back_to_dip() {
        assert_eq!(Px(12).to_dip(96), dip(12.0));
        assert_eq!(Px(15).to_dip(144), dip(10.0));
        assert_eq!(Px(15).to_dip(144).to_px(144), Px(15));
    }

    #[test]
    fn arithmetic_is_typed() {
        assert_eq!(dip(4.0) + dip(6.0), dip(10.0));
        assert_eq!(dip(10.0) - dip(4.0), dip(6.0));
        assert_eq!(dip(4.0) * 2.5, dip(10.0));
        assert_eq!(Px(4) + Px(6), Px(10));
        assert_eq!(Px(10) - Px(4), Px(6));
    }
}
