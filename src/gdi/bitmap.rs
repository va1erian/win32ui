#![forbid(unsafe_code)]

//! A DIB-section bitmap, typically uploaded from RGBA pixels (icons, images)
//! and blitted during a paint.

use windows::Win32::Graphics::Gdi::{HBITMAP, HGDIOBJ};

use crate::error::{Error, Result};
use crate::geometry::Size;
use crate::sys;

/// A 32-bit top-down bitmap.
pub struct Bitmap {
    handle: HBITMAP,
    size: Size,
}

impl Bitmap {
    /// Creates a bitmap from tightly packed RGBA pixels (`width * height * 4`
    /// bytes, row-major, top-down).
    pub fn from_rgba(width: i32, height: i32, rgba: &[u8]) -> Result<Bitmap> {
        if width <= 0 || height <= 0 {
            return Err(Error::Gdi("bitmap size"));
        }
        let expected = width as usize * height as usize * 4;
        if rgba.len() < expected {
            return Err(Error::Gdi("bitmap pixel buffer too small"));
        }
        Ok(Bitmap {
            handle: sys::gdi::create_dib(width, height, rgba)?,
            size: Size::new(width, height),
        })
    }

    /// The bitmap's dimensions.
    pub fn size(&self) -> Size {
        self.size
    }

    pub(crate) fn raw(&self) -> HBITMAP {
        self.handle
    }
}

impl Drop for Bitmap {
    fn drop(&mut self) {
        sys::gdi::delete_object(HGDIOBJ(self.handle.0));
    }
}
