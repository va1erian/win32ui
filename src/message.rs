#![forbid(unsafe_code)]

//! Typed window messages.
//!
//! [`crate::sys`] decodes the raw `(msg, WPARAM, LPARAM)` triple into the
//! [`Message`] enum below; [`WindowHandler`](crate::WindowHandler)
//! implementations match on it. `Message::Other` is the escape hatch for
//! anything not modelled yet.

use crate::controls::listview::ListViewEvent;
use crate::controls::treeview::TreeViewEvent;
use crate::geometry::Rect;
use crate::hwnd::Hwnd;

/// A window-procedure return value.
pub type LResult = isize;

/// A `WM_COMMAND` notification code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandNotification {
    /// `BN_CLICKED`: a button (or the [`Toolbar`](crate::Toolbar)) was clicked.
    Clicked,
    /// `BN_DOUBLECLICKED`.
    DoubleClicked,
    /// `BN_SETFOCUS`.
    SetFocus,
    /// `BN_KILLFOCUS`.
    KillFocus,
    /// Any other code, passed through unchanged.
    Other(u16),
}

impl CommandNotification {
    /// Maps a raw `HIWORD(wparam)` value, preferring the button codes (which
    /// are shared by menu-less controls).
    pub(crate) const fn from_code(code: u16) -> CommandNotification {
        match code {
            0 => CommandNotification::Clicked,
            5 => CommandNotification::DoubleClicked,
            6 => CommandNotification::SetFocus,
            7 => CommandNotification::KillFocus,
            other => CommandNotification::Other(other),
        }
    }
}

/// A decoded `WM_COMMAND`.
#[derive(Clone, Copy, Debug)]
pub struct Command {
    /// Control or menu identifier (`LOWORD(wparam)`).
    pub id: u16,
    /// The control that raised the notification, if any.
    pub control: Option<Hwnd>,
    /// What happened.
    pub notification: CommandNotification,
}

/// A notification routed from a child control (`WM_NOTIFY`).
#[derive(Clone, Copy, Debug)]
pub enum Notify {
    /// A [`ListView`](crate::ListView) event.
    ListView {
        /// The list view's control id.
        id: usize,
        /// What happened.
        event: ListViewEvent,
    },
    /// A [`TreeView`](crate::TreeView) event.
    TreeView {
        /// The tree view's control id.
        id: usize,
        /// What happened.
        event: TreeViewEvent,
    },
    /// A notification we don't model, left for the caller to interpret.
    Other {
        /// The control's id.
        id: usize,
        /// The raw `NMHDR.code`.
        code: u32,
        /// The control that sent it.
        hwnd: Hwnd,
    },
}

/// A mouse button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    /// Left button.
    Left,
    /// Right button.
    Right,
    /// Middle button.
    Middle,
}

/// A `SetTimer` identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TimerId(pub usize);

/// A decoded window message.
#[derive(Clone, Copy, Debug)]
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
    /// A left/right/middle button went down.
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
