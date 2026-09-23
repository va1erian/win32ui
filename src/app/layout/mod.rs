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

use std::cell::Cell;
use std::rc::Rc;

use crate::controls::control::{AsControl, Control};
use crate::geometry::Rect;
use crate::hwnd::Hwnd;
use crate::layout::{Insets, Stack, StackDirection, StackSlot};
use crate::units::{Dip, Px};

#[cfg(test)]
mod tests;

/// How an item is sized: along the parent's main axis, or, for `width`/
/// `height`, along a named axis.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Sizing {
    /// The widget's natural size along the main axis.
    Auto,
    /// Exactly this many design units along the main axis.
    Fixed(Dip),
    /// At least this many design units along the main axis; shrinks only when
    /// the parent is too small to honour every item.
    Min(Dip),
    /// A share of the leftover main-axis space, proportional to its weight.
    Fill(u32),
    /// Exactly this many design units along the width axis.
    Width(Dip),
    /// Exactly this many design units along the height axis.
    Height(Dip),
}

/// A widget the layout can move: its handle plus the shared cells that keep the
/// widget's [`Control`] bounds and visibility in sync with the OS window.
#[derive(Clone)]
pub(crate) struct WidgetHandle {
    hwnd: Hwnd,
    bounds: Rc<Cell<Rect>>,
    visible: Rc<Cell<bool>>,
}

impl WidgetHandle {
    fn from_control(control: &Control) -> WidgetHandle {
        WidgetHandle {
            hwnd: control.hwnd(),
            bounds: control.bounds_handle(),
            visible: control.visible_handle(),
        }
    }

    /// Whether the widget takes part in layout.
    fn is_visible(&self) -> bool {
        self.visible.get()
    }

    /// The widget's current extent along `direction`, in device pixels.
    fn natural(&self, direction: StackDirection) -> i32 {
        let bounds = self.bounds.get();
        let extent = match direction {
            StackDirection::Horizontal => bounds.width(),
            StackDirection::Vertical => bounds.height(),
        };
        extent.max(0)
    }

    /// Records the bounds the layout assigned (the OS move is batched).
    pub(crate) fn set_bounds(&self, bounds: Rect) {
        self.bounds.set(bounds);
    }

    /// The widget's handle.
    pub(crate) fn hwnd(&self) -> Hwnd {
        self.hwnd
    }
}

/// One widget positioned by [`Layout::compute`].
pub(crate) struct Placed {
    pub(crate) handle: WidgetHandle,
    pub(crate) rect: Rect,
}

/// Wraps a widget's control with an explicit sizing.
fn widget_item(control: &Control, sizing: Sizing) -> LayoutItem {
    LayoutItem {
        content: Content::Widget(WidgetHandle::from_control(control)),
        sizing,
    }
}

/// What a [`LayoutItem`] contains. This is the seam for new node kinds: a split
/// divider (#11) or a stack of pages (#15) becomes another variant, handled in
/// [`Layout::compute`] alongside the nested layout.
#[derive(Clone)]
enum Content {
    Widget(WidgetHandle),
    Nested(Box<Layout>),
}

/// One entry in a [`Layout`]: a widget or a nested layout, with its sizing.
///
/// Built for you by [`LayoutExt`] and the [`Layout`] builders, so it is rarely
/// named directly.
#[derive(Clone)]
pub struct LayoutItem {
    content: Content,
    sizing: Sizing,
}

impl LayoutItem {
    fn is_visible(&self) -> bool {
        match &self.content {
            Content::Widget(handle) => handle.is_visible(),
            Content::Nested(nested) => nested.slots.iter().any(LayoutItem::is_visible),
        }
    }

    fn stack_slot(&self, direction: StackDirection) -> StackSlot {
        match self.sizing {
            Sizing::Fill(weight) => StackSlot::Fill(weight),
            Sizing::Fixed(size) => StackSlot::Fixed(size),
            Sizing::Min(size) => StackSlot::Min(size),
            Sizing::Width(size) if direction == StackDirection::Horizontal => {
                StackSlot::Fixed(size)
            }
            Sizing::Height(size) if direction == StackDirection::Vertical => StackSlot::Fixed(size),
            // A named-axis size that does not match the main axis sizes the
            // cross axis instead, so the main axis keeps its natural size.
            Sizing::Auto | Sizing::Width(_) | Sizing::Height(_) => match &self.content {
                Content::Widget(handle) => StackSlot::FixedPx(Px(handle.natural(direction))),
                Content::Nested(_) => StackSlot::Fill(1),
            },
        }
    }

    /// The cross-axis extent this item asked for, if any.
    fn cross_extent(&self, direction: StackDirection) -> Option<Dip> {
        match (self.sizing, direction) {
            (Sizing::Width(size), StackDirection::Vertical) => Some(size),
            (Sizing::Height(size), StackDirection::Horizontal) => Some(size),
            _ => None,
        }
    }
}

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
            match &item.content {
                Content::Widget(handle) => placed.push(Placed {
                    handle: handle.clone(),
                    rect: area,
                }),
                Content::Nested(nested) => placed.extend(nested.compute(area, dpi)),
            }
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

/// Converts into a [`LayoutItem`], so [`row!`](crate::row) /
/// [`column!`](crate::column) accept widgets and nested layouts alike.
pub trait IntoLayoutItem {
    /// Wraps `self` as a layout item.
    fn into_layout_item(self) -> LayoutItem;
}

impl<T: AsControl + ?Sized> IntoLayoutItem for &T {
    fn into_layout_item(self) -> LayoutItem {
        widget_item(self.control(), Sizing::Auto)
    }
}

impl IntoLayoutItem for &LayoutItem {
    fn into_layout_item(self) -> LayoutItem {
        self.clone()
    }
}

impl IntoLayoutItem for &Layout {
    fn into_layout_item(self) -> LayoutItem {
        LayoutItem {
            content: Content::Nested(Box::new(self.clone())),
            sizing: Sizing::Fill(1),
        }
    }
}

impl IntoLayoutItem for LayoutItem {
    fn into_layout_item(self) -> LayoutItem {
        self
    }
}

impl IntoLayoutItem for Layout {
    fn into_layout_item(self) -> LayoutItem {
        LayoutItem {
            content: Content::Nested(Box::new(self)),
            sizing: Sizing::Fill(1),
        }
    }
}

/// Builder methods that turn any widget into a [`LayoutItem`].
pub trait LayoutExt: AsControl {
    /// Wraps the widget with its natural size along the parent's main axis.
    fn layout_item(&self) -> LayoutItem {
        widget_item(self.control(), Sizing::Auto)
    }

    /// A share of the parent's leftover space.
    fn fill(&self, weight: u32) -> LayoutItem {
        widget_item(self.control(), Sizing::Fill(weight))
    }

    /// A fixed size along the parent's main axis.
    fn fixed(&self, size: Dip) -> LayoutItem {
        widget_item(self.control(), Sizing::Fixed(size))
    }

    /// A fixed width: the main axis in a row, the cross axis in a column.
    fn width(&self, size: Dip) -> LayoutItem {
        widget_item(self.control(), Sizing::Width(size))
    }

    /// A fixed height: the main axis in a column, the cross axis in a row.
    fn height(&self, size: Dip) -> LayoutItem {
        widget_item(self.control(), Sizing::Height(size))
    }

    /// A minimum size along the parent's main axis.
    fn min(&self, size: Dip) -> LayoutItem {
        widget_item(self.control(), Sizing::Min(size))
    }
}

impl<T: AsControl + ?Sized> LayoutExt for T {}

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
