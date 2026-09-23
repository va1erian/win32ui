#![forbid(unsafe_code)]

//! Per-window activation, focus, mouse capture and cursor operations.

use crate::sys;
use crate::window::Window;

/// The cursor shown over a window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorShape {
    /// The standard arrow.
    Arrow,
    /// A pointing hand, for links and buttons.
    Hand,
    /// A text I-beam.
    IBeam,
    /// A horizontal (west-east) resize arrow.
    SizeHorizontal,
    /// A vertical (north-south) resize arrow.
    SizeVertical,
    /// A busy/hourglass indicator.
    Wait,
}

impl Window {
    /// Brings the window to the foreground, restoring it first if minimized.
    ///
    /// `SetForegroundWindow` is subject to the foreground lock: when the
    /// calling thread does not own the current foreground window, Windows may
    /// only flash the taskbar button instead of raising the window.
    pub fn set_foreground(&self) {
        sys::window_input::set_foreground(self.hwnd());
    }

    /// Enables or disables the window. A disabled window ignores keyboard and
    /// mouse input.
    pub fn set_enabled(&self, enabled: bool) {
        sys::window_input::set_enabled(self.hwnd(), enabled);
    }

    /// Whether the window is enabled.
    pub fn is_enabled(&self) -> bool {
        sys::window_input::is_enabled(self.hwnd())
    }

    /// Gives the window the keyboard focus.
    ///
    /// Only works for a window on the calling thread's input queue; bring it to
    /// the foreground first with [`Window::set_foreground`] if it is not.
    pub fn focus(&self) {
        sys::window_input::focus(self.hwnd());
    }

    /// Captures the mouse, so all mouse input goes to this window until
    /// [`Window::release_capture`] is called.
    pub fn set_capture(&self) {
        sys::window_input::set_capture(self.hwnd());
    }

    /// Releases the mouse capture, if this window holds it.
    pub fn release_capture(&self) {
        sys::window_input::release_capture();
    }

    /// Sets the cursor shown over the window.
    pub fn set_cursor(&self, shape: CursorShape) {
        sys::window_input::set_cursor(self.hwnd(), shape);
    }
}
