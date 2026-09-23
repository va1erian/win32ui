#![forbid(unsafe_code)]

//! An owner-drawn toolbar: a row of buttons that paint themselves (background,
//! hover/pressed states, icon and label) and map clicks to the app's `Msg`
//! through each item's `on_click` closure.
//!
//! The toolbar is a [`CustomWidget`](crate::CustomWidget): its child window is
//! owned by [`Custom`](crate::Custom), which routes input and theme changes to
//! it. This is the same owner-draw pattern the status bar and any user widget
//! use.

use std::cell::Cell;

use crate::app::Ui;
use crate::color::Color;
use crate::controls::control::{AsControl, Control};
use crate::controls::custom::{Custom, CustomWidget, Input, WidgetCx};
use crate::error::Result;
use crate::gdi::{Bitmap, Canvas, Font, TextFormat};
use crate::geometry::{Point, Rect, Size};
use crate::message::MouseButton;
use crate::sys;
use crate::theme::{Theme, Themed};
use crate::units::dip;

/// One toolbar button.
pub struct ToolbarItem<M> {
    label: String,
    icon: Option<Bitmap>,
    on_click: Option<Box<dyn Fn() -> Option<M>>>,
}

impl<M> ToolbarItem<M> {
    /// A button with a label and no icon.
    pub fn new(label: impl Into<String>) -> ToolbarItem<M> {
        ToolbarItem {
            label: label.into(),
            icon: None,
            on_click: None,
        }
    }

    /// Adds an icon.
    pub fn with_icon(mut self, icon: Bitmap) -> ToolbarItem<M> {
        self.icon = Some(icon);
        self
    }

    /// Maps a click on this button to an app message.
    pub fn on_click(mut self, f: impl Fn() -> Option<M> + 'static) -> ToolbarItem<M> {
        self.on_click = Some(Box::new(f));
        self
    }
}

/// Colours for the toolbar.
#[derive(Clone, Copy, Debug)]
pub struct ToolbarTheme {
    /// Toolbar background.
    pub background: Color,
    /// Idle button background.
    pub button: Color,
    /// Hovered button background.
    pub button_hover: Color,
    /// Pressed button background.
    pub button_pressed: Color,
    /// Button label colour.
    pub text: Color,
    /// Separator/border colour.
    pub border: Color,
}

impl ToolbarTheme {
    /// Derives a toolbar palette from the app [`Theme`]. Override any field
    /// after calling this for a custom look.
    pub fn from_theme(theme: &Theme) -> ToolbarTheme {
        ToolbarTheme {
            background: theme.surface,
            button: theme.surface,
            button_hover: theme.hover,
            button_pressed: theme.pressed,
            text: theme.text,
            border: theme.border,
        }
    }
}

/// The mutable state behind a [`Toolbar`], shared with the child window.
struct ToolbarWidget<M> {
    items: Vec<ToolbarItem<M>>,
    font: Font,
    dpi: u32,
    widths: Vec<i32>,
    height: i32,
    hover: Cell<Option<usize>>,
    pressed: Cell<Option<usize>>,
}

impl<M> ToolbarWidget<M> {
    fn new(items: Vec<ToolbarItem<M>>, dpi: u32) -> Result<ToolbarWidget<M>> {
        let font = Font::system_ui(dpi)?;
        let padding = dip(10.0).to_px(dpi).value();
        let icon = dip(16.0).to_px(dpi).value();
        let gap = dip(6.0).to_px(dpi).value();
        // `pixel_height` is the font's em box, not its glyphs; measure the real
        // line height (ascent + descent) so labels are never clipped. Add a 2dip
        // button margin and 4dip of text padding on each side.
        let line_height = sys::gdi::measure_text(font.raw(), "Ag").height;
        let height = line_height + dip(12.0).to_px(dpi).value();

        let widths = items
            .iter()
            .map(|item| {
                let text = sys::gdi::measure_text(font.raw(), &item.label).width;
                let icon_width = if item.icon.is_some() { icon + gap } else { 0 };
                (text + icon_width + padding * 2).max(dip(32.0).to_px(dpi).value())
            })
            .collect();

        Ok(ToolbarWidget {
            items,
            font,
            dpi,
            widths,
            height,
            hover: Cell::new(None),
            pressed: Cell::new(None),
        })
    }

    /// The button rectangles for the current client width.
    fn rects(&self) -> Vec<Rect> {
        let mut rects = Vec::with_capacity(self.items.len());
        let mut x = 0;
        for width in &self.widths {
            rects.push(Rect::new(x, 0, x + width, self.height));
            x += width;
        }
        rects
    }

    fn hit_test(&self, x: i32, y: i32) -> Option<usize> {
        let point = Point::new(x, y);
        self.rects().iter().position(|rect| rect.contains(point))
    }

    /// The rectangle the label is drawn in: the full button height (so the text
    /// is vertically centred without clipping ascenders or descenders) with a
    /// 6dip horizontal inset for padding.
    fn text_rect(&self, button: Rect) -> Rect {
        let inset = dip(6.0).to_px(self.dpi).value();
        Rect::new(
            button.left + inset,
            button.top,
            button.right - inset,
            button.bottom,
        )
    }

