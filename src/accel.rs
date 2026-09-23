#![forbid(unsafe_code)]

//! Keyboard shortcuts as data: a [`Shortcut`] pairs a [`Key`] with the
//! [`Modifiers`] held with it and renders its own display text, so menus and
//! tooltips show the same string the accelerator table matches.

use std::fmt;
use std::str::FromStr;

use crate::message::{Key, Modifiers};

/// A keyboard shortcut: a key plus the modifier keys held with it.
///
/// Build one with the modifier constructors ([`Shortcut::ctrl`], …) or
/// [`Shortcut::new`], then register it on a widget-layer window with
/// [`Ui::accelerator`](crate::Ui::accelerator). `Display` renders the canonical
/// text (`"Ctrl+N"`), which menus and tooltips reuse.
///
/// The Windows key is carried and displayed, but a Win32 accelerator table
/// (`ACCEL`) has no Win flag and the shell reserves `Win+…`, so a shortcut that
/// needs it is never matched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shortcut {
    key: Key,
    modifiers: Modifiers,
}

impl Shortcut {
    /// A shortcut for `key` with `modifiers`.
    pub const fn new(key: Key, modifiers: Modifiers) -> Shortcut {
        Shortcut { key, modifiers }
    }

    /// A bare key with no modifiers (e.g. `Delete`).
    pub const fn key(key: Key) -> Shortcut {
        Shortcut::new(key, Modifiers::NONE)
    }

    /// `Ctrl+key`.
    pub const fn ctrl(key: Key) -> Shortcut {
        Shortcut::new(
            key,
            Modifiers {
                ctrl: true,
                shift: false,
                alt: false,
                win: false,
            },
        )
    }

    /// `Shift+key`.
    pub const fn shift(key: Key) -> Shortcut {
        Shortcut::new(
            key,
            Modifiers {
                ctrl: false,
                shift: true,
                alt: false,
                win: false,
            },
        )
    }

    /// `Alt+key`.
    pub const fn alt(key: Key) -> Shortcut {
        Shortcut::new(
            key,
            Modifiers {
                ctrl: false,
                shift: false,
                alt: true,
                win: false,
            },
        )
    }

    /// `Win+key`.
    pub const fn win(key: Key) -> Shortcut {
        Shortcut::new(
            key,
            Modifiers {
                ctrl: false,
                shift: false,
                alt: false,
                win: true,
            },
        )
    }

    /// Adds Ctrl to the shortcut.
    pub const fn with_ctrl(mut self) -> Shortcut {
        self.modifiers.ctrl = true;
        self
    }

    /// Adds Shift to the shortcut.
    pub const fn with_shift(mut self) -> Shortcut {
        self.modifiers.shift = true;
        self
    }

    /// Adds Alt to the shortcut.
    pub const fn with_alt(mut self) -> Shortcut {
        self.modifiers.alt = true;
        self
    }

    /// Adds the Windows key to the shortcut.
    pub const fn with_win(mut self) -> Shortcut {
        self.modifiers.win = true;
        self
    }

    /// The shortcut's key.
    pub const fn key_code(self) -> Key {
        self.key
    }

    /// The modifiers held with the key.
    pub const fn modifiers(self) -> Modifiers {
        self.modifiers
    }
}

impl fmt::Display for Shortcut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let modifiers = self.modifiers;
        if modifiers.ctrl {
            f.write_str("Ctrl+")?;
        }
        if modifiers.shift {
            f.write_str("Shift+")?;
        }
        if modifiers.alt {
            f.write_str("Alt+")?;
        }
        if modifiers.win {
            f.write_str("Win+")?;
        }
        f.write_str(&key_name(self.key))
    }
}

/// Returned when a string is not a valid [`Shortcut`].
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is not a valid shortcut")]
pub struct ShortcutParseError(String);

impl FromStr for Shortcut {
    type Err = ShortcutParseError;

