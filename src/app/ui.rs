#![forbid(unsafe_code)]

//! The [`Ui`] handle: the widget layer's view of the top-level window.

use std::rc::Rc;

use crate::capture::RgbaImage;
use crate::error::Result;
use crate::geometry::Rect;
use crate::hwnd::Hwnd;
use crate::message::TimerId;
use crate::sys;

use super::core::Core;

/// The widget-layer handle to the top-level window.
///
/// It creates widgets (their constructors take it), sets the title, closes the
/// window, quits the loop, and is where later issues hang theme, layout and
/// menus. It is cheap to clone (an `Rc`), so a widget can keep one to enqueue
/// messages from its own event handlers.
pub struct Ui<M> {
    core: Rc<Core<M>>,
}

impl<M: 'static> Ui<M> {
    pub(crate) fn new(core: Rc<Core<M>>) -> Ui<M> {
        Ui { core }
    }

    /// The top-level window's handle.
    pub fn hwnd(&self) -> Hwnd {
        self.core.hwnd()
    }

    /// The window's dots-per-inch.
    pub fn dpi(&self) -> u32 {
        sys::dpi::window_dpi(self.core.hwnd())
    }

    /// The client area, in device pixels.
    pub fn client_rect(&self) -> Rect {
        sys::window::client_rect(self.core.hwnd())
    }

    /// The outer rectangle, in screen coordinates.
    pub fn window_rect(&self) -> Rect {
        sys::window::window_rect(self.core.hwnd())
    }

    /// Sets the window title.
    pub fn set_title(&self, title: &str) {
        let _ = sys::window::set_title(self.core.hwnd(), title);
    }

    /// Enqueues `msg` for delivery to [`App::update`](super::App::update). This
    /// is how custom widgets hand events back to the application.
    pub fn emit(&self, msg: M) {
        self.core.enqueue(msg);
    }

    /// Intercepts the window close request. Returning `Some(msg)` enqueues it
    /// and lets the app decide; returning `None` (the default) closes the
    /// window and quits.
    pub fn on_close(&self, f: impl Fn() -> Option<M> + 'static) {
        self.core.set_on_close(f);
    }

    /// Maps a `WM_TIMER` tick to a message. Only one mapping can be installed.
    pub fn on_timer(&self, f: impl Fn(TimerId) -> Option<M> + 'static) {
        self.core.set_on_timer(f);
    }

    /// Starts a repeating timer and returns its id.
    pub fn set_timer(&self, millis: u32) -> Result<TimerId> {
        sys::window::set_timer(self.core.hwnd(), millis).map(TimerId)
    }

    /// Stops a timer started by [`Ui::set_timer`].
    pub fn kill_timer(&self, id: TimerId) {
        sys::window::kill_timer(self.core.hwnd(), id.0);
    }

    /// Closes the window and ends the message loop.
    pub fn close(&self) {
        sys::window::destroy(self.core.hwnd());
        crate::looper::quit(0);
    }

    /// Ends the message loop.
    pub fn quit(&self) {
        crate::looper::quit(0);
    }

    /// Ends the message loop with a specific exit code.
    pub fn quit_with(&self, code: i32) {
        crate::looper::quit(code);
    }

    /// Renders the window into an image, for screenshots.
    pub fn capture(&self) -> Result<RgbaImage> {
        let size = self.window_rect().size();
        let captured = sys::capture::capture(self.core.hwnd(), size.width, size.height)?;
        Ok(RgbaImage {
            width: captured.width as u32,
            height: captured.height as u32,
            pixels: captured.pixels,
        })
    }
}

impl<M> Clone for Ui<M> {
    fn clone(&self) -> Ui<M> {
        Ui {
            core: Rc::clone(&self.core),
        }
    }
}
