#![forbid(unsafe_code)]

//! The [`Themed`] trait every bundled widget implements.

use super::tokens::Theme;

/// A widget that re-themes live from semantic tokens.
///
/// Applying a theme re-derives the widget's colours, updates its native
/// parts (`SetWindowTheme`, scrollbars) and invalidates it. Nothing is
/// recreated.
pub trait Themed {
    /// Applies `theme` to the widget.
    fn apply_theme(&self, theme: &Theme);
}
