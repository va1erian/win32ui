#![forbid(unsafe_code)]

//! A font handle. Dropping it releases the underlying `HFONT`.

use windows::Win32::Graphics::Gdi::HFONT;

use crate::error::Result;
use crate::sys;

/// Standard font weights (the `FW_*` scale).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontWeight {
    /// `FW_NORMAL` (400).
    Regular,
    /// `FW_MEDIUM` (500).
    Medium,
    /// `FW_SEMIBOLD` (600).
    Semibold,
    /// `FW_BOLD` (700).
    Bold,
}

impl FontWeight {
    /// The numeric weight Win32 expects.
    pub const fn value(self) -> i32 {
        match self {
            FontWeight::Regular => 400,
            FontWeight::Medium => 500,
            FontWeight::Semibold => 600,
            FontWeight::Bold => 700,
        }
    }
}

/// A GDI font.
pub struct Font {
    handle: HFONT,
    ascent: i32,
}

impl Font {
    /// Creates a font for `family` at `point_size` points on a `dpi` display.
    pub fn new(family: &str, point_size: f32, weight: FontWeight, dpi: u32) -> Result<Font> {
        // A negative height asks for a character height; the 96/72 factor turns
        // points into logical units at the given DPI.
        let height = -((point_size * dpi as f32 / 72.0).round() as i32).max(1);
        let handle = sys::gdi::create_font(family, height, weight.value())?;
        Ok(Font {
            handle,
            ascent: height.abs(),
        })
    }

    /// The UI font (Segoe UI) at 9.75 pt.
    pub fn system_ui(dpi: u32) -> Result<Font> {
        Font::new("Segoe UI", 9.75, FontWeight::Regular, dpi)
    }

    /// The nominal pixel height of the font.
    pub fn pixel_height(&self) -> i32 {
        self.ascent
    }

    pub(crate) fn raw(&self) -> HFONT {
        self.handle
    }
}

impl Drop for Font {
    fn drop(&mut self) {
        sys::gdi::delete_object(windows::Win32::Graphics::Gdi::HGDIOBJ(self.handle.0));
    }
}
