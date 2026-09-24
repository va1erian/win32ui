#![forbid(unsafe_code)]

//! Per-node appearance: [`NodeStyle`], returned from the closure given to
//! [`TreeView::style`](super::TreeView::style).

use crate::color::Color;

/// Optional, semantic overrides of the theme for one node.
///
/// Every field is optional, so a node that returns [`NodeStyle::default`]
/// keeps the theme's usual look. The style is re-read on every paint, so
/// changing it (via a new closure) is enough — no native update needed for the
/// text or badge.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NodeStyle {
    /// Draws the node's text in the bold system font.
    pub bold: bool,
    /// Trailing badge text, right-aligned in the row, e.g. an unread count.
    pub badge: Option<String>,
    /// Index into the tree's [`ImageList`](crate::ImageList) to show before
    /// the text.
    pub icon: Option<usize>,
    /// Overrides the node's text colour.
    pub text: Option<Color>,
    /// Overrides the node's background colour (selection and hover still win
    /// over it while they apply).
    pub background: Option<Color>,
}

impl NodeStyle {
    /// The default style: theme colours, regular weight, no badge, no icon.
    pub fn new() -> NodeStyle {
        NodeStyle::default()
    }

    /// Draws the node bold (or not).
    pub fn bold(mut self, bold: bool) -> NodeStyle {
        self.bold = bold;
        self
    }

    /// Shows `badge` at the row's trailing edge.
    pub fn badge(mut self, badge: impl Into<String>) -> NodeStyle {
        self.badge = Some(badge.into());
        self
    }

    /// Shows image `index` from the tree's image list before the text.
    pub fn icon(mut self, index: usize) -> NodeStyle {
        self.icon = Some(index);
        self
    }

    /// Overrides the node's text colour.
    pub fn text(mut self, color: Color) -> NodeStyle {
        self.text = Some(color);
        self
    }

    /// Overrides the node's background colour.
    pub fn background(mut self, color: Color) -> NodeStyle {
        self.background = Some(color);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builders_set_only_their_field() {
        let style = NodeStyle::new()
            .bold(true)
            .badge("3")
            .icon(2)
            .text(Color::rgb(1, 2, 3))
            .background(Color::rgb(4, 5, 6));
        assert!(style.bold);
        assert_eq!(style.badge.as_deref(), Some("3"));
        assert_eq!(style.icon, Some(2));
        assert_eq!(style.text, Some(Color::rgb(1, 2, 3)));
        assert_eq!(style.background, Some(Color::rgb(4, 5, 6)));
    }

    #[test]
    fn default_overrides_nothing() {
        let style = NodeStyle::default();
        assert!(!style.bold && style.badge.is_none() && style.icon.is_none());
        assert!(style.text.is_none() && style.background.is_none());
    }
}
