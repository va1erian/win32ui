#![forbid(unsafe_code)]

//! The toolbar's derived palette.

use crate::color::Color;
use crate::theme::Theme;

/// Colours for the toolbar.
#[derive(Clone, Copy, Debug)]
pub struct ToolbarTheme {
    /// Toolbar background.
    pub background: Color,
    /// Idle button background.
    pub button: Color,
    /// Hovered button background.
    pub button_hover: Color,
    /// Pressed button background.
    pub button_pressed: Color,
    /// A checked toggle's background.
    pub button_checked: Color,
    /// Button label and icon colour.
    pub text: Color,
    /// A disabled button's label and icon colour.
    pub text_disabled: Color,
    /// A checked toggle's label and icon colour.
    pub text_on_accent: Color,
    /// The toolbar's bottom border.
    pub border: Color,
    /// The separator rule.
    pub separator: Color,
}

impl ToolbarTheme {
    /// Derives a toolbar palette from the app [`Theme`]. Override any field
    /// after calling this for a custom look.
    pub fn from_theme(theme: &Theme) -> ToolbarTheme {
        ToolbarTheme {
            background: theme.surface,
            button: theme.surface,
            button_hover: theme.hover,
            button_pressed: theme.pressed,
            button_checked: theme.accent,
            text: theme.text,
            text_disabled: theme.text_disabled,
            text_on_accent: theme.text_on_accent,
            border: theme.border,
            separator: theme.border,
        }
    }
}
