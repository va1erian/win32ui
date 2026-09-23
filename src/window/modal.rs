#![forbid(unsafe_code)]

//! Owned (top-level popup) windows and the modal-dialog loop.

use crate::error::Result;
use crate::geometry::Rect;
use crate::window::{Window, WindowClass, WindowExStyle, WindowHandler, WindowStyle};

impl Window {
    /// Creates a top-level window owned by `owner`.
    ///
    /// An owned window stays above its owner and is destroyed with it; the
    /// owner is not disabled. Use [`Window::run_modal`] for a modal dialog.
    pub fn create_owned<H: WindowHandler + 'static>(
        class: WindowClass,
        owner: &Window,
        style: WindowStyle,
        ex_style: WindowExStyle,
        bounds: Rect,
        title: &str,
        handler: H,
    ) -> Result<Window> {
        Window::create(
            class,
            Some(owner.hwnd()),
            style,
            ex_style,
            bounds,
            title,
            handler,
        )
    }

    /// Shows this window and runs a nested message loop until it is destroyed,
    /// with `owner` disabled for the duration — a modal dialog.
    ///
    /// Returns the loop's exit code. The owner is re-enabled and brought to the
    /// foreground afterwards, in that order: a disabled window cannot become
    /// the foreground window, and reactivating after the dialog is gone keeps
    /// the owner ahead of other applications.
    pub fn run_modal(&self, owner: &Window) -> i32 {
        owner.set_enabled(false);
        self.show();
        let code = crate::looper::run_modal(self.hwnd());
        owner.set_enabled(true);
        owner.set_foreground();
        code
    }
}
