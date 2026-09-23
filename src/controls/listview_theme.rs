#![forbid(unsafe_code)]

//! The derived, overridable list palette: a view over [`Theme`].

use crate::theme::Theme;

/// Colours used while owner-drawing the list.
#[derive(Clone, Copy, Debug)]
pub struct ListViewTheme {
    /// Even-row background.
    pub background: crate::Color,
    /// Odd-row background (the subtle zebra shade).
    pub alternate: crate::Color,
    /// Normal cell text.
    pub text: crate::Color,
    /// Selected-row background.
    pub selection: crate::Color,
    /// Background of the currently playing row.
    pub playing: crate::Color,
    /// Text colour on the playing/selected row.
    pub on_playing: crate::Color,
    /// Column-separator colour.
    pub border: crate::Color,
    /// Header background.
    pub header_background: crate::Color,
    /// Header label colour.
    pub header_text: crate::Color,
}

impl ListViewTheme {
    /// Derives a list palette from the app [`Theme`]: a subtle two-shade
    /// zebra from `background`/`text`, the theme's focused selection (and
    /// accent text on top) for playing/selected rows, and thin column
    /// separators. Override any field after calling this for a custom look.
    pub fn from_theme(theme: &Theme) -> ListViewTheme {
        ListViewTheme {
            background: theme.background,
            alternate: theme.background.lerp(theme.text, 0.04),
            text: theme.text,
            selection: theme.selection,
            playing: theme.accent,
            on_playing: theme.text_on_accent,
            border: theme.border,
            header_background: theme.surface,
            header_text: theme.text_secondary,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ListViewTheme;
    use crate::theme::Theme;

    #[test]
    fn derives_from_semantic_tokens() {
        for theme in [Theme::light(), Theme::dark()] {
            let derived = ListViewTheme::from_theme(&theme);
            assert_eq!(derived.background, theme.background);
            assert_eq!(derived.selection, theme.selection);
            assert_eq!(derived.playing, theme.accent);
            assert_eq!(derived.on_playing, theme.text_on_accent);
            assert_eq!(derived.border, theme.border);
            assert_eq!(derived.header_background, theme.surface);
        }
    }
}
