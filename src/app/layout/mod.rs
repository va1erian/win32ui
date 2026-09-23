#![forbid(unsafe_code)]

//! The widget-facing layout tree: `column!`/`row!` turn widgets into a tree the
//! window lays out again on every resize and DPI change.
//!
//! Items are any [`AsControl`] or a nested [`Layout`]. [`LayoutExt`] sizes an
//! item: `fill`/`min`/`fixed` along the parent's main axis, `width`/`height`
//! along their own axis, and by default a widget keeps its current size. Hidden
//! widgets take no space. [`Layout::compute`] is pure: it maps the tree and a
//! parent rectangle to one rectangle per visible leaf, using the existing
//! [`Stack`] arithmetic.

use crate::geometry::Rect;
use crate::layout::{Insets, Stack, StackDirection};
use crate::units::Dip;

#[cfg(test)]
mod tests;

mod item;
pub(crate) mod split;
pub(crate) mod tabs;

pub(crate) use item::{Content, Placed, Sizing, WidgetHandle};
pub use item::{IntoLayoutItem, LayoutExt, LayoutItem};

/// A row or column of items, laid out as a tree the window owns.
///
/// Build one with [`row!`](crate::row) / [`column!`](crate::column) or
/// [`Layout::row`] / [`Layout::column`], and install it with
/// [`Ui::set_layout`](crate::Ui::set_layout).
#[derive(Clone)]
pub struct Layout {
    direction: StackDirection,
    spacing: Dip,
    margins: Insets,
    slots: Vec<LayoutItem>,
}

impl Layout {
    /// A layout that places its items left to right.
    pub const fn row() -> Layout {
        Layout {
            direction: StackDirection::Horizontal,
            spacing: Dip(0.0),
            margins: Insets::new(Dip(0.0), Dip(0.0), Dip(0.0), Dip(0.0)),
            slots: Vec::new(),
        }
    }

    /// A layout that places its items top to bottom.
    pub const fn column() -> Layout {
        Layout {
            direction: StackDirection::Vertical,
            spacing: Dip(0.0),
            margins: Insets::new(Dip(0.0), Dip(0.0), Dip(0.0), Dip(0.0)),
            slots: Vec::new(),
        }
    }

    /// The gap between adjacent items, in design units.
    pub const fn spacing(mut self, spacing: Dip) -> Layout {
        self.spacing = spacing;
        self
    }

    /// Margins inside the parent, in design units.
    pub const fn margins(mut self, insets: Insets) -> Layout {
        self.margins = insets;
        self
    }

    /// Appends a widget or a nested layout.
    pub fn item<I: IntoLayoutItem>(mut self, item: I) -> Layout {
        self.slots.push(item.into_layout_item());
        self
    }

    /// The installed items, for the window's split-binding pass.
    pub(crate) fn items(&self) -> &[LayoutItem] {
        &self.slots
    }

    /// Wraps this layout as a weighted item of its parent.
    pub fn fill(self, weight: u32) -> LayoutItem {
        self.item_with(Sizing::Fill(weight))
    }

    /// Wraps this layout as a fixed-size item of its parent.
    pub fn fixed(self, size: Dip) -> LayoutItem {
        self.item_with(Sizing::Fixed(size))
    }

    /// Wraps this layout with a fixed width, whichever axis that is.
    pub fn width(self, size: Dip) -> LayoutItem {
        self.item_with(Sizing::Width(size))
    }

    /// Wraps this layout with a fixed height, whichever axis that is.
    pub fn height(self, size: Dip) -> LayoutItem {
        self.item_with(Sizing::Height(size))
    }

    /// Wraps this layout as a minimum-size item of its parent.
    pub fn min(self, size: Dip) -> LayoutItem {
        self.item_with(Sizing::Min(size))
    }

    fn item_with(self, sizing: Sizing) -> LayoutItem {
        LayoutItem {
            content: Content::Nested(Box::new(self)),
            sizing,
        }
    }

    /// Lays the tree out inside `rect`, returning one entry per visible leaf.
    pub(crate) fn compute(&self, rect: Rect, dpi: u32) -> Vec<Placed> {
        let visible: Vec<&LayoutItem> =
            self.slots.iter().filter(|item| item.is_visible()).collect();
        if visible.is_empty() {
            return Vec::new();
        }

        let mut stack = match self.direction {
            StackDirection::Horizontal => Stack::horizontal(),
            StackDirection::Vertical => Stack::vertical(),
        }
        .spacing(self.spacing)
        .margins(self.margins);
        for item in &visible {
            stack = stack.push(item.stack_slot(self.direction));
        }

        let areas = stack.split(rect, dpi);
        let mut placed = Vec::new();
        for (item, area) in visible.iter().zip(areas) {
            let area = match item.cross_extent(self.direction) {
                Some(size) => cross_rect(area, self.direction, size.to_px(dpi).value()),
                None => area,
            };
            item.compute(area, dpi, &mut placed);
        }
        placed
    }
}

/// Narrows `rect` to `extent` pixels along the cross axis, keeping the start
/// edge (the top for a row, the left for a column).
fn cross_rect(rect: Rect, direction: StackDirection, extent: i32) -> Rect {
    match direction {
        StackDirection::Horizontal => Rect::new(
            rect.left,
            rect.top,
            rect.right,
            (rect.top + extent).min(rect.bottom),
        ),
        StackDirection::Vertical => Rect::new(
            rect.left,
            rect.top,
            (rect.left + extent).min(rect.right),
            rect.bottom,
        ),
    }
}

/// Builds a [`Layout`] that places its items top to bottom.
#[macro_export]
macro_rules! column {
    ($($item:expr),* $(,)?) => {
        $crate::Layout::column()$(.item(&$item))*
    };
}

/// Builds a [`Layout`] that places its items left to right.
#[macro_export]
macro_rules! row {
    ($($item:expr),* $(,)?) => {
        $crate::Layout::row()$(.item(&$item))*
    };
}

/// Builds a [`Split`](crate::Split) with two side-by-side panes.
///
/// ```ignore
/// split_row![tree, list]
///     .position(dip(220.0))
///     .min(dip(120.0), dip(200.0))
///     .on_moved(|p| Some(Msg::SplitMoved(p)))
/// ```
#[macro_export]
macro_rules! split_row {
    ($a:expr, $b:expr $(,)?) => {
        $crate::Split::row().a(&$a).b(&$b)
    };
}

/// Builds a [`Split`](crate::Split) with two stacked panes.
#[macro_export]
macro_rules! split_col {
    ($a:expr, $b:expr $(,)?) => {
        $crate::Split::column().a(&$a).b(&$b)
    };
}

/// Builds a [`Tabs`](crate::Tabs) node from `(title, page)` pairs, where each
/// page is any widget or nested layout.
///
/// ```ignore
/// tabs![("General", general_layout), ("Accounts", accounts_layout)]
///     .on_change(|index| Some(Msg::Tab(index)))
/// ```
#[macro_export]
macro_rules! tabs {
    ($( ($title:expr, $item:expr) ),* $(,)?) => {
        $crate::Tabs::new()$(.page($title, &$item))*
    };
}
