#![forbid(unsafe_code)]

//! Rendering a window's pixels into an RGBA buffer, for screenshots and visual
//! tests.

use crate::error::Result;
use crate::geometry::Size;
use crate::sys;
use crate::window::Window;

/// A tightly packed RGBA image, row-major and top-down: `pixels` holds
/// `width * height * 4` bytes (red, green, blue, alpha per pixel).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RgbaImage {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// The pixel data, four bytes per pixel.
    pub pixels: Vec<u8>,
}

impl RgbaImage {
    /// The image dimensions.
    pub fn size(&self) -> Size {
        Size::new(self.width as i32, self.height as i32)
    }

    /// The RGBA value at `(x, y)`, or `None` if the point is outside the image.
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let offset = (y as usize * self.width as usize + x as usize) * 4;
        let pixel = self.pixels.get(offset..offset + 4)?;
        Some([pixel[0], pixel[1], pixel[2], pixel[3]])
    }
}

impl Window {
    /// Renders this window into an [`RgbaImage`].
    ///
    /// Uses `PrintWindow` with `PW_RENDERFULLCONTENT`, so the result is correct
    /// even when the window is occluded or draws with DirectComposition. The
    /// captured region is the whole window (frame included) and the alpha
    /// channel is forced to 255.
    pub fn capture(&self) -> Result<RgbaImage> {
        let size = self.window_rect().size();
        let captured = sys::capture::capture(self.hwnd(), size.width, size.height)?;
        Ok(RgbaImage {
            width: captured.width as u32,
            height: captured.height as u32,
            pixels: captured.pixels,
        })
    }
}
