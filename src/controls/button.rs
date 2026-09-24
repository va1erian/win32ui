#![forbid(unsafe_code)]

//! A push button that maps clicks to the app's `Msg`.

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::Ui;
use crate::controls::control::{AsControl, Control, HasText};
use crate::controls::registry;
use crate::controls::{create_child, next_id, style};
use crate::error::Result;
use crate::gdi::Font;
use crate::geometry::Rect;
use crate::hwnd::Hwnd;
use crate::message::{CommandNotification, Message};
use crate::sys;
use crate::theme::{Theme, Themed};
use crate::units::dip;

/// The app-level click mapping of a [`Button`].
struct ButtonEvents<M> {
    on_click: Option<Box<dyn Fn() -> Option<M>>>,
}

/// A native push button.
///
/// Its text colours come from the window theme through the central
/// `WM_CTLCOLORBTN` answer; the button chrome itself follows
/// `DarkMode_CFD` (see [`Themed`]).
pub struct Button<M> {
    control: Control,
    id: usize,
    parent: Hwnd,
    events: Rc<RefCell<ButtonEvents<M>>>,
}

impl<M: 'static> Button<M> {
    /// Creates the button as a child of the window behind `ui`, adopting
    /// `ui`'s theme. Its natural size fits `text` at the window's DPI;
    /// position it with a layout or
    /// [`ControlExt::set_bounds`](crate::ControlExt::set_bounds).
    pub fn new(ui: &mut Ui<M>, text: &str) -> Result<Button<M>> {
        let dpi = ui.dpi();
        let parent = ui.hwnd();
        let font = Font::system_ui(dpi)?;
        let text_width = sys::gdi::measure_text(font.raw(), text).width;
        let width = (text_width + dip(28.0).to_px(dpi).value()).max(dip(64.0).to_px(dpi).value());
        let height =
            (font.pixel_height() + dip(14.0).to_px(dpi).value()).max(dip(24.0).to_px(dpi).value());
        let bounds = Rect::new(0, 0, width, height);
        let button_style = sys::button::button_style(false);
        let button_style_bits = style::WS_CHILD | style::WS_VISIBLE | style::WS_TABSTOP;
        let id = next_id();
        let hwnd = create_child(
            "Button",
            "BUTTON",
            parent,
            button_style_bits | button_style,
            0,
            id,
            bounds,
        )?;
        sys::control::set_control_font(hwnd, font.raw());
        sys::apply_native_theme(hwnd, sys::NativeControlKind::Button, ui.theme().is_dark);

        let events = Rc::new(RefCell::new(ButtonEvents { on_click: None }));
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
            let msg = events_for_mapper
                .borrow()
                .on_click
                .as_ref()
                .and_then(|f| f());
            if let Some(msg) = msg {
                sink.emit(msg);
            }
            true
        });
        registry::register_app_events(hwnd, mapper);

        let button = Button {
            control: Control::own(hwnd, bounds),
            id,
            parent,
            events,
        };
        button.set_text(text);
        crate::theme::register_themed(
            parent,
            hwnd,
            Rc::new(move |applied| {
                let _ = sys::control::apply_ui_font(hwnd, dpi);
                sys::apply_native_theme(hwnd, sys::NativeControlKind::Button, applied.is_dark);
                sys::window::invalidate(hwnd);
            }),
        );
        Ok(button)
    }

    /// Maps a click to an app message.
    pub fn on_click(self, f: impl Fn() -> Option<M> + 'static) -> Button<M> {
        self.events.borrow_mut().on_click = Some(Box::new(f));
        self
    }

    /// Marks the button as the window's default: it draws with the default
    /// frame and responds to Enter through the dialog navigation.
    pub fn default(self) -> Button<M> {
        self.set_default(true);
        self
    }

    /// Marks or unmarks the button as the window's default.
    pub fn set_default(&self, is_default: bool) {
        sys::button::set_default(self.control.hwnd(), self.parent, self.id, is_default);
    }

    /// Whether the button is the window's default.
    pub fn is_default(&self) -> bool {
        sys::button::is_default(self.control.hwnd())
    }

    /// Simulates a user click, firing `BN_CLICKED` synchronously.
    pub fn click(&self) {
        sys::button::click(self.control.hwnd());
    }
}

impl<M> AsControl for Button<M> {
    fn control(&self) -> &Control {
        &self.control
    }
}

impl<M> Themed for Button<M> {
    fn apply_theme(&self, theme: &Theme) {
        // Text/background come from the central `WM_CTLCOLORBTN` answer; the
        // native chrome follows the theme and a repaint picks it up.
        sys::apply_native_theme(
            self.control.hwnd(),
            sys::NativeControlKind::Button,
            theme.is_dark,
        );
        sys::window::invalidate(self.control.hwnd());
    }
}

impl<M> Drop for Button<M> {
    fn drop(&mut self) {
        registry::unregister_app_events(self.control.hwnd());
        crate::theme::unregister_themed(self.control.hwnd());
    }
}

impl<M> HasText for Button<M> {
    fn text(&self) -> String {
        sys::window::get_title(self.control.hwnd())
    }

    fn set_text(&self, text: &str) {
        let _ = sys::window::set_title(self.control.hwnd(), text);
    }
}
