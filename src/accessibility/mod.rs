#![forbid(unsafe_code)]

//! Accessibility: what a widget tells assistive technology and UI Automation
//! clients (screen readers, test drivers, other agents).
//!
//! Native controls are exposed by Windows itself. A
//! [`CustomWidget`](crate::CustomWidget) paints its own pixels, so it
//! describes itself with a [`Node`] tree through
//! [`CustomWidget::accessibility`](crate::CustomWidget::accessibility) and
//! handles the [`Action`]s a client performs on it through
//! [`CustomWidget::accessibility_action`](crate::CustomWidget::accessibility_action).
//!
//! The tree is a *snapshot*: the widget builds it on demand, when a client
//! asks, so it can never go stale and holds no reference back to the widget.
//! A child is addressed by its path (the index chain from the root), which is
//! what an [`Action`] carries back.

mod node;
pub(crate) mod registry;

pub use node::{Action, Node, RangeValue, Role};

use crate::geometry::Rect;

/// What a widget can ask about itself while it builds its accessibility tree:
/// its current client size and DPI, so it can lay node bounds out the way it
/// paints.
#[derive(Clone, Copy, Debug)]
pub struct AccessCx {
    bounds: Rect,
    dpi: u32,
}

impl AccessCx {
    pub(crate) fn new(bounds: Rect, dpi: u32) -> AccessCx {
        AccessCx { bounds, dpi }
    }

    /// The widget's client area in device pixels, at the origin.
    pub fn bounds(&self) -> Rect {
        self.bounds
    }

    /// The widget's dots-per-inch.
    pub fn dpi(&self) -> u32 {
        self.dpi
    }
}
