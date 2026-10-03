#![forbid(unsafe_code)]

//! A two-state check box that maps toggles to the app's `Msg`.
//!
//! The themed native check box fills its checked glyph with the *system*
//! accent, so the app's [`Theme::accent`] never reaches it. The box is an
//! owner-drawn (`BS_OWNERDRAW`) button instead: the glyph and label are
//! painted from theme tokens on `WM_DRAWITEM`, while the native button
//! behaviour (focus, Space, `BN_CLICKED`) is kept. An owner-drawn button holds
//! no check state, so the widget keeps it.

use std::cell::{Cell, RefCell};
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

/// The app-level toggle mapping of a [`CheckBox`].
struct CheckBoxEvents<M> {
    on_toggle: Option<Box<dyn Fn(bool) -> Option<M>>>,
}

/// The state the `WM_DRAWITEM` mapper paints from, shared with the widget.
struct CheckState {
    hwnd: Hwnd,
    checked: Cell<bool>,
    label: RefCell<String>,
    theme: Cell<Theme>,
    font: Rc<Font>,
}

impl CheckState {
    /// Records the check state, repainting only on a change. (An owner-drawn
    /// button does not repaint itself when its state changes.)
    fn set_checked(&self, checked: bool) {
        if self.checked.replace(checked) != checked {
            sys::window::invalidate(self.hwnd);
        }
    }

    fn draw(&self, dc: isize, area: Rect, state: u32) {
        let theme = self.theme.get();
        let paint = sys::checkbox_draw::CheckPaint {
            text: theme.text,
            text_disabled: theme.text_disabled,
            edge: theme.text_secondary,
            accent: theme.accent,
            mark: theme.text_on_accent,
            focus: theme.border_focused,
            background: theme.background,
        };
        sys::checkbox_draw::draw_checkbox(
            dc,
            area,
            &self.label.borrow(),
            sys::control::current_font(self.hwnd).unwrap_or(self.font.raw()),
            self.checked.get(),
            state,
            &paint,
        );
    }
}

/// An owner-drawn two-state check box.
///
/// Clicking (or Space) toggles it and reports the new state through
/// [`CheckBox::on_toggle`]. Checked, the box is filled with the theme's
/// accent colour, like [`RadioGroup`](crate::RadioGroup)'s dot.
pub struct CheckBox<M> {
    control: Control,
    events: Rc<RefCell<CheckBoxEvents<M>>>,
    state: Rc<CheckState>,
}

impl<M: 'static> CheckBox<M> {
    /// Creates the box as a child of the window behind `ui`, adopting `ui`'s
    /// theme. Its natural size fits `text` at the window's DPI.
    pub fn new(ui: &mut Ui<M>, text: &str) -> Result<CheckBox<M>> {
        let dpi = ui.dpi();
        let parent = ui.hwnd();
        let font = Font::shared_ui(dpi)?;
        let text_width = sys::gdi::measure_text(font.raw(), text).width;
        let width = (text_width + dip(28.0).to_px(dpi).value()).max(dip(64.0).to_px(dpi).value());
        let height =
            (font.pixel_height() + dip(10.0).to_px(dpi).value()).max(dip(20.0).to_px(dpi).value());
        let bounds = Rect::new(0, 0, width, height);
        let style = style::WS_CHILD
            | style::WS_VISIBLE
            | style::WS_TABSTOP
            | sys::button_draw::owner_drawn(sys::button::checkbox_style());
        let hwnd = create_child("CheckBox", "BUTTON", parent, style, 0, next_id(), bounds)?;
        let _ = sys::window::set_title(hwnd, text);

        let state = Rc::new(CheckState {
            hwnd,
            checked: Cell::new(false),
            label: RefCell::new(text.to_string()),
            theme: Cell::new(ui.theme()),
            font,
        });
        let weak = Rc::downgrade(&state);
        sys::uia::attach_native_state(
            hwnd,
            crate::accessibility::Role::CheckBox,
            Rc::new(move || weak.upgrade().is_some_and(|state| state.checked.get())),
        );

        let events = Rc::new(RefCell::new(CheckBoxEvents { on_toggle: None }));
        let sink = ui.clone();
        let events_for_mapper = Rc::clone(&events);
        let state_for_mapper = Rc::clone(&state);
        let mapper: Rc<dyn Fn(&Message) -> bool> = Rc::new(move |message| match message {
            Message::Command(command)
                if command.control == Some(hwnd)
                    && command.notification == CommandNotification::Clicked =>
            {
                let checked = !state_for_mapper.checked.get();
                state_for_mapper.set_checked(checked);
                let msg = events_for_mapper
                    .borrow()
                    .on_toggle
                    .as_ref()
                    .and_then(|f| f(checked));
                if let Some(msg) = msg {
                    sink.emit(msg);
                }
                true
            }
            Message::DrawItem {
                control,
                dc,
                state,
                area,
                ..
            } if *control == hwnd => {
                state_for_mapper.draw(*dc, *area, *state);
                true
            }
            _ => false,
        });
        registry::register_app_events(hwnd, mapper);

        let state_for_theme = Rc::clone(&state);
        crate::theme::register_themed(
            parent,
            hwnd,
            Rc::new(move |applied| {
                state_for_theme.theme.set(*applied);
                sys::window::invalidate(hwnd);
            }),
        );
        Ok(CheckBox {
            control: Control::own(hwnd, bounds),
            events,
            state,
        })
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
        self.state.checked.get()
    }

    /// Checks or unchecks the box.
    pub fn set_checked(&self, checked: bool) {
        self.state.set_checked(checked);
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
        self.state.theme.set(*theme);
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
        self.state.label.borrow().clone()
    }

    fn set_text(&self, text: &str) {
        self.state.label.replace(text.to_string());
        let _ = sys::window::set_title(self.control.hwnd(), text);
        sys::window::invalidate(self.control.hwnd());
    }
}
