#![forbid(unsafe_code)]

//! A pen handle. Dropping it releases the underlying `HPEN`.

use windows::Win32::Graphics::Gdi::{HGDIOBJ, HPEN};

use crate::color::Color;
use crate::error::Result;
use crate::sys;

/// A GDI pen.
pub struct Pen {
    handle: HPEN,
}

impl Pen {
    /// Creates a cosmetic pen `width` pixels wide.
    pub fn new(color: Color, width: i32) -> Result<Pen> {
        Ok(Pen {
            handle: sys::gdi::create_pen(color, width)?,
        })
    }

    pub(crate) fn raw(&self) -> HPEN {
        self.handle
    }
}

impl Drop for Pen {
    fn drop(&mut self) {
        sys::gdi::delete_object(HGDIOBJ(self.handle.0));
    }
}
