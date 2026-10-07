#![forbid(unsafe_code)]

//! Changing a window's shell role at run time: tool window and no-activate.
//! The initial role comes from [`WindowSpec`](super::WindowSpec).

use crate::sys;

use super::ui::Ui;

impl<M: 'static> Ui<M> {
    /// Makes the window a tool window (no taskbar button, not in Alt+Tab) or a
    /// normal one. A visible window is briefly hidden and re-shown, without
    /// activation, because the shell only reads this when a window is shown.
    /// Kept across [`Ui::enter_fullscreen`]/[`Ui::leave_fullscreen`]. See
    /// [`WindowSpec::tool_window`](super::WindowSpec::tool_window).
    pub fn set_tool_window(&self, on: bool) {
        sys::window_role::set_tool_window(self.core.hwnd(), on);
    }

    /// Whether the window is a tool window.
    pub fn is_tool_window(&self) -> bool {
        sys::window_role::is_tool_window(self.core.hwnd())
    }

    /// Makes clicking, showing or fullscreening the window never take the
    /// focus, or restores normal activation. Kept across
    /// [`Ui::enter_fullscreen`]/[`Ui::leave_fullscreen`]. See
    /// [`WindowSpec::no_activate`](super::WindowSpec::no_activate).
    pub fn set_no_activate(&self, on: bool) {
        sys::window_role::set_no_activate(self.core.hwnd(), on);
    }

    /// Whether the window never takes the focus.
    pub fn is_no_activate(&self) -> bool {
        sys::window_role::is_no_activate(self.core.hwnd())
    }
}