    fn draw(&self, canvas: &Canvas, bounds: Rect, theme: &ToolbarTheme) {
        canvas.fill_rect(bounds, theme.background);
        let radius = dip(4.0).to_px(self.dpi).value();
        let rects = self.rects();
        for (index, rect) in rects.iter().enumerate() {
            if index >= self.items.len() {
                break;
            }
            let background = if self.pressed.get() == Some(index) {
                theme.button_pressed
            } else if self.hover.get() == Some(index) {
                theme.button_hover
            } else {
                theme.button
            };
            let button = rect.shrink(dip(2.0).to_px(self.dpi).value());
            canvas.round_rect(button, radius, background, None);

            let inset = dip(6.0).to_px(self.dpi).value();
            let mut text_rect = self.text_rect(button);
            if let Some(icon) = &self.items[index].icon {
                let icon_size = icon.size();
                let top = button.top + (button.height() - icon_size.height) / 2;
                let icon_rect = Rect::new(
                    button.left + inset,
                    top,
                    button.left + inset + icon_size.width,
                    top + icon_size.height,
                );
                canvas.draw_bitmap(icon, icon_rect);
                text_rect.left = icon_rect.right + inset;
            }
            canvas.with_font(&self.font, |canvas| {
                canvas.draw_text(
                    text_rect,
                    &self.items[index].label,
                    theme.text,
                    TextFormat::left().single_line().vcenter().end_ellipsis(),
                );
            });
        }
        canvas.fill_rect(
            Rect::new(0, bounds.bottom - 1, bounds.right, bounds.bottom),
            theme.border,
        );
    }
}

impl<M: 'static> CustomWidget for ToolbarWidget<M> {
    /// The clicked item's index.
    type Event = usize;

    fn paint(&self, canvas: &Canvas, bounds: Rect, theme: &Theme) {
        let theme = ToolbarTheme::from_theme(theme);
        self.draw(canvas, bounds, &theme);
    }

    fn input(&self, input: Input, cx: &mut WidgetCx<usize>) {
        match input {
            Input::MouseMove { x, y } => {
                let hover = self.hit_test(x, y);
                if hover != self.hover.get() {
                    self.hover.set(hover);
                    cx.invalidate();
                }
            }
            Input::MouseDown {
                x,
                y,
                button: MouseButton::Left,
            } => {
                self.pressed.set(self.hit_test(x, y));
                cx.invalidate();
            }
            Input::MouseUp {
                x,
                y,
                button: MouseButton::Left,
            } => {
                let pressed = self.pressed.take();
                let hit = self.hit_test(x, y);
                if let (Some(pressed), Some(hit)) = (pressed, hit)
                    && pressed == hit
                {
                    cx.emit(hit);
                }
                cx.invalidate();
            }
            Input::MouseLeave if self.hover.get().is_some() => {
                self.hover.set(None);
                cx.invalidate();
            }
            _ => {}
        }
    }

    fn preferred_size(&self, _dpi: u32) -> Option<Size> {
        Some(Size::new(self.widths.iter().sum(), self.height))
    }
}

/// An owner-drawn toolbar control.
pub struct Toolbar<M: 'static> {
    custom: Custom<ToolbarWidget<M>, M>,
}

impl<M: 'static> Toolbar<M> {
    /// Creates the toolbar as a child of the window behind `ui`, adopting
    /// `ui`'s theme. Use [`Themed::apply_theme`] for a one-off override.
    pub fn new(ui: &mut Ui<M>, items: Vec<ToolbarItem<M>>) -> Result<Toolbar<M>> {
        let widget = ToolbarWidget::new(items, ui.dpi())?;
        let custom = Custom::new(ui, widget)?;
        let shared = custom.widget();
        let custom = custom.on_event(move |index| {
            let state = shared.borrow();
            let item = state.items.get(index)?;
            let on_click = item.on_click.as_ref()?;
            on_click()
        });
        Ok(Toolbar { custom })
    }

    /// The toolbar's natural height.
    pub fn height(&self) -> i32 {
        self.custom.widget().borrow().height
    }

    /// Schedules a repaint.
    pub fn invalidate(&self) {
        self.custom.invalidate();
    }
}

impl<M: 'static> AsControl for Toolbar<M> {
    fn control(&self) -> &Control {
        self.custom.control()
    }
}

impl<M: 'static> Themed for Toolbar<M> {
    fn apply_theme(&self, theme: &Theme) {
        self.custom.apply_theme(theme);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The label rect must span the whole button height so `DrawText`'s
    /// vertical centring never clips ascenders or descenders (the regression:
    /// the rect was inset 6dip on every side, shrinking it below the text).
    #[test]
    fn text_rect_spans_the_full_button_height() {
        let widget = ToolbarWidget::<()>::new(vec![ToolbarItem::new("Ag")], 96).unwrap();
        let button = Rect::new(2, 2, 200, 2 + widget.height - 4);
        let rect = widget.text_rect(button);
        assert_eq!(rect.top, button.top, "the label top was inset");
        assert_eq!(rect.bottom, button.bottom, "the label bottom was inset");
        assert!(
            rect.height() >= widget.font.pixel_height(),
            "the label rect ({}) is shorter than the font ({})",
            rect.height(),
            widget.font.pixel_height()
        );
    }
}
