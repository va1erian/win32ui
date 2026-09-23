#![forbid(unsafe_code)]

//! Live theme switching for platform-layer windows.

use crate::sys;
use crate::theme::Theme;
use crate::window::Window;

impl Window {
    /// Switches the window to `theme`, live.
    ///
    /// Stores the theme for central `WM_CTLCOLOR*` answers, applies the DWM
    /// dark title bar, re-themes every registered child and repaints once.
    /// Controls created under the window adopt the theme automatically;
    /// nothing needs recreating.
    pub fn set_theme(&self, theme: Theme) {
        crate::theme::set_window_theme(self.hwnd(), theme);
        sys::set_titlebar_dark(self.hwnd(), theme.is_dark);
        sys::set_class_background(self.hwnd(), theme.background);
        crate::theme::retheme_children(self.hwnd(), &theme);
        sys::window::invalidate(self.hwnd());
    }

    /// The window's current theme, or [`Theme::light`] when none was set.
    pub fn theme(&self) -> Theme {
        crate::theme::window_theme(self.hwnd())
    }
}
