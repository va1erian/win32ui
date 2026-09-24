#![forbid(unsafe_code)]

//! A two-state check box that maps toggles to the app's `Msg`.

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::Ui;
use crate::controls::control::{AsControl, Control, HasText};
use crate::controls::registry;
use crate::controls::{create_child, next_id, style};
use crate::error::Result;
use crate::gdi::Font;
use crate::geometry::Rect;
use crate::message::{CommandNotification, Message};
use crate::sys;
use crate::theme::{Theme, Themed};
use crate::units::dip;

/// The app-level toggle mapping of a [`CheckBox`].
struct CheckBoxEvents<M> {
    on_toggle: Option<Box<dyn Fn(bool) -> Option<M>>>,
}

/// A native auto check box (`BS_AUTOCHECKBOX`).
///
/// The box toggles itself natively; the click notification only reports the
/// new state through [`CheckBox::is_checked`]. Colours follow the window
/// theme like [`Button`](crate::Button).
pub struct CheckBox<M> {
    control: Control,
    events: Rc<RefCell<CheckBoxEvents<M>>>,
}

impl<M: 'static> CheckBox<M> {
    /// Creates the box as a child of the window behind `ui`, adopting `ui`'s
    /// theme. Its natural size fits `text` at the window's DPI.
    pub fn new(ui: &mut Ui<M>, text: &str) -> Result<CheckBox<M>> {
        let dpi = ui.dpi();
        let parent = ui.hwnd();
        let font = Font::system_ui(dpi)?;
        let text_width = sys::gdi::measure_text(font.raw(), text).width;
        let width = (text_width + dip(28.0).to_px(dpi).value()).max(dip(64.0).to_px(dpi).value());
        let height =
            (font.pixel_height() + dip(10.0).to_px(dpi).value()).max(dip(20.0).to_px(dpi).value());
        let bounds = Rect::new(0, 0, width, height);
        let style =
            style::WS_CHILD | style::WS_VISIBLE | style::WS_TABSTOP | sys::button::checkbox_style();
        let hwnd = create_child("CheckBox", "BUTTON", parent, style, 0, next_id(), bounds)?;
        sys::control::set_control_font(hwnd, font.raw());
        sys::apply_native_theme(hwnd, sys::NativeControlKind::Button, ui.theme().is_dark);

        let events = Rc::new(RefCell::new(CheckBoxEvents { on_toggle: None }));
        let sink = ui.clone();
        let events_for_mapper = Rc::clone(&events);
        let mapper: Rc<dyn Fn(&Message) -> bool> = Rc::new(move |message| {
            let Message::Command(command) = message else {
                return false;
            };
            if command.control != Some(hwnd) || command.notification != CommandNotification::Clicked
            {
                return false;
            }
            let checked = sys::button::is_checked(hwnd);
            let msg = events_for_mapper
                .borrow()
                .on_toggle
                .as_ref()
                .and_then(|f| f(checked));
            if let Some(msg) = msg {
                sink.emit(msg);
            }
            true
        });
        registry::register_app_events(hwnd, mapper);

        let check = CheckBox {
            control: Control::own(hwnd, bounds),
            events,
        };
        check.set_text(text);
        crate::theme::register_themed(
            parent,
            hwnd,
            Rc::new(move |applied| {
                let _ = sys::control::apply_ui_font(hwnd, dpi);
                sys::apply_native_theme(hwnd, sys::NativeControlKind::Button, applied.is_dark);
                sys::window::invalidate(hwnd);
            }),
        );
        Ok(check)
    }

    /// Sets the initial state, returning the box for chaining.
    pub fn checked(self, checked: bool) -> CheckBox<M> {
        self.set_checked(checked);
        self
    }

    /// Maps a toggle to an app message, receiving the new state.
    pub fn on_toggle(self, f: impl Fn(bool) -> Option<M> + 'static) -> CheckBox<M> {
        self.events.borrow_mut().on_toggle = Some(Box::new(f));
        self
    }

    /// Whether the box is currently checked.
    pub fn is_checked(&self) -> bool {
        sys::button::is_checked(self.control.hwnd())
    }

    /// Checks or unchecks the box.
    pub fn set_checked(&self, checked: bool) {
        sys::button::set_checked(self.control.hwnd(), checked);
    }

    /// Simulates a user click, toggling the box synchronously.
    pub fn click(&self) {
        sys::button::click(self.control.hwnd());
    }
}

impl<M> AsControl for CheckBox<M> {
    fn control(&self) -> &Control {
        &self.control
    }
}

impl<M> Themed for CheckBox<M> {
    fn apply_theme(&self, theme: &Theme) {
        sys::apply_native_theme(
            self.control.hwnd(),
            sys::NativeControlKind::Button,
            theme.is_dark,
        );
        sys::window::invalidate(self.control.hwnd());
    }
}

impl<M> Drop for CheckBox<M> {
    fn drop(&mut self) {
        registry::unregister_app_events(self.control.hwnd());
        crate::theme::unregister_themed(self.control.hwnd());
    }
}

impl<M> HasText for CheckBox<M> {
    fn text(&self) -> String {
        sys::window::get_title(self.control.hwnd())
    }

    fn set_text(&self, text: &str) {
        let _ = sys::window::set_title(self.control.hwnd(), text);
    }
}
