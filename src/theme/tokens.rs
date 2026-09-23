#![forbid(unsafe_code)]

//! Semantic colour tokens: the complete light/dark palette every control
//! paints from. See the README's *Theming* section for the model.

use crate::Color;

/// Semantic colours used by the bundled controls.
///
/// Every colour is opaque. Palettes are sampled from Windows 11's own
/// light/dark apps (Explorer chrome and address bar, Settings pages and
/// cards, WinUI text/accent values on 24H2); each constructor documents the
/// source per token group. Per-control structs
/// (`ListViewTheme::from_theme`, …) are derived, overridable views over
/// these tokens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    /// Whether this is the dark variant (drives `SetWindowTheme` and DWM).
    pub is_dark: bool,
    /// Window/panel background (Settings page: light `#F3F3F3`, dark `#202020`).
    pub background: Color,
    /// Slightly raised surface: toolbar, header, status bar (Explorer command
    /// bar: light `#F9F9F9`, dark `#2B2B2B`).
    pub surface: Color,
    /// Further raised surface: menus, popups, cards (light `#FFFFFF`, dark
    /// `#2D2D2D`).
    pub raised: Color,
    /// Primary text (WinUI TextPrimary: light `#1B1B1B`, dark `#FFFFFF`).
    pub text: Color,
    /// De-emphasised text (WinUI TextSecondary: light `#605E5C`, dark `#C7C7C7`).
    pub text_secondary: Color,
    /// Disabled text (WinUI disabled: light `#A19F9D`, dark `#767676`).
    pub text_disabled: Color,
    /// Text drawn on top of [`Theme::accent`].
    pub text_on_accent: Color,
    /// Accent used for highlighted rows and active buttons (WinUI
    /// AccentDefault: light `#005FB8`, dark `#4CC2FF`).
    pub accent: Color,
    /// Caution fill, e.g. a paused progress bar (WinUI SystemFillColorCaution:
    /// light `#9D5D00`, dark `#FCE100`).
    pub warning: Color,
    /// Critical/danger fill, e.g. an error progress bar (WinUI
    /// SystemFillColorCritical: light `#C42B1C`, dark `#FF99A4`).
    pub danger: Color,
    /// Focused selection background (Explorer selected row: light `#C7E0F4`,
    /// dark `#2B4A67`).
    pub selection: Color,
    /// Unfocused selection background (Explorer grey: light `#E5E5E5`, dark
    /// `#3A3A3A`).
    pub selection_unfocused: Color,
    /// Hover fill (Explorer hover: light `#EAEAEA`, dark `#333333`).
    pub hover: Color,
    /// Pressed fill (Explorer pressed: light `#D6D6D6`, dark `#292929`).
    pub pressed: Color,
    /// Widget border / separators (WinUI CardStroke: light `#E1E1E1`, dark
    /// `#303030`).
    pub border: Color,
    /// Focused border (accent outline: matches [`Theme::accent`]).
    pub border_focused: Color,
    /// Input background: edits, address bar (light `#FFFFFF`, dark `#2D2D2D`).
    pub input_background: Color,
    /// Scrollbar thumb (WinUI thumb: light `#C8C6C4`, dark `#605E5C`).
    pub scrollbar: Color,
    /// Scrollbar track (matches [`Theme::background`]).
    pub scrollbar_track: Color,
}

impl Theme {
    /// The light palette.
    pub const fn light() -> Theme {
        Theme {
            is_dark: false,
            background: Color::hex(0xF3_F3_F3),
            surface: Color::hex(0xF9_F9_F9),
            raised: Color::hex(0xFF_FF_FF),
            text: Color::hex(0x1B_1B_1B),
            text_secondary: Color::hex(0x60_5E_5C),
            text_disabled: Color::hex(0xA1_9F_9D),
            text_on_accent: Color::hex(0xFF_FF_FF),
            accent: Color::hex(0x00_5F_B8),
            warning: Color::hex(0x9D_5D_00),
            danger: Color::hex(0xC4_2B_1C),
            selection: Color::hex(0xC7_E0_F4),
            selection_unfocused: Color::hex(0xE5_E5_E5),
            hover: Color::hex(0xEA_EA_EA),
            pressed: Color::hex(0xD6_D6_D6),
            border: Color::hex(0xE1_E1_E1),
            border_focused: Color::hex(0x00_5F_B8),
            input_background: Color::hex(0xFF_FF_FF),
            scrollbar: Color::hex(0xC8_C6_C4),
            scrollbar_track: Color::hex(0xF3_F3_F3),
        }
    }

    /// The dark palette.
    pub const fn dark() -> Theme {
        Theme {
            is_dark: true,
            background: Color::hex(0x20_20_20),
            surface: Color::hex(0x2B_2B_2B),
            raised: Color::hex(0x2D_2D_2D),
            text: Color::hex(0xFF_FF_FF),
            text_secondary: Color::hex(0xC7_C7_C7),
            text_disabled: Color::hex(0x76_76_76),
            text_on_accent: Color::hex(0x00_00_00),
            accent: Color::hex(0x4C_C2_FF),
            warning: Color::hex(0xFC_E1_00),
            danger: Color::hex(0xFF_99_A4),
            selection: Color::hex(0x2B_4A_67),
            selection_unfocused: Color::hex(0x3A_3A_3A),
            hover: Color::hex(0x33_33_33),
            pressed: Color::hex(0x29_29_29),
            border: Color::hex(0x30_30_30),
            border_focused: Color::hex(0x4C_C2_FF),
            input_background: Color::hex(0x2D_2D_2D),
            scrollbar: Color::hex(0x60_5E_5C),
            scrollbar_track: Color::hex(0x20_20_20),
        }
    }
}

impl Default for Theme {
    /// The light theme.
    fn default() -> Theme {
        Theme::light()
    }
}

#[cfg(test)]
mod tests {
    use super::Theme;

    #[test]
    fn light_and_dark_differ_in_kind() {
        assert!(!Theme::light().is_dark);
        assert!(Theme::dark().is_dark);
        assert_ne!(Theme::light(), Theme::dark());
    }

    #[test]
    fn tracks_follow_background() {
        assert_eq!(Theme::light().scrollbar_track, Theme::light().background);
        assert_eq!(Theme::dark().scrollbar_track, Theme::dark().background);
    }

    #[test]
    fn focused_borders_match_accent() {
        assert_eq!(Theme::light().border_focused, Theme::light().accent);
        assert_eq!(Theme::dark().border_focused, Theme::dark().accent);
    }

    #[test]
    fn status_fills_differ_between_variants() {
        assert_ne!(Theme::light().warning, Theme::dark().warning);
        assert_ne!(Theme::light().danger, Theme::dark().danger);
    }
}
