//! Drawing DirectWrite layouts on the render target.

use windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT;

use crate::color::Color;
use crate::d2d::PointF;
use crate::sys::d2d::text::TextLayout;

use super::{Target, vector};

impl Target {
    /// Draws `layout` with its top-left corner at `origin`.
    pub(crate) fn draw_layout(&mut self, layout: &TextLayout, origin: PointF, color: Color) {
        if let Some(brush) = self.brush(color) {
            // SAFETY: the layout, brush and target are live; drawing is active.
            unsafe {
                self.render.DrawTextLayout(
                    vector(origin),
                    layout.raw(),
                    &brush,
                    D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
                )
            }
        }
    }
}
