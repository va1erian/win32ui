//! Pure decoding of the input messages (keyboard, mouse, focus, activation,
//! cursor, context menu, session).
//!
//! Everything here is a plain function of the raw `(msg, wparam, lparam)`
//! fields plus the already-read modifier state, so it can be unit-tested
//! without a window or a message loop. Window-dependent fix-ups (converting
//! `WM_MOUSEWHEEL`'s screen coordinates to client space) happen in the caller.

use windows::Win32::System::SystemServices::{MK_CONTROL, MK_SHIFT};
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::WindowsAndMessaging::{
    WA_INACTIVE, WM_ACTIVATE, WM_CAPTURECHANGED, WM_CONTEXTMENU, WM_ENDSESSION, WM_KEYDOWN,
    WM_KEYUP, WM_KILLFOCUS, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDBLCLK,
    WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_QUERYENDSESSION,
    WM_RBUTTONDBLCLK, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETCURSOR, WM_SETFOCUS, WM_SYSKEYDOWN,
    WM_SYSKEYUP, WM_XBUTTONDBLCLK, WM_XBUTTONDOWN, WM_XBUTTONUP, XBUTTON1, XBUTTON2,
};

use crate::geometry::Point;
use crate::message::{HitTest, Key, Message, Modifiers, MouseButton};

/// Decodes one input message, or returns `None` if `msg` is not one.
///
/// `keys` is the modifier state read before the call, via `GetKeyState`; it
/// supplies alt/win for mouse messages, since neither is reported in their
/// `wparam`. Ctrl/shift on a mouse message instead come from that message's
/// own `wparam` (`MK_CONTROL`/`MK_SHIFT`), which reflects the state at the
/// time the message was posted rather than when it is decoded. For
/// `WM_MOUSEWHEEL` / `WM_MOUSEHWHEEL`, `x`/`y` are the message's screen
/// coordinates; the caller converts them to client coordinates.
pub(crate) fn decode_input(
    msg: u32,
    wparam: usize,
    lparam: isize,
    keys: Modifiers,
) -> Option<Message> {
    let x = (lparam & 0xffff) as i16 as i32;
    let y = ((lparam >> 16) & 0xffff) as i16 as i32;

    if msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN {
        return Some(Message::KeyDown {
            key: Key::from_code(wparam as u16),
            modifiers: keys,
            repeat: (lparam & 0xffff) as u16,
            system: msg == WM_SYSKEYDOWN,
        });
    }
    if msg == WM_KEYUP || msg == WM_SYSKEYUP {
        return Some(Message::KeyUp {
            key: Key::from_code(wparam as u16),
            modifiers: keys,
            system: msg == WM_SYSKEYUP,
        });
    }
    if msg == WM_LBUTTONDOWN
        || msg == WM_RBUTTONDOWN
        || msg == WM_MBUTTONDOWN
        || msg == WM_XBUTTONDOWN
    {
        let modifiers = mouse_modifiers(wparam, keys);
        return mouse_button(msg, wparam).map(|button| Message::MouseDown {
            x,
            y,
            button,
            modifiers,
        });
    }
    if msg == WM_LBUTTONUP || msg == WM_RBUTTONUP || msg == WM_MBUTTONUP || msg == WM_XBUTTONUP {
        let modifiers = mouse_modifiers(wparam, keys);
        return mouse_button(msg, wparam).map(|button| Message::MouseUp {
            x,
            y,
            button,
            modifiers,
        });
    }
    if msg == WM_LBUTTONDBLCLK
        || msg == WM_RBUTTONDBLCLK
        || msg == WM_MBUTTONDBLCLK
        || msg == WM_XBUTTONDBLCLK
    {
        let modifiers = mouse_modifiers(wparam, keys);
        return mouse_button(msg, wparam).map(|button| Message::MouseDoubleClick {
            x,
            y,
            button,
            modifiers,
        });
    }
    if msg == WM_MOUSEMOVE {
        return Some(Message::MouseMove {
            x,
            y,
            modifiers: mouse_modifiers(wparam, keys),
        });
    }
    if msg == WM_MOUSEWHEEL || msg == WM_MOUSEHWHEEL {
        return Some(Message::MouseWheel {
            delta: ((wparam >> 16) & 0xffff) as u16 as i16,
            horizontal: msg == WM_MOUSEHWHEEL,
            x,
            y,
            modifiers: keys,
        });
    }
    if msg == WM_MOUSELEAVE {
        return Some(Message::MouseLeave);
    }
    if msg == WM_CAPTURECHANGED {
        return Some(Message::CaptureChanged);
    }
    if msg == WM_SETFOCUS {
        return Some(Message::SetFocus);
    }
    if msg == WM_KILLFOCUS {
        return Some(Message::KillFocus);
    }
    if msg == WM_ACTIVATE {
        return Some(Message::Activate {
            active: (wparam & 0xffff) != WA_INACTIVE as usize,
            minimized: (wparam >> 16) & 0xffff != 0,
        });
    }
    if msg == WM_SETCURSOR {
        return Some(Message::SetCursor {
            hit_test: HitTest::from_code((lparam & 0xffff) as u16),
        });
    }
    if msg == WM_CONTEXTMENU {
        let position = if x == -1 && y == -1 {
            None
        } else {
            Some(Point::new(x, y))
        };
        return Some(Message::ContextMenu { position });
    }
    if msg == WM_QUERYENDSESSION {
        return Some(Message::QueryEndSession);
    }
    if msg == WM_ENDSESSION {
        return Some(Message::EndSession {
            ending: wparam != 0,
        });
    }
    None
}

