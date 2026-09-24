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
    /// Selected-row background while the list has focus.
    pub selection: crate::Color,
    /// Selected-row background while the list does not have focus.
    pub selection_unfocused: crate::Color,
    /// Whether odd rows get the subtle `alternate` background. Off by
    /// default; turn on with [`ListView::zebra`](super::ListView::zebra).
    pub zebra: bool,
    /// Column-separator colour.
    pub border: crate::Color,
    /// Header background.
    pub header_background: crate::Color,
    /// Header label colour.
    pub header_text: crate::Color,
}

impl ListViewTheme {
    /// Derives a list palette from the app [`Theme`]: a subtle two-shade
    /// zebra from `background`/`text` (opt in with
    /// [`ListView::zebra`](super::ListView::zebra)), the theme's focused and
    /// unfocused selections for selected rows, plus thin column separators.
    /// Override any field after calling this for a custom look.
    pub fn from_theme(theme: &Theme) -> ListViewTheme {
        ListViewTheme {
            background: theme.background,
            alternate: theme.background.lerp(theme.text, 0.04),
            text: theme.text,
            selection: theme.selection,
            selection_unfocused: theme.selection_unfocused,
            zebra: false,
            border: theme.border,
            header_background: theme.surface,
            header_text: theme.text_secondary,
        }
    }

    /// The default `(background, text)` colours for one row, before any
    /// [`RowStyle`](super::RowStyle) override or
    /// [`row_painter`](super::ListView::row_painter) is applied. Pure, so the
    /// contrast guarantees below are unit-tested rather than eyeballed.
    ///
    /// A selected row keeps the normal text colour and only swaps its
    /// background — `selection` while the list has focus, else
    /// `selection_unfocused` — matching Explorer, where selection never
    /// re-tints the text. An unselected odd row gets the `alternate`
    /// background only when [`zebra`](ListViewTheme::zebra) is set.
    pub fn row_colors(
        &self,
        selected: bool,
        focused: bool,
        alternate: bool,
    ) -> (crate::Color, crate::Color) {
        if selected {
            let background = if focused {
                self.selection
            } else {
                self.selection_unfocused
            };
            return (background, self.text);
        }
        let background = if self.zebra && alternate {
            self.alternate
        } else {
            self.background
        };
        (background, self.text)
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
            assert_eq!(derived.selection_unfocused, theme.selection_unfocused);
            assert!(!derived.zebra, "zebra striping is opt-in");
            assert_eq!(derived.border, theme.border);
            assert_eq!(derived.header_background, theme.surface);
        }
    }

    /// Every row state keeps body-text contrast (`>= 4.5`): selected rows in
    /// particular keep the normal text colour, which is what makes them
    /// readable on both selection backgrounds.
    #[test]
    fn row_text_stays_readable_in_every_state() {
        for theme in [Theme::light(), Theme::dark()] {
            let mut palette = ListViewTheme::from_theme(&theme);
            palette.zebra = true;
            for focused in [true, false] {
                let (background, text) = palette.row_colors(true, focused, false);
                assert!(
                    text.contrast_ratio(background) >= 4.5,
                    "selected text unreadable (focused={focused}, {theme:?})",
                );
            }
            for alternate in [true, false] {
                let (background, text) = palette.row_colors(false, true, alternate);
                assert!(
                    text.contrast_ratio(background) >= 4.5,
                    "plain text unreadable (alternate={alternate}, {theme:?})",
                );
            }
        }
    }

    /// `zebra` only shades odd rows when it is turned on.
    #[test]
    fn zebra_is_opt_in() {
        let mut palette = ListViewTheme::from_theme(&Theme::light());
        let (plain, _) = palette.row_colors(false, true, true);
        assert_eq!(plain, palette.background);
        palette.zebra = true;
        let (striped, _) = palette.row_colors(false, true, true);
        assert_eq!(striped, palette.alternate);
    }
}
