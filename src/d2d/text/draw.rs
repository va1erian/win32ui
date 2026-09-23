//! Drawing text on a [`D2dCanvas`].

use crate::color::Color;
use crate::d2d::{D2dCanvas, PointF};

use super::{Layout, RichLayout};

impl D2dCanvas<'_> {
    /// Draws `layout` with its top-left corner at `origin`. Colour glyphs
    /// (emoji) keep their own colours; everything else takes `color`.
    pub fn draw_text(&mut self, layout: &Layout, origin: PointF, color: Color) {
        self.with(|target| target.draw_layout(layout.sys(), origin, color));
    }

    /// Draws a [`RichLayout`] with its top-left corner at `origin`, each span
    /// in its own colour. Colour glyphs (emoji) keep their own colours.
    pub fn draw_rich_text(&mut self, layout: &RichLayout, origin: PointF) {
        self.with(|target| target.draw_rich_layout(layout.sys(), origin));
    }
}
