#![forbid(unsafe_code)]

//! The widget-layer event mapping of a [`ComboBox`](super::ComboBox): a
//! `CBN_SELCHANGE` becomes the selected typed value, then the app's message.
//!
//! The mapping is a free function so it can be unit-tested without a window:
//! the same function drives the registered `WM_COMMAND` mapper.

use crate::controls::combobox_model::ComboBoxItems;
use crate::hwnd::Hwnd;
use crate::message::{Command, CommandNotification, Message};

/// `CBN_SELCHANGE`, from `WinUser.h`. A combo reports it in `HIWORD(wparam)` of
/// a `WM_COMMAND`; it shares its numeric value with other controls' codes, so
/// the message's control handle disambiguates it.
const CBN_SELCHANGE: u16 = 1;

/// Maps a selection change to an optional app message, given the selected
/// typed value.
pub(crate) type SelectMapper<T, M> = Box<dyn Fn(&T) -> Option<M>>;

/// The app-level events a combo box maps to `Msg`.
pub(crate) struct ComboBoxEvents<T, M> {
    pub(crate) on_select: Option<SelectMapper<T, M>>,
}

impl<T, M> ComboBoxEvents<T, M> {
    pub(crate) fn new() -> ComboBoxEvents<T, M> {
        ComboBoxEvents { on_select: None }
    }
}

/// Maps `message` when it is a `CBN_SELCHANGE` raised by `hwnd`.
///
/// Returns `None` when the message is not this combo's selection change, and
/// `Some(mapped)` when it is: `mapped` is the app's message (`None` when
/// nothing is selected or no `on_select` was installed). The `Some(None)`
/// case still consumes the notification, so the app never sees a bare index.
pub(crate) fn selection_message<T, M>(
    message: &Message,
    hwnd: Hwnd,
    selected_index: Option<usize>,
    items: &ComboBoxItems<T>,
    events: &ComboBoxEvents<T, M>,
) -> Option<Option<M>> {
    let Message::Command(Command {
        control: Some(control),
        notification,
        ..
    }) = message
    else {
        return None;
    };
    if *control != hwnd || *notification != CommandNotification::Other(CBN_SELCHANGE) {
        return None;
    }
    let mapped = selected_index
        .and_then(|index| items.value(index))
        .and_then(|value| events.on_select.as_ref().and_then(|f| f(value)));
    Some(mapped)
}

#[cfg(test)]
mod tests {
    use super::{ComboBoxEvents, selection_message};
    use crate::controls::combobox_model::ComboBoxItems;
    use crate::hwnd::Hwnd;
    use crate::message::{Command, CommandNotification, Message};

    const COMBO: Hwnd = Hwnd::from_raw(0x7001);
    const OTHER: Hwnd = Hwnd::from_raw(0x7002);

    fn items() -> ComboBoxItems<u32> {
        vec![
            ("Alpha".to_string(), 10u32),
            ("Beta".to_string(), 20),
            ("Gamma".to_string(), 30),
        ]
        .into_iter()
        .collect()
    }

    fn command(control: Hwnd, code: u16) -> Message {
        Message::Command(Command {
            id: 0,
            control: Some(control),
            notification: CommandNotification::Other(code),
        })
    }

    fn events() -> ComboBoxEvents<u32, String> {
        ComboBoxEvents {
            on_select: Some(Box::new(|value| Some(format!("picked {value}")))),
        }
    }

    #[test]
    fn selection_change_maps_index_to_value_and_msg() {
        let mapped = selection_message(&command(COMBO, 1), COMBO, Some(1), &items(), &events());
        assert_eq!(mapped, Some(Some("picked 20".to_string())));
    }

    #[test]
    fn selection_change_without_selection_is_consumed() {
        let mapped = selection_message(&command(COMBO, 1), COMBO, None, &items(), &events());
        assert_eq!(mapped, Some(None));
    }

    #[test]
    fn other_control_is_ignored() {
        assert_eq!(
            selection_message(&command(OTHER, 1), COMBO, Some(0), &items(), &events()),
            None
        );
    }

    #[test]
    fn other_notification_is_ignored() {
        assert_eq!(
            selection_message(&command(COMBO, 2), COMBO, Some(0), &items(), &events()),
            None
        );
    }

    #[test]
    fn without_mapper_the_change_is_still_consumed() {
        let empty: ComboBoxEvents<u32, String> = ComboBoxEvents::new();
        let mapped = selection_message(&command(COMBO, 1), COMBO, Some(2), &items(), &empty);
        assert_eq!(mapped, Some(None));
    }
}
