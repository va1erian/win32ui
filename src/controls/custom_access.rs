#![forbid(unsafe_code)]

//! Bridges a [`Custom`](super::custom::Custom) widget to UI Automation: the
//! window's accessibility [`Source`] snapshots the widget's
//! [`CustomWidget::accessibility`] tree and forwards client actions to
//! [`CustomWidget::accessibility_action`] with a normal [`WidgetCx`], so the
//! events an action raises take the same route as a mouse click's.

use std::cell::Cell;
use std::rc::{Rc, Weak};

use crate::accessibility::registry::Source;
use crate::accessibility::{AccessCx, Action, Node};
use crate::controls::custom::{CustomWidget, WidgetCx};
use crate::controls::custom_inner::CustomShared;
use crate::geometry::Rect;
use crate::hwnd::Hwnd;
use crate::sys;

/// The accessibility source of one custom widget. It holds the widget weakly:
/// the window (and this source with it) may outlive the `Custom` handle.
pub(super) struct CustomAccess<W: CustomWidget, M> {
    pub(super) shared: Weak<CustomShared<W, M>>,
    pub(super) hwnd: Hwnd,
    pub(super) bounds: Rc<Cell<Rect>>,
    pub(super) emit: Rc<dyn Fn(W::Event)>,
    pub(super) animate: Rc<Cell<bool>>,
}

impl<W: CustomWidget, M: 'static> Source for CustomAccess<W, M> {
    fn snapshot(&self) -> Option<Node> {
        let shared = self.shared.upgrade()?;
        // `try_borrow`: a client call that lands while the app holds the
        // widget mutably reads as "unavailable" instead of panicking.
        let widget = shared.widget.try_borrow().ok()?;
        widget.accessibility(&AccessCx::new(
            self.bounds.get(),
            sys::dpi::window_dpi(self.hwnd),
        ))
    }

    fn perform(&self, path: &[usize], action: Action) -> bool {
        let Some(shared) = self.shared.upgrade() else {
            return false;
        };
        let Ok(widget) = shared.widget.try_borrow() else {
            return false;
        };
        let mut cx = WidgetCx::new(
            self.hwnd,
            Rc::clone(&self.bounds),
            Rc::clone(&self.emit),
            sys::dpi::window_dpi(self.hwnd),
            Rc::clone(&self.animate),
        );
        widget.accessibility_action(path, action, &mut cx)
    }
}
