#![forbid(unsafe_code)]

//! A colour swatch that opens the common `ChooseColor` dialog.
//!
//! [`ColorPicker`] is an owner-drawn `BUTTON`: it shows the current colour as a
//! swatch painted from the window theme, and pressing it raises the standard
//! colour dialog seeded with that colour. A chosen colour raises
//! [`ColorPicker::on_change`] with the app's message. This is the "read-only
//! swatch plus *More colours…*" shape: the swatch is shown, not edited in place.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::app::Ui;
use crate::color::Color;
use crate::controls::control::{AsControl, Control};
use crate::controls::registry;
use crate::controls::{create_child, next_id, style};
use crate::error::Result;
use crate::geometry::Rect;
use crate::message::{CommandNotification, Message};
use crate::sys;
use crate::theme::{Theme, Themed};
use crate::units::dip;

/// The app-level change mapping of a [`ColorPicker`].
struct ColorPickerEvents<M> {
    on_change: Option<Box<dyn Fn(Color) -> Option<M>>>,
}

/// An owner-drawn colour swatch that picks a colour through `ChooseColor`.
pub struct ColorPicker<M> {
    control: Control,
    color: Rc<Cell<Color>>,
    events: Rc<RefCell<ColorPickerEvents<M>>>,
}

impl<M: 'static> ColorPicker<M> {
    /// Creates a picker showing `initial`, as a child of the window behind `ui`
    /// (or of the container `ui` is scoped to). Position it with a layout or
    /// [`ControlExt::set_bounds`](crate::ControlExt::set_bounds).
    pub fn new(ui: &mut Ui<M>, initial: Color) -> Result<ColorPicker<M>> {
        let dpi = ui.dpi();
        let parent = ui.hwnd();
        let bounds = Rect::new(
            0,
            0,
            dip(56.0).to_px(dpi).value(),
            dip(24.0).to_px(dpi).value(),
        );
        let id = next_id();
        let style_bits = style::WS_CHILD | style::WS_VISIBLE | style::WS_TABSTOP;
        let hwnd = create_child(
            "ColorPicker",
            "BUTTON",
            parent,
            style_bits | sys::button::owner_draw_style(),
            0,
            id,
            bounds,
        )?;
        sys::apply_native_theme(hwnd, sys::NativeControlKind::Button, ui.theme().is_dark);

        let color = Rc::new(Cell::new(initial));
        let events = Rc::new(RefCell::new(ColorPickerEvents { on_change: None }));
        let sink = ui.clone();
        let color_for_mapper = Rc::clone(&color);
        let events_for_mapper = Rc::clone(&events);
        let mapper: Rc<dyn Fn(&Message) -> bool> = Rc::new(move |message| match message {
            Message::DrawItem {
                control,
                dc,
                area,
                state,
                ..
            } if *control == hwnd => {
                let theme = sink.theme();
                sys::colorpicker::draw_swatch(
                    *dc,
                    *area,
                    color_for_mapper.get(),
                    theme.border,
                    *state,
                );
                true
            }
            Message::Command(command)
                if command.control == Some(hwnd)
                    && command.notification == CommandNotification::Clicked =>
            {
                let owner = sys::window::root(hwnd);
                let current = color_for_mapper.get();
                if let Some(chosen) = sys::colorpicker::choose_color(owner, current)
                    && chosen != current
                {
                    color_for_mapper.set(chosen);
                    sys::window::invalidate(hwnd);
                    let msg = events_for_mapper
                        .borrow()
                        .on_change
                        .as_ref()
                        .and_then(|f| f(chosen));
                    if let Some(msg) = msg {
                        sink.emit(msg);
                    }
                }
                true
            }
            _ => false,
        });
        registry::register_app_events(hwnd, mapper);

        let picker = ColorPicker {
            control: Control::own(hwnd, bounds),
            color,
            events,
        };
        crate::theme::register_themed(
            parent,
            hwnd,
            Rc::new(move |applied| {
                sys::apply_native_theme(hwnd, sys::NativeControlKind::Button, applied.is_dark);
                sys::window::invalidate(hwnd);
            }),
        );
        Ok(picker)
    }

    /// Maps a chosen colour to an app message. Returning `None` ignores the
    /// change (the swatch still updates).
    pub fn on_change(self, f: impl Fn(Color) -> Option<M> + 'static) -> ColorPicker<M> {
        self.events.borrow_mut().on_change = Some(Box::new(f));
        self
    }

    /// The colour currently shown.
    pub fn color(&self) -> Color {
        self.color.get()
    }

    /// Replaces the colour shown, without raising `on_change`. Setting the
    /// colour it already shows is a no-op, so syncing a form does not repaint
    /// (and flicker) the swatch.
    pub fn set_color(&self, color: Color) {
        if self.color.replace(color) == color {
            return;
        }
        sys::window::invalidate(self.control.hwnd());
    }
}

impl<M> AsControl for ColorPicker<M> {
    fn control(&self) -> &Control {
        &self.control
    }
}

impl<M> Themed for ColorPicker<M> {
    fn apply_theme(&self, theme: &Theme) {
        sys::apply_native_theme(
            self.control.hwnd(),
            sys::NativeControlKind::Button,
            theme.is_dark,
        );
        sys::window::invalidate(self.control.hwnd());
    }
}

impl<M> Drop for ColorPicker<M> {
    fn drop(&mut self) {
        registry::unregister_app_events(self.control.hwnd());
        crate::theme::unregister_themed(self.control.hwnd());
    }
}
