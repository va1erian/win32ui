#![forbid(unsafe_code)]

//! Borderless fullscreen: making a top-level window cover one monitor.

use crate::error::Result;
use crate::sys;
use crate::window::{MonitorInfo, Window};

impl Window {
    /// Makes this window a borderless, topmost fullscreen window covering
    /// `monitor`'s full rectangle (not just its work area), with no caption and
    /// no DWM frame. The previous style, topmost state and placement are saved
    /// so [`leave_fullscreen`](Window::leave_fullscreen) restores them.
    ///
    /// The window stays this crate's window, so the app keeps receiving its
    /// normal messages while fullscreen — `Escape` and a double-click arrive as
    /// [`Message::KeyDown`](crate::Message::KeyDown)/
    /// [`Message::MouseDoubleClick`](crate::Message::MouseDoubleClick), and
    /// losing the focus as
    /// [`Message::Activate`](crate::Message::Activate) with `active == false`.
    /// Entering fullscreen twice just moves the window to the new monitor.
    pub fn enter_fullscreen(&self, monitor: &MonitorInfo) -> Result<()> {
        sys::fullscreen::enter(self.hwnd(), monitor.rect)
    }

    /// Leaves fullscreen and restores the window's saved style, topmost state
    /// and placement. A no-op when the window is not in fullscreen.
    pub fn leave_fullscreen(&self) -> Result<()> {
        sys::fullscreen::leave(self.hwnd())
    }

    /// Whether the window is currently in fullscreen.
    pub fn is_fullscreen(&self) -> bool {
        sys::fullscreen::is_fullscreen(self.hwnd())
    }
}
