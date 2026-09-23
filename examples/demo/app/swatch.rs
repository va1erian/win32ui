use std::cell::Cell;

use win32ui::Size;
use win32ui::gdi::Canvas;
use win32ui::prelude::*;

/// A clickable colour swatch: paints its fill from a mutable colour and emits
/// [`SwatchEvent::Clicked`] on a left click. Demonstrates a custom owner-drawn
/// widget, with app-side mutation through a `Cell` field (the widget is shared
/// and `paint`/`input` take `&self`).
pub(super) struct Swatch {
    color: Cell<Color>,
}

/// The events a [`Swatch`] raises.
pub(super) enum SwatchEvent {
    /// The swatch was left-clicked.
    Clicked,
}

impl Swatch {
    pub(super) fn new(color: Color) -> Swatch {
        Swatch {
            color: Cell::new(color),
        }
    }

    /// Replaces the fill colour; the demo calls this from `update`.
    pub(super) fn set_color(&self, color: Color) {
        self.color.set(color);
    }

    /// The current fill colour.
    pub(super) fn color(&self) -> Color {
        self.color.get()
    }
}

impl CustomWidget for Swatch {
    type Event = SwatchEvent;

    fn paint(&self, canvas: &Canvas, bounds: Rect, theme: &Theme) {
        canvas.fill_rect(bounds.shrink(1), self.color.get());
        canvas.outline(bounds, theme.border);
    }

    fn input(&self, input: Input, cx: &mut WidgetCx<SwatchEvent>) {
        if let Input::MouseUp {
            button: MouseButton::Left,
            ..
        } = input
        {
            cx.emit(SwatchEvent::Clicked);
        }
    }

    fn preferred_size(&self, dpi: u32) -> Option<Size> {
        Some(Size::new(
            dip(48.0).to_px(dpi).value(),
            dip(24.0).to_px(dpi).value(),
        ))
    }
}
