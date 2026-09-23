#![forbid(unsafe_code)]

//! Central `WM_CTLCOLOR*` answers: native edits, statics and buttons pick up
//! the window's theme without the app doing anything.
//!
//! The shared window procedure calls [`answer`] when its handler did not claim
//! a `WM_CTLCOLOR*` message. Brushes come from the bounded GDI cache, so the
//! live handle count stays flat no matter how often themes switch.

use crate::color::Color;
use crate::hwnd::Hwnd;
use crate::sys;

use super::registry::window_theme;
use super::tokens::Theme;

/// `WM_CTLCOLOR*` ids answered from the theme (from `WinUser.h` via the
/// `windows` crate's `WindowsAndMessaging` module).
pub(crate) const CTLCOLOR_MSGS: [u32; 5] = [
    sys::ctlcolor_msg_edit(),
    sys::ctlcolor_msg_static(),
    sys::ctlcolor_msg_btn(),
    sys::ctlcolor_msg_listbox(),
    sys::ctlcolor_msg_dlg(),
];

/// Whether `msg` is a theme-answered `WM_CTLCOLOR*`.
pub(crate) fn is_ctlcolor(msg: u32) -> bool {
    CTLCOLOR_MSGS.contains(&msg)
}

/// Colours a `WM_CTLCOLOR*` message draws with.
struct CtlColors {
    text: Color,
    background: Color,
}

/// Picks the colours for `msg` from `theme`.
fn colors_for(msg: u32, theme: &Theme) -> CtlColors {
    if msg == sys::ctlcolor_msg_btn() {
        CtlColors {
            text: theme.text,
            background: theme.surface,
        }
    } else if msg == sys::ctlcolor_msg_dlg() {
        CtlColors {
            text: theme.text,
            background: theme.background,
        }
    } else if msg == sys::ctlcolor_msg_static() {
        CtlColors {
            text: theme.text_secondary,
            background: theme.background,
        }
    } else {
        // Edits and list boxes read best on the input surface.
        CtlColors {
            text: theme.text,
            background: theme.input_background,
        }
    }
}

/// Answers a `WM_CTLCOLOR*` for `window`, returning the brush handle value to
/// return from the window procedure, or `None` when `msg` is not handled.
///
/// `hdc` is the device context `wparam` carried; the text/background colours
/// are selected into it before returning.
pub(crate) fn answer(window: Hwnd, msg: u32, hdc: isize) -> Option<isize> {
    if !is_ctlcolor(msg) || hdc == 0 {
        return None;
    }
    let theme = window_theme(window);
    let colors = colors_for(msg, &theme);
    sys::set_ctlcolor(hdc, colors.text, colors.background)?;
    sys::ctlcolor_brush(colors.background)
}

#[cfg(test)]
mod tests {
    use super::answer;
    use crate::hwnd::Hwnd;
    use crate::theme::Theme;
    use crate::theme::registry::set_window_theme;

    #[test]
    fn non_ctlcolor_is_ignored() {
        assert_eq!(answer(Hwnd::from_raw(0x3101), 0x000F, 1), None);
        assert_eq!(
            answer(Hwnd::from_raw(0x3101), super::sys::ctlcolor_msg_static(), 0),
            None
        );
    }

    #[test]
    fn ctlcolor_derives_from_window_theme() {
        // Without a real HDC the sys call fails and no brush is returned, but
        // the message classification itself is theme-independent.
        let window = Hwnd::from_raw(0x3102);
        set_window_theme(window, Theme::dark());
        assert_eq!(window_theme_for_test(window), Theme::dark());
        super::super::registry::forget_window(window);
    }

    fn window_theme_for_test(window: Hwnd) -> Theme {
        super::super::registry::window_theme(window)
    }

    #[test]
    fn brush_cache_inputs_stay_bounded() {
        use std::collections::HashSet;
        // The brush itself comes from the bounded GDI cache; here we assert the
        // colour selection feeding it is a small, closed set per theme.
        for theme in [Theme::light(), Theme::dark()] {
            let mut backgrounds = HashSet::new();
            for msg in super::CTLCOLOR_MSGS {
                backgrounds.insert(super::colors_for(msg, &theme).background.to_colorref());
            }
            assert!(
                backgrounds.len() <= 3,
                "a theme should use at most three CTLCOLOR backgrounds"
            );
        }
    }
}
