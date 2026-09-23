#![forbid(unsafe_code)]

//! Pure-geometry layout helpers: docking strips and weighted stacks.
//!
//! This is deliberately not a layout engine, just the two arrangements a
//! window needs in `WM_SIZE`. Everything is expressed in 96-DPI design units
//! and scaled once, at split time, with [`dpi_scale`]; the functions never
//! touch Win32 and contain no `unsafe`.
//!
//! ```
//! use win32ui::prelude::*;
//!
//! let client = Rect::new(0, 0, 800, 600);
//! let areas = Dock::new().top(40).bottom(24).left(200).split(client, 96);
//! assert_eq!(areas.fill, Rect::new(200, 40, 800, 576));
//! ```

mod dock;
mod stack;

pub use dock::{Dock, DockLayout};
pub use stack::{Stack, StackDirection, StackSlot};

use crate::geometry::Rect;
use crate::window::dpi_scale;

/// Edge insets (margins) in 96-DPI design units.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Insets {
    /// Left edge.
    pub left: i32,
    /// Top edge.
    pub top: i32,
    /// Right edge.
    pub right: i32,
    /// Bottom edge.
    pub bottom: i32,
}

impl Insets {
    /// Creates insets from each edge.
    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Insets {
        Insets {
            left,
            top,
            right,
            bottom,
        }
    }

    /// The same inset on every edge.
    pub const fn all(value: i32) -> Insets {
        Insets::new(value, value, value, value)
    }

    /// `horizontal` on the left/right, `vertical` on the top/bottom.
    pub const fn symmetric(horizontal: i32, vertical: i32) -> Insets {
        Insets::new(horizontal, vertical, horizontal, vertical)
    }

    /// Shrinks `rect` by the insets, scaled to `dpi`. The result never has a
    /// negative width or height, so a parent smaller than the insets is empty
    /// rather than inverted.
    pub fn apply(self, rect: Rect, dpi: u32) -> Rect {
        let left = rect.left + dpi_scale(self.left, dpi);
        let top = rect.top + dpi_scale(self.top, dpi);
        let right = (rect.right - dpi_scale(self.right, dpi)).max(left);
        let bottom = (rect.bottom - dpi_scale(self.bottom, dpi)).max(top);
        Rect::new(left, top, right, bottom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insets_scale_and_clamp() {
        let rect = Rect::new(0, 0, 100, 100);
        assert_eq!(Insets::all(10).apply(rect, 96), Rect::new(10, 10, 90, 90));
        assert_eq!(
            Insets::new(10, 0, 0, 0).apply(rect, 192),
            Rect::new(20, 0, 100, 100)
        );
        assert_eq!(Insets::all(80).apply(rect, 96), Rect::new(80, 80, 80, 80));
    }
}
