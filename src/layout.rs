#![forbid(unsafe_code)]

//! Pure-geometry layout helpers: docking strips and weighted stacks.
//!
//! This is deliberately not a layout engine, just the two arrangements a
//! window needs in `WM_SIZE`. Everything is expressed in [`Dip`] design values
//! and scaled once, at split time; the functions never touch Win32 and contain
//! no `unsafe`.
//!
//! ```
//! use win32ui::prelude::*;
//!
//! let client = Rect::new(0, 0, 800, 600);
//! let areas = Dock::new()
//!     .top(dip(40.0))
//!     .bottom(dip(24.0))
//!     .left(dip(200.0))
//!     .split(client, 96);
//! assert_eq!(areas.fill, Rect::new(200, 40, 800, 576));
//! ```

mod dock;
mod stack;

pub use dock::{Dock, DockLayout};
pub use stack::{Stack, StackDirection, StackSlot};

use crate::geometry::Rect;
use crate::units::Dip;

/// Edge insets (margins) in [`Dip`] design units.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Insets {
    /// Left edge.
    pub left: Dip,
    /// Top edge.
    pub top: Dip,
    /// Right edge.
    pub right: Dip,
    /// Bottom edge.
    pub bottom: Dip,
}

impl Insets {
    /// Creates insets from each edge.
    pub const fn new(left: Dip, top: Dip, right: Dip, bottom: Dip) -> Insets {
        Insets {
            left,
            top,
            right,
            bottom,
        }
    }

    /// The same inset on every edge.
    pub const fn all(value: Dip) -> Insets {
        Insets::new(value, value, value, value)
    }

    /// `horizontal` on the left/right, `vertical` on the top/bottom.
    pub const fn symmetric(horizontal: Dip, vertical: Dip) -> Insets {
        Insets::new(horizontal, vertical, horizontal, vertical)
    }

    /// Shrinks `rect` by the insets, scaled to `dpi`. The result never has a
    /// negative width or height, so a parent smaller than the insets is empty
    /// rather than inverted.
    pub fn apply(self, rect: Rect, dpi: u32) -> Rect {
        let left = rect.left + self.left.to_px(dpi).value();
        let top = rect.top + self.top.to_px(dpi).value();
        let right = (rect.right - self.right.to_px(dpi).value()).max(left);
        let bottom = (rect.bottom - self.bottom.to_px(dpi).value()).max(top);
        Rect::new(left, top, right, bottom)
    }
}

/// The layout-arithmetic types a frontend usually needs.
pub mod prelude {
    pub use super::{Dock, DockLayout, Insets, Stack, StackDirection, StackSlot};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insets_scale_and_clamp() {
        use crate::units::dip;

        let rect = Rect::new(0, 0, 100, 100);
        assert_eq!(
            Insets::all(dip(10.0)).apply(rect, 96),
            Rect::new(10, 10, 90, 90)
        );
        assert_eq!(
            Insets::new(dip(10.0), dip(0.0), dip(0.0), dip(0.0)).apply(rect, 192),
            Rect::new(20, 0, 100, 100)
        );
        assert_eq!(
            Insets::all(dip(80.0)).apply(rect, 96),
            Rect::new(80, 80, 80, 80)
        );
    }
}
