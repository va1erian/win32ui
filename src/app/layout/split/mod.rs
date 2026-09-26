#![forbid(unsafe_code)]

//! Split layout nodes: two panes separated by a draggable divider.
//!
//! [`Split`] is a layout node, not a free-standing control: build it with
//! [`split_row!`](crate::split_row) / [`split_col!`](crate::split_col), size it
//! with [`Split::position`] and [`Split::min`], and install it with
//! [`Ui::set_layout`](crate::Ui::set_layout). The divider is a small owner-drawn
//! child window ([`CustomWidget`](crate::CustomWidget)): it shows a resize
//! cursor on hover, captures the mouse while dragging, relayouts live, and moves
//! by arrow keys when focused. [`Split::on_moved`] maps a move to the app's
//! `Msg` so the position can be persisted.

mod divider;

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::app::Ui;
use crate::geometry::Rect;
use crate::hwnd::Hwnd;
use crate::layout::StackDirection;
use crate::sys;
use crate::units::Dip;
use crate::window::{Window, WindowClass, WindowExStyle, WindowStyle};

use super::{Content, IntoLayoutItem, LayoutItem, Placed, Sizing, WidgetHandle};

use divider::{Divider, DividerHandler, SplitEvent};

/// The divider's thickness, in design units.
const DIVIDER_DIP: f32 = 5.0;
/// How far an arrow key moves the divider, in design units.
const ARROW_STEP_DIP: f32 = 8.0;

/// The state a split shares with its divider window and the layout tree.
pub(crate) struct SplitShared {
    direction: StackDirection,
    /// The anchored pane's extent, in design units; `None` until first laid
    /// out. The anchored pane is the first (left/top) pane by default, or the
    /// second (right/bottom) one when [`Split::position_b`] was used.
    position: Cell<Option<f32>>,
    /// Whether [`position`](Self::position) is the *second* pane's extent, so
    /// the split is anchored from the end.
    anchor_end: Cell<bool>,
    /// Minimum extents for the first and second pane, in design units.
    min_a: Cell<f32>,
    min_b: Cell<f32>,
    /// The split's rectangle, in the parent's coordinates.
    area: Cell<Rect>,
    dpi: Cell<u32>,
    /// The divider's thickness in device pixels, set by [`SplitNode::compute`].
    thickness: Cell<i32>,
    /// The divider child window, once bound by [`Core`](crate::app::Core).
    hwnd: Cell<Hwnd>,
    /// The divider's layout bounds and visibility, shared with the tree.
    bounds: Rc<Cell<Rect>>,
    visible: Rc<Cell<bool>>,
    /// The type-erased [`Split::on_moved`] mapper, bound by `Core`. An `Rc` so
    /// the same `Split` can be bound again when a layout is reinstalled.
    mapper: RefCell<Option<Box<dyn Any>>>,
}

impl SplitShared {
    fn new(direction: StackDirection) -> SplitShared {
        SplitShared {
            direction,
            position: Cell::new(None),
            anchor_end: Cell::new(false),
            min_a: Cell::new(0.0),
            min_b: Cell::new(0.0),
            area: Cell::new(Rect::default()),
            dpi: Cell::new(96),
            thickness: Cell::new(0),
            hwnd: Cell::new(Hwnd::NULL),
            bounds: Rc::new(Cell::new(Rect::default())),
            visible: Rc::new(Cell::new(true)),
            mapper: RefCell::new(None),
        }
    }

    fn handle(&self) -> WidgetHandle {
        WidgetHandle::new(
            self.hwnd.get(),
            Rc::clone(&self.bounds),
            Rc::clone(&self.visible),
        )
    }

    fn is_horizontal(&self) -> bool {
        self.direction == StackDirection::Horizontal
    }

    /// Whether [`position`](Split::position_b) is the second pane's extent.
    fn is_end_anchored(&self) -> bool {
        self.anchor_end.get()
    }

    /// `+1` when a movement towards larger coordinates grows the anchored pane,
    /// `-1` when the anchored pane sits at the end (its divider edge moves the
    /// other way).
    fn anchored_sign(&self) -> i32 {
        if self.is_end_anchored() { -1 } else { 1 }
    }

    /// The anchored pane's raw extent in device pixels, defaulting to half.
    fn position_px(&self) -> i32 {
        let dpi = self.dpi.get().max(96);
        let (available, _, _) = self.limits();
        match self.position.get() {
            Some(value) => Dip(value).to_px(dpi).value(),
            None => available / 2,
        }
    }

    /// `(available, min_a, min_b)` in device pixels, from the last laid-out
    /// area, the current DPI and the configured minimums.
    fn limits(&self) -> (i32, i32, i32) {
        let dpi = self.dpi.get().max(96);
        let area = self.area.get();
        let total = if self.is_horizontal() {
            area.width()
        } else {
            area.height()
        };
        let available = (total - self.thickness.get()).max(0);
        let min_a = Dip(self.min_a.get()).to_px(dpi).value();
        let min_b = Dip(self.min_b.get()).to_px(dpi).value();
        (available, min_a, min_b)
    }

