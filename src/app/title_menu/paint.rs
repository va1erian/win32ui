#![forbid(unsafe_code)]

//! Painting the strip menu items with Direct2D/DirectWrite from the rectangles
//! laid out in [`super`]. DirectWrite text and translucent pills write
//! premultiplied alpha, so both stay visible over the DWM material where GDI
//! would write zero alpha.

use crate::color::Color;
use crate::d2d::{D2dCanvas, Interpolation, PointF, RectF, Rgba};
use crate::theme::Theme;
use crate::units::dip;

use super::{ICON_LEFT, ICON_SIZE, Item, PAD, TitleBarMenu};

/// Corner radius of the hover/pressed/focus pill.
const RADIUS: f32 = 5.0;
/// Alpha of the hover pill (a translucent token, so the material shows).
const HOVER_ALPHA: u8 = 0x50;
/// Alpha of the focus/open pill.
const FOCUS_ALPHA: u8 = 0x78;
/// Alpha of the pressed pill.
const PRESSED_ALPHA: u8 = 0xA0;

impl<M: 'static> TitleBarMenu<M> {
    /// Paints the window title and the menu items from the stored rectangles.
    /// DWM draws only the caption buttons in an extended frame, so the app owns
    /// the title text on the glass.
    pub(crate) fn paint(&self, canvas: &mut D2dCanvas, dpi: u32, theme: &Theme) {
        let scale = dpi as f32 / 96.0;
        let pad_px = dip(PAD).to_px(dpi).value();
        let inset_px = dip(2.0).to_px(dpi).value().max(1);
        let active = self.active.get();
        let rects = self.layout.borrow();
        let caption = self.caption_px.get() as f32 / scale;

        if let Some(icon) = self.icon.borrow().as_ref() {
            let id = self.icon_id.get().unwrap_or_else(|| {
                let id = canvas.image(icon);
                self.icon_id.set(Some(id));
                id
            });
            let left = dip(ICON_LEFT).to_px(dpi).value() as f32 / scale;
            let top = ((caption - ICON_SIZE) / 2.0).max(0.0);
            canvas.draw_image(
                id,
                RectF::new(left, top, left + ICON_SIZE, top + ICON_SIZE),
                None,
                1.0,
                Interpolation::Linear,
            );
        }

        if let Some(title) = self.title.borrow().as_ref() {
            let top = (caption - self.line_height_dip) / 2.0;
            canvas.draw_text(
                title,
                PointF::new(self.title_x.get() as f32 / scale, top.max(0.0)),
                theme.text,
            );
        }

        for (index, item) in self.items.iter().enumerate() {
            let Some(rect) = rects.get(index).copied() else {
                continue;
            };
            let hovered = self.hover.get() == Some(index);
            let focused = active && self.focus.get() == Some(index);
            let pressed = self.pressed.get() == Some(index);
            if hovered || focused || pressed {
                let color = if pressed { theme.pressed } else { theme.hover };
                let alpha = if pressed {
                    PRESSED_ALPHA
                } else if focused {
                    FOCUS_ALPHA
                } else {
                    HOVER_ALPHA
                };
                let pill = RectF::new(
                    rect.left as f32 / scale,
                    (rect.top + inset_px) as f32 / scale,
                    rect.right as f32 / scale,
                    (rect.bottom - inset_px) as f32 / scale,
                );
                canvas.fill_rounded_rect_rgba(pill, RADIUS, rgba(color, alpha));
            }

            let text_color = if item.enabled {
                theme.text
            } else {
                theme.text_disabled
            };
            let top = rect.top as f32 / scale
                + ((rect.height() as f32 - self.line_height_dip * scale) / 2.0) / scale;
            let origin = PointF::new((rect.left + pad_px) as f32 / scale, top);
            canvas.draw_text(&item.layout, origin, text_color);

            if self.cues.get()
                && let Some(byte) = item.mnemonic_index
            {
                self.underline(canvas, item, byte, origin, text_color);
            }
        }
    }

    /// Underlines the mnemonic character of `item` at `origin` (DIPs).
    fn underline(
        &self,
        canvas: &mut D2dCanvas,
        item: &Item,
        byte: usize,
        origin: PointF,
        color: Color,
    ) {
        let char_len = item.label[byte..]
            .chars()
            .next()
            .map_or(0, |c| c.len_utf8());
        let start = item.layout.caret_rect(byte);
        let end = item.layout.caret_rect(byte + char_len);
        let left = origin.x + start.left;
        let right = origin.x + end.left.max(start.left + 1.0);
        let y = origin.y + start.bottom.max(self.line_height_dip) + 1.0;
        canvas.draw_line(
            PointF::new(left, y),
            PointF::new(right, y),
            color,
            crate::d2d::Stroke::solid(1.0),
        );
    }
}

/// A token colour at `alpha`, as the transparent Direct2D brushes take it.
fn rgba(color: Color, alpha: u8) -> Rgba {
    Rgba::with_alpha(color.r, color.g, color.b, alpha)
}
