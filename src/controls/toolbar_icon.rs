#![forbid(unsafe_code)]

//! Vector icons for the toolbar.
//!
//! A few small shapes drawn with the same anti-aliased [`Canvas`] primitives as
//! every other owner-drawn control, so an icon stays crisp at every DPI instead
//! of being a scaled 16px bitmap.

use crate::color::Color;
use crate::gdi::Canvas;
use crate::geometry::{Point, Rect};

/// A small vector icon drawn on a toolbar button.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ToolbarIcon {
    /// A filled circle.
    Circle,
    /// A chevron pointing down.
    Chevron,
    /// A check mark.
    Check,
    /// An arrow pointing right.
    Arrow,
    /// A close (X) mark.
    Close,
}

/// Draws `icon` in the square `rect` in `color`. The stroke scales with the
/// icon so it stays proportional at every DPI.
pub(crate) fn draw_icon(canvas: &Canvas, icon: ToolbarIcon, rect: Rect, color: Color) {
    if rect.is_empty() {
        return;
    }
    let stroke = (rect.width() / 8).max(1);
    let center_y = rect.top + rect.height() / 2;
    match icon {
        ToolbarIcon::Circle => {
            canvas.round_rect(rect, rect.width().min(rect.height()) / 2, color, None);
        }
        ToolbarIcon::Chevron => {
            let top = rect.top + rect.height() / 4;
            let bottom = rect.bottom - rect.height() / 4;
            let middle = rect.left + rect.width() / 2;
            canvas.line(
                Point::new(rect.left, top),
                Point::new(middle, bottom),
                color,
                stroke,
            );
            canvas.line(
                Point::new(middle, bottom),
                Point::new(rect.right, top),
                color,
                stroke,
            );
        }
        ToolbarIcon::Check => {
            let left = rect.left + rect.width() / 6;
            let middle = rect.left + rect.width() * 2 / 5;
            let right = rect.right - rect.width() / 6;
            let top = rect.top + rect.height() / 4;
            let bottom = rect.bottom - rect.height() / 4;
            canvas.line(
                Point::new(left, center_y),
                Point::new(middle, bottom),
                color,
                stroke,
            );
            canvas.line(
                Point::new(middle, bottom),
                Point::new(right, top),
                color,
                stroke,
            );
        }
        ToolbarIcon::Arrow => {
            let left = rect.left + rect.width() / 6;
            let right = rect.right - rect.width() / 6;
            let head = rect.height() / 5;
            canvas.line(
                Point::new(left, center_y),
                Point::new(right, center_y),
                color,
                stroke,
            );
            canvas.line(
                Point::new(right - head, center_y - head),
                Point::new(right, center_y),
                color,
                stroke,
            );
            canvas.line(
                Point::new(right, center_y),
                Point::new(right - head, center_y + head),
                color,
                stroke,
            );
        }
        ToolbarIcon::Close => {
            let inset = rect.width() / 4;
            canvas.line(
                Point::new(rect.left + inset, rect.top + inset),
                Point::new(rect.right - inset, rect.bottom - inset),
                color,
                stroke,
            );
            canvas.line(
                Point::new(rect.right - inset, rect.top + inset),
                Point::new(rect.left + inset, rect.bottom - inset),
                color,
                stroke,
            );
        }
    }
}