    /// Clamps an anchored-pane extent to the panes' minimums and the available
    /// space.
    fn clamp_anchored(&self, px: i32) -> i32 {
        let (available, min_a, min_b) = self.limits();
        if min_a + min_b <= available {
            if self.anchor_end.get() {
                px.clamp(min_b, available - min_a)
            } else {
                px.clamp(min_a, available - min_b)
            }
        } else {
            available / 2
        }
    }

    /// The first pane's extent in device pixels, from the clamped anchored
    /// extent.
    fn pane_a_px(&self) -> i32 {
        let (available, _, _) = self.limits();
        let anchored = self.clamp_anchored(self.position_px());
        if self.anchor_end.get() {
            (available - anchored).max(0)
        } else {
            anchored
        }
    }

    /// Returns a clone of the erased `on_moved` mapper, downcast to this app's
    /// `Msg`. Cloning (rather than taking) keeps a reused `Split` bound when a
    /// layout is reinstalled.
    fn moved_mapper<M: 'static>(&self) -> Option<Rc<dyn Fn(Dip) -> Option<M>>> {
        let erased = self.mapper.borrow();
        erased
            .as_ref()?
            .downcast_ref::<Rc<dyn Fn(Dip) -> Option<M>>>()
            .cloned()
    }
}

/// A split of two layout items with a draggable divider.
///
/// Built by [`split_row!`](crate::split_row) / [`split_col!`](crate::split_col).
pub struct Split {
    shared: Rc<SplitShared>,
    a: Option<LayoutItem>,
    b: Option<LayoutItem>,
}

impl Split {
    /// A split whose panes are side by side.
    pub fn row() -> Split {
        Split::new(StackDirection::Horizontal)
    }

    /// A split whose panes are stacked.
    pub fn column() -> Split {
        Split::new(StackDirection::Vertical)
    }

    fn new(direction: StackDirection) -> Split {
        Split {
            shared: Rc::new(SplitShared::new(direction)),
            a: None,
            b: None,
        }
    }

    /// Sets the first (left/top) pane.
    pub fn a(mut self, item: impl IntoLayoutItem) -> Split {
        self.a = Some(item.into_layout_item());
        self
    }

    /// Sets the second (right/bottom) pane.
    pub fn b(mut self, item: impl IntoLayoutItem) -> Split {
        self.b = Some(item.into_layout_item());
        self
    }

    /// The first pane's initial extent, in design units.
    pub fn position(self, position: Dip) -> Split {
        self.shared.position.set(Some(position.value()));
        self.shared.anchor_end.set(false);
        self
    }

    /// The second pane's initial extent, in design units: the split is anchored
    /// from the end, so a fixed-width right or bottom panel keeps its width as
    /// the window grows. [`Split::on_moved`] reports this second pane's extent.
    pub fn position_b(self, position: Dip) -> Split {
        self.shared.position.set(Some(position.value()));
        self.shared.anchor_end.set(true);
        self
    }

    /// The minimum extents of the first and second pane, in design units.
    pub fn min(self, a: Dip, b: Dip) -> Split {
        self.shared.min_a.set(a.value().max(0.0));
        self.shared.min_b.set(b.value().max(0.0));
        self
    }

    /// Maps a divider move to an app message: the closure receives the anchored
    /// pane's new extent in design units (the first pane's, or the second's
    /// when built with [`Split::position_b`]), and returns `Some(msg)` to raise
    /// it, or `None` to ignore the move (it is still applied).
    pub fn on_moved<M: 'static>(self, f: impl Fn(Dip) -> Option<M> + 'static) -> Split {
        let mapper: Rc<dyn Fn(Dip) -> Option<M>> = Rc::new(f);
        self.shared.mapper.replace(Some(Box::new(mapper)));
        self
    }

    fn node(&self) -> SplitNode {
        SplitNode {
            shared: Rc::clone(&self.shared),
            a: Box::new(self.a.clone().unwrap_or_else(empty_item)),
            b: Box::new(self.b.clone().unwrap_or_else(empty_item)),
        }
    }
}

fn empty_item() -> LayoutItem {
    LayoutItem {
        content: Content::Nested(Box::new(crate::Layout::row())),
        sizing: Sizing::Fill(1),
    }
}

impl IntoLayoutItem for &Split {
    fn into_layout_item(self) -> LayoutItem {
        LayoutItem {
            content: Content::Split(Box::new(self.node())),
            sizing: Sizing::Fill(1),
        }
    }
}

impl IntoLayoutItem for Split {
    fn into_layout_item(self) -> LayoutItem {
        (&self).into_layout_item()
    }
}

/// A bound split node inside a [`Layout`](super::Layout).
#[derive(Clone)]
pub(crate) struct SplitNode {
    pub(crate) shared: Rc<SplitShared>,
    a: Box<LayoutItem>,
    b: Box<LayoutItem>,
}

