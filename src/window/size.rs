#![forbid(unsafe_code)]

//! Tracking-size limits, applied to `WM_GETMINMAXINFO` internally.

use crate::geometry::Size;
use crate::sys;
use crate::window::Window;

impl Window {
    /// Sets the smallest size the user can resize the window to, in pixels.
    ///
    /// Applied when Windows asks for `WM_GETMINMAXINFO`, so a handler does not
    /// have to. A non-positive size clears the limit.
    pub fn set_min_size(&self, size: Size) {
        sys::window_ext::set_min_size(self.hwnd(), size);
    }

    /// Sets the largest size the user can resize the window to, in pixels.
    ///
    /// Applied when Windows asks for `WM_GETMINMAXINFO`. A non-positive size
    /// clears the limit.
    pub fn set_max_size(&self, size: Size) {
        sys::window_ext::set_max_size(self.hwnd(), size);
    }
}