    /// Parses text like `"Ctrl+Shift+S"` or `"F5"`. Modifier names and key
    /// names are case-insensitive; the canonical names are the ones
    /// [`Display`](fmt::Display) produces.
    fn from_str(text: &str) -> Result<Shortcut, ShortcutParseError> {
        let invalid = || ShortcutParseError(text.to_string());
        let mut modifiers = Modifiers::NONE;
        let mut key = None;
        for part in text.split('+') {
            let part = part.trim();
            if part.is_empty() {
                return Err(invalid());
            }
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => modifiers.ctrl = true,
                "shift" => modifiers.shift = true,
                "alt" => modifiers.alt = true,
                "win" | "windows" | "super" | "meta" => modifiers.win = true,
                _ if key.is_none() => key = key_from_token(part),
                _ => return Err(invalid()),
            }
        }
        key.map(|key| Shortcut::new(key, modifiers))
            .ok_or_else(invalid)
    }
}

/// The `(key, canonical name, aliases…)` table shared by display and parsing.
///
/// Key names are display names, not Win32 identifiers; the first name is the
/// canonical one. `VK_*` codes come from the `Key` constants (the `windows`
/// crate), never from memory.
const KEY_NAMES: &[(Key, &[&str])] = &[
    (Key::A, &["A"]),
    (Key::B, &["B"]),
    (Key::C, &["C"]),
    (Key::D, &["D"]),
    (Key::E, &["E"]),
    (Key::F, &["F"]),
    (Key::G, &["G"]),
    (Key::H, &["H"]),
    (Key::I, &["I"]),
    (Key::J, &["J"]),
    (Key::K, &["K"]),
    (Key::L, &["L"]),
    (Key::M, &["M"]),
    (Key::N, &["N"]),
    (Key::O, &["O"]),
    (Key::P, &["P"]),
    (Key::Q, &["Q"]),
    (Key::R, &["R"]),
    (Key::S, &["S"]),
    (Key::T, &["T"]),
    (Key::U, &["U"]),
    (Key::V, &["V"]),
    (Key::W, &["W"]),
    (Key::X, &["X"]),
    (Key::Y, &["Y"]),
    (Key::Z, &["Z"]),
    (Key::DIGIT0, &["0"]),
    (Key::DIGIT1, &["1"]),
    (Key::DIGIT2, &["2"]),
    (Key::DIGIT3, &["3"]),
    (Key::DIGIT4, &["4"]),
    (Key::DIGIT5, &["5"]),
    (Key::DIGIT6, &["6"]),
    (Key::DIGIT7, &["7"]),
    (Key::DIGIT8, &["8"]),
    (Key::DIGIT9, &["9"]),
    (Key::F1, &["F1"]),
    (Key::F2, &["F2"]),
    (Key::F3, &["F3"]),
    (Key::F4, &["F4"]),
    (Key::F5, &["F5"]),
    (Key::F6, &["F6"]),
    (Key::F7, &["F7"]),
    (Key::F8, &["F8"]),
    (Key::F9, &["F9"]),
    (Key::F10, &["F10"]),
    (Key::F11, &["F11"]),
    (Key::F12, &["F12"]),
    (Key::BACK, &["Backspace", "Back"]),
    (Key::TAB, &["Tab"]),
    (Key::RETURN, &["Enter", "Return"]),
    (Key::ESCAPE, &["Esc", "Escape"]),
    (Key::SPACE, &["Space"]),
    (Key::DELETE, &["Del", "Delete"]),
    (Key::INSERT, &["Ins", "Insert"]),
    (Key::HOME, &["Home"]),
    (Key::END, &["End"]),
    (Key::PAGE_UP, &["PgUp", "PageUp", "Page_Up"]),
    (Key::PAGE_DOWN, &["PgDn", "PageDown", "Page_Down"]),
    (Key::LEFT, &["Left"]),
    (Key::RIGHT, &["Right"]),
    (Key::UP, &["Up"]),
    (Key::DOWN, &["Down"]),
];

/// The canonical display name of `key`, or its raw code for keys the table does
/// not name.
fn key_name(key: Key) -> String {
    KEY_NAMES
        .iter()
        .find(|(candidate, _)| *candidate == key)
        .map(|(_, names)| names[0].to_string())
        .unwrap_or_else(|| format!("VK{:02X}", key.code()))
}

/// The key whose name (or alias) is `token`, case-insensitively.
fn key_from_token(token: &str) -> Option<Key> {
    KEY_NAMES
        .iter()
        .find(|(_, names)| names.iter().any(|name| name.eq_ignore_ascii_case(token)))
        .map(|(key, _)| *key)
}

/// The accelerator types a frontend usually needs.
pub mod prelude {
    pub use super::{Shortcut, ShortcutParseError};
}
