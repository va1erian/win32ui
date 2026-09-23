//! Drawing text on a [`D2dCanvas`].

use crate::color::Color;
use crate::d2d::{D2dCanvas, PointF};

use super::Layout;

impl D2dCanvas<'_> {
    /// Draws `layout` with its top-left corner at `origin`. Colour glyphs
    /// (emoji) keep their own colours; everything else takes `color`.
    pub fn draw_text(&mut self, layout: &Layout, origin: PointF, color: Color) {
        self.with(|target| target.draw_layout(layout.sys(), origin, color));
    }
}
