#![forbid(unsafe_code)]

//! Safe window handles: [`WindowClass`] registration, the [`Window`] wrapper
//! and the [`WindowHandler`] trait that receives typed [`Message`]s.

mod backdrop;
mod icon;
mod modal;
pub(crate) mod nc;
mod ops;
mod placement;
mod size;
mod style;
mod theme;
mod title_bar;

pub use backdrop::Backdrop;
pub use icon::Icon;
pub use ops::CursorShape;
pub use placement::{Placement, ShowState, monitor_work_areas};
pub use style::{WindowExStyle, WindowStyle};
pub use title_bar::TitleBar;

use std::sync::atomic::{AtomicU64, Ordering};

use crate::color::Color;
use crate::error::Result;
use crate::gdi::Brush;
use crate::geometry::{Rect, Size};
use crate::hwnd::Hwnd;
use crate::message::{LResult, Message, MinMaxInfo, TimerId};
use crate::sys;

/// Receives a window's messages. Return `Some(value)` to mark a message
/// handled, or `None` to fall through to `DefWindowProcW`.
///
/// The method takes `&self`: a handler can be re-entered while it is already
/// running (a Win32 call inside it can synchronously deliver another message
/// to the same window). Keep mutable state in `Cell`/`RefCell` fields and
/// borrow it only for the duration of each access.
pub trait WindowHandler {
    /// Handles one message for `window`.
    fn message(&self, window: &Window, message: Message) -> Option<LResult>;
}

/// A registered window class, consumed by [`Window::create`] to back exactly
/// one window (each registration mints a uniquely-named class). Holds the
/// class registration alive; dropping it unregisters the class, so it must
/// outlive the window created from it.
pub struct WindowClass {
    wide_name: Vec<u16>,
    _brush: Brush,
}

impl WindowClass {
    /// Registers a class with a unique atom name and `background` as its
    /// client-area brush.
    pub fn register(name: &str, background: Color) -> Result<WindowClass> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
        let unique = format!("win32ui::{name}::{sequence}");
        let mut wide_name: Vec<u16> = unique.encode_utf16().collect();
        wide_name.push(0);

        let brush = Brush::solid(background)?;
        sys::window::register_class(&wide_name, name, brush.raw())?;
        Ok(WindowClass {
            wide_name,
            _brush: brush,
        })
    }

    fn wide_name(&self) -> &[u16] {
        &self.wide_name
    }
}

impl Drop for WindowClass {
    fn drop(&mut self) {
        sys::window::unregister_class(&self.wide_name);
    }
}

/// A safe handle to a window created by this crate.
pub struct Window {
    hwnd: Hwnd,
    // Held only to keep the class registration and its brush alive; dropping
    // it unregisters the class.
    _class: Option<WindowClass>,
    owned: bool,
}

