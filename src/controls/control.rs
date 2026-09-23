#![forbid(unsafe_code)]

//! The widget-layer core: [`Control`] owns a widget's child `HWND`, [`AsControl`]
//! exposes it, and the blanket [`ControlExt`] gives every widget shared behaviour.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::gdi::Font;
use crate::geometry::Rect;
use crate::hwnd::Hwnd;
use crate::sys;

/// The core every widget holds: its child `HWND` (destroyed on drop), an
/// optional custom font, and its last known bounds.
///
/// A widget built on a custom-drawn [`Window`](crate::Window) (toolbar, status
/// bar) owns its handle through that window; in that case `Control` borrows the
/// handle so `AsControl` still exposes it uniformly without a second destroy.
///
/// The bounds and visibility are shared (`Rc`) with any layout tree the widget
/// is placed in, so moving or hiding a widget through the layout keeps this
/// cached state in sync.
pub struct Control {
    hwnd: Hwnd,
    owned: bool,
    bounds: Rc<Cell<Rect>>,
    visible: Rc<Cell<bool>>,
    font: RefCell<Option<Font>>,
}

impl Control {
    /// Wraps a freshly created child handle and takes ownership of it.
    pub(crate) fn own(hwnd: Hwnd, bounds: Rect) -> Control {
        Control {
            hwnd,
            owned: true,
            bounds: Rc::new(Cell::new(bounds)),
            visible: Rc::new(Cell::new(true)),
            font: RefCell::new(None),
        }
    }

    /// Wraps a handle owned elsewhere (e.g. by a `Window`) without destroying it.
    pub(crate) fn borrowed(hwnd: Hwnd, bounds: Rect) -> Control {
        Control {
            hwnd,
            owned: false,
            bounds: Rc::new(Cell::new(bounds)),
            visible: Rc::new(Cell::new(true)),
            font: RefCell::new(None),
        }
    }

    /// The control's handle.
    pub fn hwnd(&self) -> Hwnd {
        self.hwnd
    }

    /// A shared handle to the cached bounds, used by the layout tree.
    pub(crate) fn bounds_handle(&self) -> Rc<Cell<Rect>> {
        Rc::clone(&self.bounds)
    }

    /// A shared handle to the visibility flag, used by the layout tree.
    pub(crate) fn visible_handle(&self) -> Rc<Cell<bool>> {
        Rc::clone(&self.visible)
    }
}

impl Drop for Control {
    fn drop(&mut self) {
        if self.owned {
            sys::window::destroy(self.hwnd);
        }
    }
}

/// Exposes a widget's [`Control`]. Every widget implements this; shared
/// behaviour then comes from the blanket [`ControlExt`], never a base class.
pub trait AsControl {
    /// The widget's underlying [`Control`].
    fn control(&self) -> &Control;
}

/// Shared behaviour every widget gets through its [`AsControl`] impl.
///
/// There is deliberately no `Deref` to a base type and no downcasting; a
/// widget's extra capabilities live in traits like [`HasText`].
pub trait ControlExt: AsControl {
    /// Enables or disables the widget (disabled widgets are greyed out).
    fn set_enabled(&self, enabled: bool) {
        sys::window::enable_window(self.control().hwnd, enabled);
    }

    /// Shows or hides the widget.
    fn set_visible(&self, visible: bool) {
        self.control().visible.set(visible);
        let kind = if visible {
            sys::window::ShowKind::Normal
        } else {
            sys::window::ShowKind::Hidden
        };
        sys::window::show(self.control().hwnd, kind);
    }

    /// Whether the widget is currently shown. A hidden widget takes no space in
    /// a layout tree.
    fn is_visible(&self) -> bool {
        self.control().visible.get()
    }

    /// Gives the widget keyboard focus.
    fn focus(&self) {
        sys::window::set_focus(self.control().hwnd);
    }

    /// The widget's bounds, in device pixels.
    fn bounds(&self) -> Rect {
        self.control().bounds.get()
    }

    /// Moves/resizes the widget.
    fn set_bounds(&self, bounds: Rect) {
        self.control().bounds.set(bounds);
        sys::window::move_window(self.control().hwnd, bounds);
    }

    /// The widget's handle.
    fn hwnd(&self) -> Hwnd {
        self.control().hwnd
    }

    /// Replaces the widget's font.
    fn set_font(&self, font: Font) {
        sys::control::set_control_font(self.control().hwnd, font.raw());
        self.control().font.replace(Some(font));
    }
}

impl<T: AsControl + ?Sized> ControlExt for T {}

/// A widget that carries a single text (labels, buttons, edit boxes).
pub trait HasText {
    /// The widget's current text.
    fn text(&self) -> String;

    /// Replaces the widget's text.
    fn set_text(&self, text: &str);
}
