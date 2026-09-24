#![forbid(unsafe_code)]

//! A plain static-text label.

use std::rc::Rc;

use crate::app::Ui;
use crate::controls::control::{AsControl, Control, ControlExt, HasText};
use crate::controls::{create_child, next_id, style};
use crate::error::Result;
use crate::geometry::Rect;
use crate::sys;
use crate::theme::{Theme, Themed};

const SS_LEFT: u32 = 0x0000;

/// A static text control. Its colours come from the window theme through the
/// central `WM_CTLCOLORSTATIC` answer.
pub struct Label {
    control: Control,
}

impl Label {
    /// Creates a left-aligned label as a child of the window behind `ui`,
    /// adopting `ui`'s theme.
    pub fn new<M: 'static>(ui: &mut Ui<M>, bounds: Rect, text: &str) -> Result<Label> {
        use crate::gdi::Font;

        let style = style::WS_CHILD | style::WS_VISIBLE | SS_LEFT;
        let parent = ui.hwnd();
        let dpi = ui.dpi();
        let hwnd = create_child("Label", "STATIC", parent, style, 0, next_id(), bounds)?;
        let font = Font::system_ui(dpi)?;

        let label = Label {
            control: Control::own(hwnd, bounds),
        };
        // Store the font in the control so it stays alive
        label.set_font(font);
        sys::apply_native_theme(hwnd, sys::NativeControlKind::Button, ui.theme().is_dark);
        label.set_text(text);
        crate::theme::register_themed(
            parent,
            hwnd,
            Rc::new(move |applied| {
                sys::apply_native_theme(hwnd, sys::NativeControlKind::Button, applied.is_dark);
                sys::window::invalidate(hwnd);
            }),
        );
        Ok(label)
    }
}

impl AsControl for Label {
    fn control(&self) -> &Control {
        &self.control
    }
}

impl Themed for Label {
    fn apply_theme(&self, theme: &Theme) {
        // Colours are supplied by the central `WM_CTLCOLORSTATIC` answer; the
        // native button chrome follows the theme and a repaint picks it up.
        sys::apply_native_theme(
            self.control.hwnd(),
            sys::NativeControlKind::Button,
            theme.is_dark,
        );
        sys::window::invalidate(self.control.hwnd());
    }
}

impl Drop for Label {
    fn drop(&mut self) {
        crate::theme::unregister_themed(self.control.hwnd());
    }
}

impl HasText for Label {
    fn text(&self) -> String {
        sys::window::get_title(self.control.hwnd())
    }

    fn set_text(&self, text: &str) {
        let _ = sys::window::set_title(self.control.hwnd(), text);
    }
}
