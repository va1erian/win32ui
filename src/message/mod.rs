#![forbid(unsafe_code)]

//! Typed window messages.
//!
//! [`crate::sys`] decodes the raw `(msg, WPARAM, LPARAM)` triple into the
//! [`Message`] enum below; [`WindowHandler`](crate::WindowHandler)
//! implementations match on it. `Message::Other` is the escape hatch for
//! anything not modelled yet.

mod command;
mod input;
mod notify;

pub use command::{Command, CommandNotification};
pub use input::{HitTest, Key, Modifiers, MouseButton};
pub use notify::{Notify, TimerId};

use crate::geometry::{Point, Rect, Size};
use crate::hwnd::Hwnd;

/// A window-procedure return value.
pub type LResult = isize;

/// The size and position limits a window reports for `WM_GETMINMAXINFO`.
///
/// Returned by [`Window::min_max_info`](crate::Window::min_max_info) and
/// accepted by [`Window::set_min_max_info`](crate::Window::set_min_max_info);
/// only meaningful while handling [`Message::GetMinMaxInfo`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MinMaxInfo {
    /// `ptMaxSize`: the maximised size.
    pub max_size: Size,
    /// `ptMaxPosition`: the position of a maximised window.
    pub max_position: Point,
    /// `ptMinTrackSize`: the smallest size the window can be resized to.
    pub min_track_size: Size,
    /// `ptMaxTrackSize`: the largest size the window can be resized to.
    pub max_track_size: Size,
}

