#![forbid(unsafe_code)]

//! Cursor visibility while the pointer rests inside a window.

use crate::error::Result;
use crate::sys;
use crate::window::Window;

impl Window {
    /// Hides the mouse cursor after `millis` without a move while the pointer
    /// is over this window, and shows it again on the next move. Call `0` to
    /// disable idle hiding and restore the cursor.
    ///
    /// Useful for a fullscreen visualization or a media player: the pointer
    /// disappears while the user is idle and comes back as soon as they move it.
    /// The state is per-window and dropped when the window is destroyed.
    pub fn hide_cursor_when_idle(&self, millis: u32) -> Result<()> {
        sys::cursor_idle::arm(self.hwnd(), millis)
    }
}