impl Window {
    /// Creates a window from a registered [`WindowClass`], taking ownership of
    /// `handler` until the window is destroyed.
    pub fn create<H: WindowHandler + 'static>(
        class: WindowClass,
        parent: Option<Hwnd>,
        style: WindowStyle,
        ex_style: WindowExStyle,
        bounds: Rect,
        title: &str,
        handler: H,
    ) -> Result<Window> {
        let params = sys::window::CreateParams {
            class_name: class.wide_name(),
            title,
            style: style.bits(),
            ex_style: ex_style.bits(),
            bounds,
            parent,
            menu: 0,
        };
        let hwnd = sys::window::create(params, handler)?;
        Ok(Window {
            hwnd: sys::hwnd_from(hwnd),
            _class: Some(class),
            owned: true,
        })
    }

    /// Wraps an existing handle for the duration of a message. Such a `Window`
    /// never destroys the OS window and carries no class registration.
    pub(crate) fn from_raw(hwnd: Hwnd) -> Window {
        Window {
            hwnd,
            _class: None,
            owned: false,
        }
    }

    /// The underlying handle.
    pub fn hwnd(&self) -> Hwnd {
        self.hwnd
    }

    /// Whether the window is still alive.
    pub fn is_alive(&self) -> bool {
        !self.hwnd.is_null() && sys::window::is_window(self.hwnd)
    }

    /// Sets the window title.
    pub fn set_title(&self, title: &str) -> Result<()> {
        sys::window::set_title(self.hwnd, title)
    }

    /// The client area, in pixels.
    pub fn client_rect(&self) -> Rect {
        sys::window::client_rect(self.hwnd)
    }

    /// The outer rectangle, in screen coordinates.
    pub fn window_rect(&self) -> Rect {
        sys::window::window_rect(self.hwnd)
    }

    /// Moves/resizes the window.
    pub fn set_bounds(&self, bounds: Rect) {
        sys::window::move_window(self.hwnd, bounds);
    }

    /// Schedules a repaint.
    pub fn invalidate(&self) {
        sys::window::invalidate(self.hwnd);
    }

    /// Shows and focuses the window.
    pub fn show(&self) {
        sys::window::show(self.hwnd, sys::window::ShowKind::Normal);
    }

    /// Shows the window with its content already painted, so it never appears
    /// with blank (white or grey) controls.
    pub(crate) fn show_painted(&self) {
        let hwnd = self.hwnd;
        sys::first_show::show_painted(hwnd, || {
            sys::window::show(hwnd, sys::window::ShowKind::Normal);
        });
    }

    /// Maximizes the window.
    pub fn show_maximized(&self) {
        sys::window::show(self.hwnd, sys::window::ShowKind::Maximized);
    }

    /// Minimizes the window.
    pub fn show_minimized(&self) {
        sys::window::show(self.hwnd, sys::window::ShowKind::Minimized);
    }

    /// Hides the window.
    pub fn hide(&self) {
        sys::window::show(self.hwnd, sys::window::ShowKind::Hidden);
    }

    /// Destroys the window. Safe to call more than once (a stale handle is a
    /// no-op on the Win32 side).
    pub fn destroy(&self) {
        sys::window::destroy(self.hwnd);
    }

    /// Starts a repeating timer.
    pub fn set_timer(&self, millis: u32) -> Result<TimerId> {
        sys::window::set_timer(self.hwnd, millis).map(TimerId)
    }

    /// Stops a timer started by [`Window::set_timer`].
    pub fn kill_timer(&self, id: TimerId) {
        sys::window::kill_timer(self.hwnd, id.0);
    }

    /// Posts this process's registered "wake" message to the window, nudging
    /// the UI loop from a worker thread.
    pub fn post_wake(&self) -> Result<()> {
        let message = sys::message::wake_message();
        sys::window::post_message(self.hwnd, message, 0, 0)
    }

    /// Posts an arbitrary message.
    pub fn post_message(&self, code: u32, wparam: usize, lparam: isize) -> Result<()> {
        sys::window::post_message(self.hwnd, code, wparam, lparam)
    }

    /// Sends an arbitrary message and waits for the result.
    pub fn send_message(&self, code: u32, wparam: usize, lparam: isize) -> isize {
        sys::window::send_message(self.hwnd, code, wparam, lparam)
    }

    /// The window's current dots-per-inch.
    pub fn dpi(&self) -> u32 {
        sys::dpi::window_dpi(self.hwnd)
    }

    /// Arms one `WM_MOUSELEAVE` notification for the next time the cursor
    /// leaves the window. Call this from a `MouseMove` handler to keep being
    /// told when the cursor leaves.
    pub fn track_mouse_leave(&self) -> Result<()> {
        sys::window::track_mouse_leave(self.hwnd)
    }

    /// The size limits reported for the [`Message::GetMinMaxInfo`] currently
    /// being handled, or `None` outside that message.
    pub fn min_max_info(&self) -> Option<MinMaxInfo> {
        sys::message::read_min_max_info()
    }

    /// Overrides the size limits for the [`Message::GetMinMaxInfo`] currently
    /// being handled. A no-op outside that message.
    pub fn set_min_max_info(&self, info: MinMaxInfo) {
        sys::message::write_min_max_info(info);
    }

    /// Records the size an owner-drawn item should report while handling the
    /// [`Message::MeasureItem`] currently being dispatched. A no-op outside
    /// that message.
    pub fn set_measured_size(&self, size: Size) {
        sys::message::set_measured_size(size);
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        if self.owned {
            self.destroy();
        }
    }
}

/// The window types a frontend usually needs.
pub mod prelude {
    pub use super::{
        Backdrop, CursorShape, Icon, Placement, ShowState, TitleBar, Window, WindowClass,
        WindowExStyle, WindowHandler, WindowStyle, monitor_work_areas,
    };
}
