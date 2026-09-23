#![forbid(unsafe_code)]

//! An RGB colour, convertible to a Win32 `COLORREF` (`0x00BBGGRR`).

/// An opaque 24-bit colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Color {
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8,
}

impl Color {
    /// Creates a colour from its channels.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color { r, g, b }
    }

    /// A colour from a `0xRRGGBB` literal (the familiar web/hex order).
    pub const fn hex(rgb: u32) -> Color {
        Color {
            r: ((rgb >> 16) & 0xff) as u8,
            g: ((rgb >> 8) & 0xff) as u8,
            b: (rgb & 0xff) as u8,
        }
    }

    /// The Win32 `COLORREF` value: `0x00BBGGRR`.
    pub const fn to_colorref(self) -> u32 {
        (self.b as u32) << 16 | (self.g as u32) << 8 | self.r as u32
    }

    /// Builds a colour from a Win32 `COLORREF`.
    pub const fn from_colorref(value: u32) -> Color {
        Color {
            r: (value & 0xff) as u8,
            g: ((value >> 8) & 0xff) as u8,
            b: ((value >> 16) & 0xff) as u8,
        }
    }

    /// Blends towards `other` by `t` in `0.0..=1.0` (used for hover states).
    pub fn lerp(self, other: Color, t: f32) -> Color {
        let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Color {
            r: mix(self.r, other.r),
            g: mix(self.g, other.g),
            b: mix(self.b, other.b),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Color;

    #[test]
    fn colorref_is_bgr() {
        assert_eq!(Color::rgb(0x12, 0x34, 0x56).to_colorref(), 0x0056_3412);
        assert_eq!(Color::hex(0x56_34_12), Color::rgb(0x56, 0x34, 0x12));
    }

    #[test]
    fn blend_endpoints() {
        let black = Color::rgb(0, 0, 0);
        let white = Color::rgb(255, 255, 255);
        assert_eq!(black.lerp(white, 0.0), black);
        assert_eq!(black.lerp(white, 1.0), white);
        assert_eq!(black.lerp(white, 0.5), Color::rgb(128, 128, 128));
    }
}
