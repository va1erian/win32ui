#![forbid(unsafe_code)]

//! Pixel geometry, kept free of Win32 types so the whole crate (and its
//! callers) can do layout arithmetic without touching `RECT`.

/// A point in client or screen coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Point {
    /// Horizontal coordinate.
    pub x: i32,
    /// Vertical coordinate.
    pub y: i32,
}

impl Point {
    /// Creates a point.
    pub const fn new(x: i32, y: i32) -> Point {
        Point { x, y }
    }
}

/// A width/height pair.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Size {
    /// Width in pixels.
    pub width: i32,
    /// Height in pixels.
    pub height: i32,
}

impl Size {
    /// Creates a size.
    pub const fn new(width: i32, height: i32) -> Size {
        Size { width, height }
    }

    /// Whether either dimension is non-positive.
    pub const fn is_empty(self) -> bool {
        self.width <= 0 || self.height <= 0
    }
}

/// A rectangle, with the same half-open semantics as `RECT`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    /// Left edge.
    pub left: i32,
    /// Top edge.
    pub top: i32,
    /// Right edge (exclusive).
    pub right: i32,
    /// Bottom edge (exclusive).
    pub bottom: i32,
}

impl Rect {
    /// Creates a rectangle from its edges.
    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
        Rect {
            left,
            top,
            right,
            bottom,
        }
    }

    /// Creates a rectangle at the origin with the given size.
    pub const fn from_size(size: Size) -> Rect {
        Rect::new(0, 0, size.width, size.height)
    }

    /// Width in pixels.
    pub const fn width(self) -> i32 {
        self.right - self.left
    }

    /// Height in pixels.
    pub const fn height(self) -> i32 {
        self.bottom - self.top
    }

    /// The rectangle's size.
    pub const fn size(self) -> Size {
        Size::new(self.width(), self.height())
    }

    /// Whether either dimension is non-positive.
    pub const fn is_empty(self) -> bool {
        self.width() <= 0 || self.height() <= 0
    }

    /// Moves the rectangle so its top-left corner is `(x, y)`.
    pub const fn at(self, x: i32, y: i32) -> Rect {
        Rect::new(x, y, x + self.width(), y + self.height())
    }

    /// Returns the rectangle moved by `(dx, dy)`.
    pub const fn offset(self, dx: i32, dy: i32) -> Rect {
        Rect::new(
            self.left + dx,
            self.top + dy,
            self.right + dx,
            self.bottom + dy,
        )
    }

    /// Shrinks the rectangle by `amount` on every edge.
    pub const fn shrink(self, amount: i32) -> Rect {
        Rect::new(
            self.left + amount,
            self.top + amount,
            self.right - amount,
            self.bottom - amount,
        )
    }

    /// Whether `point` lies inside the rectangle.
    pub const fn contains(self, point: Point) -> bool {
        point.x >= self.left && point.x < self.right && point.y >= self.top && point.y < self.bottom
    }

    /// Splits off a left column of `width` pixels, returning it and the rest.
    pub fn split_left(self, width: i32) -> (Rect, Rect) {
        let split = (self.left + width).min(self.right);
        (
            Rect::new(self.left, self.top, split, self.bottom),
            Rect::new(split, self.top, self.right, self.bottom),
        )
    }

    /// Splits off a top row of `height` pixels, returning it and the rest.
    pub fn split_top(self, height: i32) -> (Rect, Rect) {
        let split = (self.top + height).min(self.bottom);
        (
            Rect::new(self.left, self.top, self.right, split),
            Rect::new(self.left, split, self.right, self.bottom),
        )
    }

    /// Splits off a bottom row of `height` pixels, returning the rest and it.
    pub fn split_bottom(self, height: i32) -> (Rect, Rect) {
        let split = (self.bottom - height).max(self.top);
        (
            Rect::new(self.left, self.top, self.right, split),
            Rect::new(self.left, split, self.right, self.bottom),
        )
    }
}

impl From<Size> for Rect {
    fn from(size: Size) -> Rect {
        Rect::from_size(size)
    }
}

/// The geometry types a frontend usually needs.
pub mod prelude {
    pub use super::{Point, Rect};
}
