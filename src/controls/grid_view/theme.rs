#![forbid(unsafe_code)]

//! The derived colour palette for [`GridView`](super::GridView).

use crate::color::Color;
use crate::theme::Theme;

/// Colours for the grid's own chrome: the tile fill the widget paints behind
/// the app's `content` callback. Caption text inside a tile is the app's own
/// job, from `theme.text`/`theme.text_secondary`.
#[derive(Clone, Copy, Debug)]
pub struct GridViewTheme {
    /// The viewport background, behind every tile.
    pub background: Color,
    /// The fill painted behind the selected tile.
    pub selection: Color,
    /// The fill painted behind a hovered (not selected) tile.
    pub hover: Color,
}

impl GridViewTheme {
    /// Derives a palette from the app [`Theme`]. Override any field after
    /// calling this for a custom look.
    pub fn from_theme(theme: &Theme) -> GridViewTheme {
        GridViewTheme {
            background: theme.background,
            selection: theme.selection,
            hover: theme.hover,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::GridViewTheme;
    use crate::theme::Theme;

    #[test]
    fn follows_semantic_tokens() {
        for theme in [Theme::light(), Theme::dark()] {
            let grid = GridViewTheme::from_theme(&theme);
            assert_eq!(grid.background, theme.background);
            assert_eq!(grid.selection, theme.selection);
            assert_eq!(grid.hover, theme.hover);
        }
    }
}