/// Combines the UTF-16 code units of `WM_CHAR` into a `char`, buffering a high
/// surrogate in `pending` until its low half arrives.
///
/// A high surrogate with no matching low one is dropped, and a lone low
/// surrogate becomes `U+FFFD`.
pub(crate) fn decode_char(code_unit: u16, pending: &mut Option<u16>) -> Option<char> {
    if (0xd800..=0xdbff).contains(&code_unit) {
        *pending = Some(code_unit);
        return None;
    }
    if (0xdc00..=0xdfff).contains(&code_unit) {
        let Some(high) = pending.take() else {
            return Some(char::REPLACEMENT_CHARACTER);
        };
        let code = 0x10000 + (((high as u32 - 0xd800) << 10) | (code_unit as u32 - 0xdc00));
        return char::from_u32(code);
    }
    *pending = None;
    Some(char::from_u32(code_unit as u32).unwrap_or(char::REPLACEMENT_CHARACTER))
}

/// The modifiers held for a mouse message: ctrl/shift from the message's own
/// `wparam`, alt/win carried over from `keys` (`GetKeyState`, read separately
/// since neither is reported in a mouse message's `wparam`).
fn mouse_modifiers(wparam: usize, keys: Modifiers) -> Modifiers {
    Modifiers {
        ctrl: wparam & MK_CONTROL.0 as usize != 0,
        shift: wparam & MK_SHIFT.0 as usize != 0,
        alt: keys.alt,
        win: keys.win,
    }
}

/// Maps a button message to the button it names (`None` for an unknown X
/// button).
fn mouse_button(msg: u32, wparam: usize) -> Option<MouseButton> {
    match msg {
        WM_LBUTTONDOWN | WM_LBUTTONUP | WM_LBUTTONDBLCLK => Some(MouseButton::Left),
        WM_RBUTTONDOWN | WM_RBUTTONUP | WM_RBUTTONDBLCLK => Some(MouseButton::Right),
        WM_MBUTTONDOWN | WM_MBUTTONUP | WM_MBUTTONDBLCLK => Some(MouseButton::Middle),
        WM_XBUTTONDOWN | WM_XBUTTONUP | WM_XBUTTONDBLCLK => {
            match ((wparam >> 16) & 0xffff) as u16 {
                XBUTTON1 => Some(MouseButton::X1),
                XBUTTON2 => Some(MouseButton::X2),
                _ => None,
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
