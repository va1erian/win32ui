#![forbid(unsafe_code)]

//! Docking: carve fixed strips off the edges of a rectangle.

use super::Insets;
use crate::geometry::Rect;
use crate::window::dpi_scale;

/// A strip size for [`Dock`]: design units or already-scaled device pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Extent {
    Design(i32),
    Pixels(i32),
}

impl Extent {
    fn resolve(self, dpi: u32) -> i32 {
        match self {
            Extent::Design(value) => dpi_scale(value, dpi),
            Extent::Pixels(value) => value,
        }
        .max(0)
    }
}

/// The rectangles produced by [`Dock::split`]. A side is `None` when no strip
/// was requested for it; `fill` is whatever is left in the middle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DockLayout {
    /// Top strip.
    pub top: Option<Rect>,
    /// Bottom strip.
    pub bottom: Option<Rect>,
    /// Left strip.
    pub left: Option<Rect>,
    /// Right strip.
    pub right: Option<Rect>,
    /// The remaining area.
    pub fill: Rect,
}

/// Carves fixed strips off the edges of a rectangle.
///
/// Strips are taken in the order top, bottom, left, right, so the horizontal
/// strips span the full width and the vertical strips fit between them. Sizes
/// are 96-DPI design units unless a `*_px` builder is used.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Dock {
    insets: Insets,
    top: Option<Extent>,
    bottom: Option<Extent>,
    left: Option<Extent>,
    right: Option<Extent>,
}

impl Dock {
    /// An empty dock.
    pub const fn new() -> Dock {
        Dock {
            insets: Insets::new(0, 0, 0, 0),
            top: None,
            bottom: None,
            left: None,
            right: None,
        }
    }

    /// Shrinks the input rect by `insets` before carving (design units).
    pub const fn margins(self, insets: Insets) -> Dock {
        Dock { insets, ..self }
    }

    /// A top strip `height` design units tall.
    pub const fn top(self, height: i32) -> Dock {
        Dock {
            top: Some(Extent::Design(height)),
            ..self
        }
    }

    /// A bottom strip `height` design units tall.
    pub const fn bottom(self, height: i32) -> Dock {
        Dock {
            bottom: Some(Extent::Design(height)),
            ..self
        }
    }

    /// A left strip `width` design units wide.
    pub const fn left(self, width: i32) -> Dock {
        Dock {
            left: Some(Extent::Design(width)),
            ..self
        }
    }

    /// A right strip `width` design units wide.
    pub const fn right(self, width: i32) -> Dock {
        Dock {
            right: Some(Extent::Design(width)),
            ..self
        }
    }

    /// Like [`Dock::top`], but `height` is already in device pixels.
    pub const fn top_px(self, height: i32) -> Dock {
        Dock {
            top: Some(Extent::Pixels(height)),
            ..self
        }
    }

    /// Like [`Dock::bottom`], but `height` is already in device pixels.
    pub const fn bottom_px(self, height: i32) -> Dock {
        Dock {
            bottom: Some(Extent::Pixels(height)),
            ..self
        }
    }

    /// Like [`Dock::left`], but `width` is already in device pixels.
    pub const fn left_px(self, width: i32) -> Dock {
        Dock {
            left: Some(Extent::Pixels(width)),
            ..self
        }
    }

    /// Like [`Dock::right`], but `width` is already in device pixels.
    pub const fn right_px(self, width: i32) -> Dock {
        Dock {
            right: Some(Extent::Pixels(width)),
            ..self
        }
    }

    /// Carves the requested strips off `rect`, scaling design units to `dpi`.
    pub fn split(&self, rect: Rect, dpi: u32) -> DockLayout {
        let mut remaining = self.insets.apply(rect, dpi);
        let top = self.top.map(|extent| {
            let (strip, rest) = remaining.split_top(extent.resolve(dpi));
            remaining = rest;
            strip
        });
        let bottom = self.bottom.map(|extent| {
            let (rest, strip) = remaining.split_bottom(extent.resolve(dpi));
            remaining = rest;
            strip
        });
        let left = self.left.map(|extent| {
            let (strip, rest) = remaining.split_left(extent.resolve(dpi));
            remaining = rest;
            strip
        });
        let right = self.right.map(|extent| {
            let split = (remaining.right - extent.resolve(dpi)).max(remaining.left);
            let strip = Rect::new(split, remaining.top, remaining.right, remaining.bottom);
            remaining = Rect::new(remaining.left, remaining.top, split, remaining.bottom);
            strip
        });
        DockLayout {
            top,
            bottom,
            left,
            right,
            fill: remaining,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dock_carves_in_order() {
        let rect = Rect::new(0, 0, 100, 100);
        let areas = Dock::new()
            .top(10)
            .bottom(10)
            .left(10)
            .right(10)
            .split(rect, 96);
        assert_eq!(areas.top, Some(Rect::new(0, 0, 100, 10)));
        assert_eq!(areas.bottom, Some(Rect::new(0, 90, 100, 100)));
        assert_eq!(areas.left, Some(Rect::new(0, 10, 10, 90)));
        assert_eq!(areas.right, Some(Rect::new(90, 10, 100, 90)));
        assert_eq!(areas.fill, Rect::new(10, 10, 90, 90));
    }

    #[test]
    fn dock_scales_design_units_but_not_pixels() {
        let rect = Rect::new(0, 0, 200, 200);
        let areas = Dock::new().top(10).bottom_px(30).split(rect, 192);
        assert_eq!(areas.top, Some(Rect::new(0, 0, 200, 20)));
        assert_eq!(areas.bottom, Some(Rect::new(0, 170, 200, 200)));
    }

    #[test]
    fn dock_with_margins_leaves_fill() {
        let rect = Rect::new(0, 0, 100, 100);
        let areas = Dock::new().margins(Insets::all(5)).top(10).split(rect, 96);
        assert_eq!(areas.top, Some(Rect::new(5, 5, 95, 15)));
        assert_eq!(areas.fill, Rect::new(5, 15, 95, 95));
    }
}
