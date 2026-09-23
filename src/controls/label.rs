#![forbid(unsafe_code)]

//! A plain static-text label.

use crate::controls::{create_child, style};
use crate::error::Result;
use crate::geometry::Rect;
use crate::hwnd::Hwnd;
use crate::sys;

const SS_LEFT: u32 = 0x0000;

/// A static text control.
pub struct Label {
    hwnd: Hwnd,
}

impl Label {
    /// Creates a left-aligned label as a child of `parent`.
    pub fn new(parent: Hwnd, id: usize, bounds: Rect, text: &str) -> Result<Label> {
        let style = style::WS_CHILD | style::WS_VISIBLE | SS_LEFT;
        let hwnd = create_child("Label", "STATIC", parent, style, 0, id, bounds)?;
        let label = Label { hwnd };
        label.set_text(text);
        Ok(label)
    }

    /// Replaces the label's text.
    pub fn set_text(&self, text: &str) {
        let _ = sys::window::set_title(self.hwnd, text);
    }

    /// Moves/resizes the label.
    pub fn set_bounds(&self, bounds: Rect) {
        sys::window::move_window(self.hwnd, bounds);
    }

    /// The control handle.
    pub fn hwnd(&self) -> Hwnd {
        self.hwnd
    }
}

impl Drop for Label {
    fn drop(&mut self) {
        sys::window::destroy(self.hwnd);
    }
}