impl SplitNode {
    /// Whether either pane is visible.
    pub(crate) fn is_visible(&self) -> bool {
        self.a.is_visible() || self.b.is_visible()
    }

    /// The two panes, so the window can bind a split nested inside either one.
    pub(crate) fn panes(&self) -> [&LayoutItem; 2] {
        [&self.a, &self.b]
    }

    /// Shows or hides both panes and the divider. Used when a split is a page
    /// of a [`Tabs`](crate::Tabs) node.
    pub(crate) fn set_tree_visible(&self, visible: bool) {
        self.a.set_tree_visible(visible);
        self.b.set_tree_visible(visible);
        let hwnd = self.shared.hwnd.get();
        if !hwnd.is_null() {
            let kind = if visible {
                sys::window::ShowKind::Normal
            } else {
                sys::window::ShowKind::Hidden
            };
            sys::window::show(hwnd, kind);
        }
    }

    /// Lays the two panes and the divider into `rect`.
    pub(crate) fn compute(&self, rect: Rect, dpi: u32, out: &mut Vec<Placed>) {
        let shared = &self.shared;
        shared.area.set(rect);
        shared.dpi.set(dpi);
        let thickness = Dip(DIVIDER_DIP).to_px(dpi).value();
        shared.thickness.set(thickness);

        match (self.a.is_visible(), self.b.is_visible()) {
            (false, false) => {}
            (true, false) => self.a.compute(rect, dpi, out),
            (false, true) => self.b.compute(rect, dpi, out),
            (true, true) => {
                let horizontal = shared.is_horizontal();
                // The first pane's extent, clamped to the pane minimums; the
                // anchored pane's extent is honoured on whichever end it is.
                let position = shared.pane_a_px();

                let (a_rect, divider_rect, b_rect) = if horizontal {
                    let a_right = rect.left + position;
                    let divider_right = a_right + thickness;
                    (
                        Rect::new(rect.left, rect.top, a_right, rect.bottom),
                        Rect::new(a_right, rect.top, divider_right, rect.bottom),
                        Rect::new(divider_right, rect.top, rect.right, rect.bottom),
                    )
                } else {
                    let a_bottom = rect.top + position;
                    let divider_bottom = a_bottom + thickness;
                    (
                        Rect::new(rect.left, rect.top, rect.right, a_bottom),
                        Rect::new(rect.left, a_bottom, rect.right, divider_bottom),
                        Rect::new(rect.left, divider_bottom, rect.right, rect.bottom),
                    )
                };
                self.a.compute(a_rect, dpi, out);
                self.b.compute(b_rect, dpi, out);
                if shared.hwnd.get().is_alive() {
                    out.push(Placed {
                        handle: shared.handle(),
                        rect: divider_rect,
                    });
                }
            }
        }
    }
}

/// Creates and binds the divider child window for `node`.
///
/// Called by [`Core`](crate::app::Core) when a layout is installed; returns
/// `None` if the divider already exists or the window cannot be created.
pub(crate) fn build_divider<M: 'static>(ui: &Ui<M>, node: &SplitNode) -> Option<Window> {
    let shared = Rc::clone(&node.shared);
    if shared.hwnd.get().is_alive() {
        return None;
    }
    let mapper = shared.moved_mapper::<M>();
    let core = ui.core_weak();
    let emit: Rc<dyn Fn(SplitEvent)> = {
        let shared = Rc::clone(&shared);
        let core = core.clone();
        Rc::new(move |event| {
            let SplitEvent::Moved(position) = event;
            shared.position.set(Some(position.value()));
            if let Some(core) = core.upgrade() {
                let ui = Ui::new(core);
                ui.relayout();
                if let Some(msg) = mapper.as_ref().and_then(|f| f(position)) {
                    ui.emit(msg);
                }
            }
        })
    };

    let dpi = ui.dpi();
    let thickness = Dip(DIVIDER_DIP).to_px(dpi).value();
    let handler = DividerHandler {
        core,
        bounds: Rc::new(Cell::new(Rect::new(0, 0, thickness, thickness))),
        emit,
        widget: Divider::new(Rc::clone(&shared)),
    };
    let class = WindowClass::register("win32ui.split", ui.theme().background).ok()?;
    let window = Window::create(
        class,
        Some(ui.hwnd()),
        WindowStyle::new().child().visible().tab_stop(),
        WindowExStyle::new(),
        Rect::new(0, 0, thickness, thickness),
        "splitter",
        handler,
    )
    .ok()?;

    let hwnd = window.hwnd();
    shared.hwnd.set(hwnd);
    crate::theme::register_themed(
        ui.hwnd(),
        hwnd,
        Rc::new(move |applied| {
            sys::set_class_background(hwnd, applied.background);
            sys::window::invalidate(hwnd);
        }),
    );
    Some(window)
}
