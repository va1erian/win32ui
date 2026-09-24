#![forbid(unsafe_code)]

//! Row appearance: the optional [`RowStyle`] overrides handed to
//! [`ListView::row_style`](super::ListView::row_style), and the [`RowState`]
//! passed to both `row_style` and
//! [`ListView::row_painter`](super::ListView::row_painter).

use crate::color::Color;

/// Optional, semantic overrides of the theme for one row.
///
/// Every field is `None` (or `false`) by default, which keeps the theme's
/// usual colours and weight; set only the fields a row needs to differ on.
/// Returned from the closure given to
/// [`ListView::row_style`](super::ListView::row_style).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RowStyle {
    /// Draws the row's text in the bold system font instead of the regular
    /// one.
    pub bold: bool,
    /// Overrides the row's text colour.
    pub text: Option<Color>,
    /// Overrides the row's background colour (before selection/zebra
    /// shading would otherwise apply).
    pub background: Option<Color>,
    /// Paints a thin vertical bar along the row's left edge in this colour —
    /// e.g. to flag an "unread" row. `None` paints no bar.
    pub accent_bar: Option<Color>,
}

impl RowStyle {
    /// The default style: theme colours, regular weight, no accent bar.
    pub fn new() -> RowStyle {
        RowStyle::default()
    }

    /// Draws the row bold (or not).
    pub fn bold(mut self, bold: bool) -> RowStyle {
        self.bold = bold;
        self
    }

    /// Overrides the row's text colour.
    pub fn text(mut self, color: Color) -> RowStyle {
        self.text = Some(color);
        self
    }

    /// Overrides the row's background colour.
    pub fn background(mut self, color: Color) -> RowStyle {
        self.background = Some(color);
        self
    }

    /// Paints a left-edge accent bar in `color`.
    pub fn accent_bar(mut self, color: Color) -> RowStyle {
        self.accent_bar = Some(color);
        self
    }
}

/// The state of one row at paint time, passed to
/// [`ListView::row_style`](super::ListView::row_style) and
/// [`ListView::row_painter`](super::ListView::row_painter).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RowState {
    /// Whether the row is selected.
    pub selected: bool,
    /// Whether the list view itself has the keyboard focus (selection is
    /// drawn differently while unfocused, matching Explorer).
    pub focused: bool,
    /// Whether the pointer is hovering the row (`CDIS_HOT`, `commctrl.h`).
    pub hot: bool,
    /// Whether this is an odd row, for zebra striping.
    pub alternate: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_style_builders_set_only_their_field() {
        let style = RowStyle::new()
            .bold(true)
            .text(Color::rgb(1, 2, 3))
            .background(Color::rgb(4, 5, 6))
            .accent_bar(Color::rgb(7, 8, 9));
        assert!(style.bold);
        assert_eq!(style.text, Some(Color::rgb(1, 2, 3)));
        assert_eq!(style.background, Some(Color::rgb(4, 5, 6)));
        assert_eq!(style.accent_bar, Some(Color::rgb(7, 8, 9)));
    }

    #[test]
    fn row_style_default_overrides_nothing() {
        let style = RowStyle::default();
        assert!(!style.bold);
        assert_eq!(style.text, None);
        assert_eq!(style.background, None);
        assert_eq!(style.accent_bar, None);
    }

    #[test]
    fn row_state_default_is_a_plain_unselected_row() {
        let state = RowState::default();
        assert!(!state.selected && !state.focused && !state.hot && !state.alternate);
    }
}
