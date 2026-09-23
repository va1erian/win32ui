#![forbid(unsafe_code)]

//! A solid brush handle. Dropping it releases the underlying `HBRUSH`.

use windows::Win32::Graphics::Gdi::{HBRUSH, HGDIOBJ};

use crate::color::Color;
use crate::error::Result;
use crate::sys;

/// A GDI brush.
pub struct Brush {
    handle: HBRUSH,
}

impl Brush {
    /// Creates a solid brush of `color`.
    pub fn solid(color: Color) -> Result<Brush> {
        Ok(Brush {
            handle: sys::gdi::solid_brush(color)?,
        })
    }

    pub(crate) fn raw(&self) -> HBRUSH {
        self.handle
    }
}

impl Drop for Brush {
    fn drop(&mut self) {
        sys::gdi::delete_object(HGDIOBJ(self.handle.0));
    }
}
