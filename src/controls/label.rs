#![forbid(unsafe_code)]

//! A plain static-text label.

use crate::app::Ui;
use crate::controls::control::{AsControl, Control, HasText};
use crate::controls::{create_child, next_id, style};
use crate::error::Result;
use crate::geometry::Rect;
use crate::sys;

const SS_LEFT: u32 = 0x0000;

/// A static text control.
pub struct Label {
    control: Control,
}

impl Label {
    /// Creates a left-aligned label as a child of the window behind `ui`.
    pub fn new<M: 'static>(ui: &mut Ui<M>, bounds: Rect, text: &str) -> Result<Label> {
        let style = style::WS_CHILD | style::WS_VISIBLE | SS_LEFT;
        let hwnd = create_child("Label", "STATIC", ui.hwnd(), style, 0, next_id(), bounds)?;
        let label = Label {
            control: Control::own(hwnd, bounds),
        };
        label.set_text(text);
        Ok(label)
    }
}

impl AsControl for Label {
    fn control(&self) -> &Control {
        &self.control
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