/// A decoded window message.
#[derive(Clone, Debug)]
pub enum Message {
    /// `WM_CREATE`: the window has been created but isn't visible yet.
    Create,
    /// `WM_DESTROY`: the window is being torn down. `WM_NCDESTROY` is never
    /// delivered here — it is consumed internally to reclaim the handler.
    Destroy,
    /// `WM_CLOSE`: the user or code asked to close the window.
    Close,
    /// `WM_PAINT`: the window should repaint itself.
    Paint,
    /// `WM_SIZE`: the client area changed.
    Size {
        /// New client width.
        width: i32,
        /// New client height.
        height: i32,
    },
    /// `WM_DPICHANGED`: the window moved to a monitor with a different DPI.
    DpiChanged {
        /// The new dots per inch.
        dpi: u32,
        /// The rectangle Windows suggests the window occupy.
        suggested: Rect,
    },
    /// `WM_TIMER`.
    Timer {
        /// The timer that fired.
        id: TimerId,
    },
    /// The process-registered "wake" message: a worker has new data.
    Wake,
    /// `WM_COMMAND`.
    Command(Command),
    /// `WM_NOTIFY`.
    Notify(Notify),
    /// `WM_DRAWITEM`: an owner-drawn control needs painting. The device
    /// context is only valid while handling this message; return `Some(1)`
    /// (TRUE) once drawn.
    DrawItem {
        /// The control that needs painting.
        control: Hwnd,
        /// The control's id.
        id: usize,
        /// `itemID`: the menu command id, or the control's item index.
        item: usize,
        /// `itemData`: the application value set when the item was added. For
        /// owner-drawn menus this is win32ui's own render id.
        data: usize,
        /// Whether this is an owner-drawn *menu* item (`CtlType == ODT_MENU`).
        menu: bool,
        /// `itemAction` (`ODA_*` from `Winuser.h`): why painting was requested.
        /// Owner-drawn widgets usually repaint unconditionally.
        action: u32,
        /// `itemState` (`ODS_*` from `Winuser.h`): selected, focused,
        /// disabled and similar flags. Interpret it with `sys` helpers so
        /// callers never name the raw codes.
        state: u32,
        /// The device context to paint into, as a raw value.
        dc: isize,
        /// The rectangle to paint, in the control's client coordinates.
        area: Rect,
    },
    /// `WM_MEASUREITEM`: an owner-drawn control (or menu) is asked how large one
    /// item should be. Only meaningful while handling it; write the size back
    /// with [`Window::set_measured_size`](crate::Window::set_measured_size).
    MeasureItem {
        /// The control id, or `0` for a menu.
        id: usize,
        /// `itemID`: the menu command id, or the control's item index.
        item: usize,
        /// `itemData`: the application value set when the item was added.
        data: usize,
        /// Whether this is an owner-drawn *menu* item (`CtlType == ODT_MENU`).
        menu: bool,
    },
    /// `WM_KEYDOWN` / `WM_SYSKEYDOWN`.
    KeyDown {
        /// The virtual key pressed.
        key: Key,
        /// Which modifiers were held.
        modifiers: Modifiers,
        /// Auto-repeat count (`1` on the first press).
        repeat: u16,
        /// Whether this came from a system key (`WM_SYSKEYDOWN`, i.e. an
        /// Alt combination).
        system: bool,
    },
    /// `WM_KEYUP` / `WM_SYSKEYUP`.
    KeyUp {
        /// The virtual key released.
        key: Key,
        /// Which modifiers were held.
        modifiers: Modifiers,
        /// Whether this came from a system key (`WM_SYSKEYUP`).
        system: bool,
    },
    /// `WM_CHAR`: a translated character. UTF-16 surrogate pairs from two
    /// messages are combined into one `char`.
    Char(char),
    /// A left/right/middle/extra button went down.
    MouseDown {
        /// Cursor x in client coordinates.
        x: i32,
        /// Cursor y in client coordinates.
        y: i32,
        /// Which button.
        button: MouseButton,
    },
    /// A mouse button was released.
    MouseUp {
        /// Cursor x in client coordinates.
        x: i32,
        /// Cursor y in client coordinates.
        y: i32,
        /// Which button.
        button: MouseButton,
    },
    /// The cursor moved over the window.
    MouseMove {
        /// Cursor x in client coordinates.
        x: i32,
        /// Cursor y in client coordinates.
        y: i32,
    },
    /// A mouse button was double-clicked. The window class must be registered
    /// with `CS_DBLCLKS`, which [`WindowClass`](crate::WindowClass) does.
    MouseDoubleClick {
        /// Cursor x in client coordinates.
        x: i32,
        /// Cursor y in client coordinates.
        y: i32,
        /// Which button.
        button: MouseButton,
    },
    /// `WM_MOUSEWHEEL` / `WM_MOUSEHWHEEL`: the wheel was rolled.
    MouseWheel {
        /// Wheel rotation, in multiples of `WHEEL_DELTA` (`120`); positive is
        /// away from the user / to the right.
        delta: i16,
        /// Whether this is a horizontal wheel (`WM_MOUSEHWHEEL`).
        horizontal: bool,
        /// Cursor x in client coordinates (converted from the message's screen
        /// coordinates).
        x: i32,
        /// Cursor y in client coordinates.
        y: i32,
        /// Which modifiers were held.
        modifiers: Modifiers,
    },
    /// `WM_MOUSELEAVE`: the cursor left the window after
    /// [`Window::track_mouse_leave`](crate::Window::track_mouse_leave) armed
    /// tracking.
    MouseLeave,
    /// `WM_CAPTURECHANGED`: another window took the mouse capture.
    CaptureChanged,
    /// `WM_SETFOCUS`: the window gained the keyboard focus.
    SetFocus,
    /// `WM_KILLFOCUS`: the window lost the keyboard focus.
    KillFocus,
    /// `WM_ACTIVATE`: the window was activated or deactivated.
    Activate {
        /// Whether the window is now active.
        active: bool,
        /// Whether it is minimised.
        minimized: bool,
    },
    /// `WM_SETCURSOR`: set the cursor for `hit_test`. Return `Some(0)` from the
    /// handler to keep a custom cursor.
    SetCursor {
        /// What the cursor is over.
        hit_test: HitTest,
    },
    /// `WM_CONTEXTMENU`: the context menu was requested.
    ContextMenu {
        /// The position in screen coordinates, or `None` for a keyboard-invoked
        /// menu (the raw `(-1, -1)`).
        position: Option<Point>,
    },
    /// `WM_GETMINMAXINFO`: the window is asked for its size limits. Read them
    /// with [`Window::min_max_info`](crate::Window::min_max_info) and change
    /// them with [`Window::set_min_max_info`](crate::Window::set_min_max_info).
    GetMinMaxInfo,
    /// `WM_SETTINGCHANGE`: a system setting changed. `section` is the area that
    /// changed (e.g. `"Environment"`), when the sender provided one.
    SettingChange {
        /// The section name, or `None` for a settings-area-only change.
        section: Option<String>,
    },
    /// `WM_QUERYENDSESSION`: Windows is asking whether it may end the session.
    QueryEndSession,
    /// `WM_ENDSESSION`: the session is ending.
    EndSession {
        /// Whether the session is actually ending (`false` if the shutdown was
        /// cancelled).
        ending: bool,
    },
    /// Any message not modelled above, with its raw fields.
    Other {
        /// The raw message id.
        code: u32,
        /// The raw `wparam`.
        wparam: usize,
        /// The raw `lparam`.
        lparam: isize,
    },
}

impl Message {
    /// Whether this message is a plain repaint/layout message that a control
    /// can usually ignore.
    pub const fn is_invalidation(&self) -> bool {
        matches!(self, Message::Paint | Message::Size { .. })
    }
}

/// The typed-message vocabulary a frontend usually needs.
pub mod prelude {
    pub use super::{
        Command, CommandNotification, HitTest, Key, LResult, Message, MinMaxInfo, Modifiers,
        MouseButton, Notify, TimerId,
    };
}
