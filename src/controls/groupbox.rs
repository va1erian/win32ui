#![forbid(unsafe_code)]

//! A labelled frame that visually groups related controls.
//!
//! The native group box frame ignores dark mode (it paints classic-light), so
//! this is an owner-drawn (`BS_OWNERDRAW`) button: the frame and title are
//! painted from theme tokens on `WM_DRAWITEM` (a documented API).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::app::Ui;
use crate::controls::control::{AsControl, Control, HasText};
use crate::controls::registry;
use crate::controls::{create_child, next_id, style};
use crate::error::Result;
use crate::gdi::Font;
use crate::geometry::Rect;
use crate::message::Message;
use crate::sys;
use crate::theme::{Theme, Themed};
use crate::units::dip;

/// A native group box (`BS_GROUPBOX`): a rectangle with a title in its upper
/// left corner that surrounds related controls. It cannot be selected and
/// takes no part in the tab order; its only purpose is visual grouping.
pub struct GroupBox {
    control: Control,
    title: Rc<RefCell<String>>,
    theme: Rc<Cell<Theme>>,
}

impl GroupBox {
    /// Creates the frame as a child of the window behind `ui`, adopting
    /// `ui`'s theme. Size it with a layout (e.g. `.height(…)`) or
    /// [`ControlExt::set_bounds`](crate::ControlExt::set_bounds).
    pub fn new<M: 'static>(ui: &mut Ui<M>, title: &str) -> Result<GroupBox> {
        let dpi = ui.dpi();
        let parent = ui.hwnd();
        let font = Rc::new(Font::system_ui(dpi)?);
        let text_width = sys::gdi::measure_text(font.raw(), title).width;
        let width = (text_width + dip(32.0).to_px(dpi).value()).max(dip(96.0).to_px(dpi).value());
        let height = font.pixel_height() + dip(14.0).to_px(dpi).value();
        let bounds = Rect::new(0, 0, width, height);
        let style = style::WS_CHILD
            | style::WS_VISIBLE
            | sys::button_draw::owner_drawn(sys::button::groupbox_style());
        let hwnd = create_child("GroupBox", "BUTTON", parent, style, 0, next_id(), bounds)?;
        sys::control::set_control_font(hwnd, font.raw());
        let _ = sys::window::set_title(hwnd, title);

        let title_cell = Rc::new(RefCell::new(title.to_string()));
        let theme_cell = Rc::new(Cell::new(ui.theme()));
        let mapper_title = Rc::clone(&title_cell);
        let mapper_theme = Rc::clone(&theme_cell);
        let mapper_font = Rc::clone(&font);
        let mapper: Rc<dyn Fn(&Message) -> bool> = Rc::new(move |message| {
            let Message::DrawItem {
                control, dc, area, ..
            } = message
            else {
                return false;
            };
            if *control != hwnd {
                return false;
            }
            let theme = mapper_theme.get();
            let paint = sys::button_draw::GroupPaint {
                text: theme.text,
                border: theme.border,
                background: theme.background,
            };
            sys::button_draw::draw_groupbox(
                *dc,
                *area,
                &mapper_title.borrow(),
                mapper_font.raw(),
                &paint,
            );
            true
        });
        registry::register_app_events(hwnd, mapper);

        let theme_for_callback = Rc::clone(&theme_cell);
        crate::theme::register_themed(
            parent,
            hwnd,
            Rc::new(move |applied| {
                theme_for_callback.set(*applied);
                sys::window::invalidate(hwnd);
            }),
        );
        Ok(GroupBox {
            control: Control::own(hwnd, bounds),
            title: title_cell,
            theme: theme_cell,
        })
    }
}

impl AsControl for GroupBox {
    fn control(&self) -> &Control {
        &self.control
    }
}

impl Themed for GroupBox {
    fn apply_theme(&self, theme: &Theme) {
        self.theme.set(*theme);
        sys::window::invalidate(self.control.hwnd());
    }
}

impl Drop for GroupBox {
    fn drop(&mut self) {
        registry::unregister_app_events(self.control.hwnd());
        crate::theme::unregister_themed(self.control.hwnd());
    }
}

impl HasText for GroupBox {
    fn text(&self) -> String {
        self.title.borrow().clone()
    }

    fn set_text(&self, text: &str) {
        self.title.replace(text.to_string());
        let _ = sys::window::set_title(self.control.hwnd(), text);
        sys::window::invalidate(self.control.hwnd());
    }
}
