#![forbid(unsafe_code)]

//! The widget-layer event mapping of an [`Edit`](super::Edit): an edit reports
//! `EN_CHANGE`, `EN_SETFOCUS` and `EN_KILLFOCUS` through `WM_COMMAND`, which
//! this turns into the app's typed message.
//!
//! The mapping is a free function so it can be unit-tested without a window:
//! the same function drives the registered `WM_COMMAND` mapper.

use crate::hwnd::Hwnd;
use crate::message::{Command, CommandNotification, Message};

// `EN_*` notification codes, from `WinUser.h`. They share their numeric values
// with other controls' `BN_*`/`CBN_*` codes, so the message's control handle
// disambiguates them.
const EN_SETFOCUS: u16 = 0x0100;
const EN_KILLFOCUS: u16 = 0x0200;
const EN_CHANGE: u16 = 0x0300;

/// Maps the edited text to the app's message.
pub(crate) type ChangeMapper<M> = Box<dyn Fn(&str) -> Option<M>>;

/// Maps a bare event (submit) to the app's message.
pub(crate) type CommandMapper<M> = Box<dyn Fn() -> Option<M>>;

/// Maps a focus change to the app's message.
pub(crate) type FocusMapper<M> = Box<dyn Fn(bool) -> Option<M>>;

/// The app-level events an edit maps to `Msg`.
pub(crate) struct EditEvents<M> {
    pub(crate) on_change: Option<ChangeMapper<M>>,
    pub(crate) on_submit: Option<CommandMapper<M>>,
    pub(crate) on_focus: Option<FocusMapper<M>>,
}

impl<M> EditEvents<M> {
    pub(crate) fn new() -> EditEvents<M> {
        EditEvents {
            on_change: None,
            on_submit: None,
            on_focus: None,
        }
    }
}

/// Maps `message` when it is an edit notification raised by `hwnd`.
///
/// Returns `None` when the message is not this edit's, and `Some(mapped)` when
/// it is: `mapped` is the app's message (`None` when no mapper was installed).
/// The `Some(None)` case still consumes the notification, so the app never
/// sees the raw `WM_COMMAND`. `text` reads the current text, and is only
/// consulted for `EN_CHANGE`.
pub(crate) fn command_message<M>(
    message: &Message,
    hwnd: Hwnd,
    text: impl FnOnce() -> String,
    events: &EditEvents<M>,
) -> Option<Option<M>> {
    let Message::Command(Command {
        control: Some(control),
        notification,
        ..
    }) = message
    else {
        return None;
    };
    if *control != hwnd {
        return None;
    }
    let mapped = match *notification {
        CommandNotification::Other(EN_CHANGE) => events.on_change.as_ref().and_then(|f| f(&text())),
        CommandNotification::Other(EN_SETFOCUS) => events.on_focus.as_ref().and_then(|f| f(true)),
        CommandNotification::Other(EN_KILLFOCUS) => events.on_focus.as_ref().and_then(|f| f(false)),
        _ => return None,
    };
    Some(mapped)
}

#[cfg(test)]
mod tests {
    use super::{EN_CHANGE, EN_KILLFOCUS, EN_SETFOCUS, EditEvents, command_message};
    use crate::hwnd::Hwnd;
    use crate::message::{Command, CommandNotification, Message};

    const EDIT: Hwnd = Hwnd::from_raw(0x7101);
    const OTHER: Hwnd = Hwnd::from_raw(0x7102);

    fn command(control: Hwnd, code: u16) -> Message {
        Message::Command(Command {
            id: 0,
            control: Some(control),
            notification: CommandNotification::Other(code),
        })
    }

    fn events() -> EditEvents<String> {
        EditEvents {
            on_change: Some(Box::new(|text| Some(format!("changed:{text}")))),
            on_submit: Some(Box::new(|| Some("submit".to_string()))),
            on_focus: Some(Box::new(|focused| {
                Some(if focused { "focus" } else { "blur" }.to_string())
            })),
        }
    }

    #[test]
    fn change_reads_the_text() {
        let mapped = command_message(
            &command(EDIT, EN_CHANGE),
            EDIT,
            || "hi".to_string(),
            &events(),
        );
        assert_eq!(mapped, Some(Some("changed:hi".to_string())));
    }

    #[test]
    fn focus_and_blur_map_to_a_flag() {
        let mapped = command_message(&command(EDIT, EN_SETFOCUS), EDIT, String::new, &events());
        assert_eq!(mapped, Some(Some("focus".to_string())));
        let mapped = command_message(&command(EDIT, EN_KILLFOCUS), EDIT, String::new, &events());
        assert_eq!(mapped, Some(Some("blur".to_string())));
    }

    #[test]
    fn other_control_is_ignored() {
        assert_eq!(
            command_message(&command(OTHER, EN_CHANGE), EDIT, String::new, &events()),
            None
        );
    }

    #[test]
    fn unrelated_notification_is_ignored() {
        assert_eq!(
            command_message(&command(EDIT, 0x0001), EDIT, String::new, &events()),
            None
        );
    }

    #[test]
    fn without_mapper_the_notification_is_still_consumed() {
        let empty: EditEvents<String> = EditEvents::new();
        assert_eq!(
            command_message(&command(EDIT, EN_CHANGE), EDIT, || "x".to_string(), &empty),
            Some(None)
        );
    }
}
