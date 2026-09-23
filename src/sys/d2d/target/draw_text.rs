//! Drawing DirectWrite layouts on the render target.

use windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT;
use windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_RANGE;

use crate::color::Color;
use crate::d2d::PointF;
use crate::sys::d2d::text::{RichTextLayout, TextLayout};

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

    /// Draws a rich `layout` with its top-left corner at `origin`, giving each
    /// span its own solid-colour brush as drawing effect.
    pub(crate) fn draw_rich_layout(&mut self, layout: &RichTextLayout, origin: PointF) {
        // A default brush is required even though every span sets an effect.
        let mut fallback = None;
        for style in layout.styles() {
            let Some(brush) = self.paints.solid(&self.render, style.color) else {
                continue;
            };
            if fallback.is_none() {
                fallback = Some(brush.clone());
            }
            let range = DWRITE_TEXT_RANGE {
                startPosition: style.start,
                length: style.length,
            };
            // SAFETY: the layout and the brush from this target are live;
            // drawing is active. The effect only changes the range's colour.
            unsafe {
                let _ = layout.raw().SetDrawingEffect(&brush, range);
            }
        }
        let Some(fallback) = fallback else {
            return;
        };
        // SAFETY: the layout and a brush from this target are live; drawing is
        // active.
        unsafe {
            self.render.DrawTextLayout(
                vector(origin),
                layout.raw(),
                &fallback,
                D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
            )
        }
    }
}
