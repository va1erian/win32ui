#![forbid(unsafe_code)]

//! Decoded `WM_COMMAND` values.

use crate::hwnd::Hwnd;

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
