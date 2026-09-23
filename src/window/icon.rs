#![forbid(unsafe_code)]

//! An RAII window icon built from RGBA pixels.

use windows::Win32::UI::WindowsAndMessaging::HICON;

use crate::error::{Error, Result};
use crate::geometry::Size;
use crate::sys;
use crate::window::Window;

/// A window icon, created from RGBA pixels via `CreateIconIndirect` and
/// destroyed on drop.
///
/// Windows keeps a reference to the icon rather than copying it when it is set
/// with [`Window::set_icon`], so the `Icon` must outlive the window (store it
/// in a field, not a temporary).
pub struct Icon {
    handle: HICON,
    size: Size,
}

impl Icon {
    /// Creates an icon from tightly packed RGBA pixels (`width * height * 4`
    /// bytes, row-major, top-down). The alpha channel is honoured.
    pub fn from_rgba(width: i32, height: i32, rgba: &[u8]) -> Result<Icon> {
        if width <= 0 || height <= 0 {
            return Err(Error::Icon("size"));
        }
        let expected = width as usize * height as usize * 4;
        if rgba.len() < expected {
            return Err(Error::Icon("pixel buffer too small"));
        }
        Ok(Icon {
            handle: sys::window_icon::create_icon(width, height, rgba)?,
            size: Size::new(width, height),
        })
    }

    /// The icon's dimensions.
    pub fn size(&self) -> Size {
        self.size
    }

    pub(crate) fn raw(&self) -> HICON {
        self.handle
    }
}

impl Drop for Icon {
    fn drop(&mut self) {
        sys::window_icon::destroy_icon(self.handle);
    }
}

impl Window {
    /// Sets the window's large and small icons from `icon`.
    ///
    /// The same image backs both sizes; Windows scales it as needed. The icon
    /// must outlive the window (see [`Icon`]).
    pub fn set_icon(&self, icon: &Icon) {
        sys::window_icon::set_icon(self.hwnd(), icon.raw());
    }
}
