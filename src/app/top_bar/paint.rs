#![forbid(unsafe_code)]

//! Painting the material top bar items with Direct2D/DirectWrite from the
//! rectangles laid out in [`super::state`]. Glyphs and translucent hover pills
//! write premultiplied alpha, so they stay visible over the DWM material where
//! GDI would write zero alpha. Nothing here allocates.

use crate::color::Color;
use crate::d2d::{D2dCanvas, PointF, RectF, Rgba};
use crate::theme::Theme;

use super::state::{Item, Kind, LABEL_PAD_DIP, TopBarState};

/// Corner radius of a button's pill.
const RADIUS: f32 = 4.0;
/// Alpha of the hover pill (a translucent token, so the material shows).
const HOVER_ALPHA: u8 = 0x50;
/// Alpha of the pressed pill.
const PRESSED_ALPHA: u8 = 0xA0;
/// Alpha of the focused pill.
const FOCUS_ALPHA: u8 = 0x60;
/// Alpha of a checked toggle's accent pill.
const CHECKED_ALPHA: u8 = 0xC0;

impl TopBarState {
    /// Paints every item from the stored rectangles. The band's background is
    /// the caller's (the core fills it opaque when the material is inactive).
    pub(crate) fn paint_items(&self, canvas: &mut D2dCanvas, dpi: u32, theme: &Theme) {
        let scale = dpi as f32 / 96.0;
        let rects = self.rects.borrow();
        let items = self.items.borrow();
        for (index, item) in items.iter().enumerate() {
            let Some(rect) = rects.get(index).copied() else {
                continue;
            };
            let rect = RectF::new(
                rect.left as f32 / scale,
                rect.top as f32 / scale,
                rect.right as f32 / scale,
                rect.bottom as f32 / scale,
            );
            match item.kind {
                Kind::Icon | Kind::Toggle => self.paint_button(canvas, item, index, rect, theme),
                Kind::Slider => {
                    // `SliderState::draw` draws in widget-local coordinates, so
                    // the row's origin is supplied as a canvas translation and
                    // the slider is drawn from `(0, 0)`. Reset it afterwards so
                    // the items that follow paint at their own rectangles.
                    if let Some(slider) = item.slider.as_ref() {
                        canvas.set_translation(rect.left, rect.top);
                        let local = RectF::new(0.0, 0.0, rect.width(), rect.height());
                        slider.draw(canvas, local, theme);
                        canvas.set_translation(0.0, 0.0);
                    }
                }
                Kind::Label => paint_label(canvas, item, rect, theme),
                Kind::Spacer | Kind::Native => {}
            }
        }
    }

    /// Paints one icon/toggle button: its state pill and its centred glyph.
    fn paint_button(
        &self,
        canvas: &mut D2dCanvas,
        item: &Item,
        index: usize,
        rect: RectF,
        theme: &Theme,
    ) {
        let pressed = self.pressed.get() == Some(index);
        let focused = self.focus.get() == Some(index);
        let hovered = self.hover.get() == Some(index);
        let checked = item.kind == Kind::Toggle && item.checked;

        if checked {
            canvas.fill_rounded_rect_rgba(rect, RADIUS, rgba(theme.accent, CHECKED_ALPHA));
        } else if pressed {
            canvas.fill_rounded_rect_rgba(rect, RADIUS, rgba(theme.pressed, PRESSED_ALPHA));
        } else if hovered || focused {
            let alpha = if focused { FOCUS_ALPHA } else { HOVER_ALPHA };
            canvas.fill_rounded_rect_rgba(rect, RADIUS, rgba(theme.hover, alpha));
        }

        let color = if !item.enabled {
            theme.text_disabled
        } else if checked {
            theme.text_on_accent
        } else {
            theme.text
        };
        if let Some(glyph) = item.glyph.as_ref() {
            draw_centered(canvas, glyph, rect, color);
        }
    }
}

/// Draws a label, vertically centred and left-aligned inside its rectangle.
fn paint_label(canvas: &mut D2dCanvas, item: &Item, rect: RectF, theme: &Theme) {
    let Some(text) = item.text.as_ref() else {
        return;
    };
    let color = if item.enabled {
        theme.text
    } else {
        theme.text_disabled
    };
    let top = rect.top + (rect.height() - text.height()) / 2.0;
    canvas.draw_text(
        text,
        PointF::new(rect.left + LABEL_PAD_DIP, top.max(rect.top)),
        color,
    );
}

/// Draws `layout` centred in `rect`.
fn draw_centered(canvas: &mut D2dCanvas, layout: &crate::d2d::Layout, rect: RectF, color: Color) {
    let (width, height) = layout.size();
    let x = rect.left + (rect.width() - width) / 2.0;
    let y = rect.top + (rect.height() - height) / 2.0;
    canvas.draw_text(layout, PointF::new(x, y), color);
}

/// A token colour at `alpha`, as the transparent Direct2D brushes take it.
fn rgba(color: Color, alpha: u8) -> Rgba {
    Rgba::with_alpha(color.r, color.g, color.b, alpha)
}
