#![forbid(unsafe_code)]

//! The widget-author-facing types of the custom owner-draw pattern: how a
//! [`CustomWidget`](super::CustomWidget) paints ([`Renderer`]), the typed input
//! it receives ([`Input`]), and the context it is handed while handling input
//! ([`WidgetCx`]).

use std::cell::Cell;
use std::rc::Rc;

use crate::geometry::Rect;
use crate::hwnd::Hwnd;
use crate::message::{Key, Message, Modifiers, MouseButton};
use crate::sys;
use crate::window::CursorShape;

/// How a custom widget paints itself.
///
/// The default ([`Renderer::Gdi`]) draws with
/// [`CustomWidget::paint`](super::CustomWidget::paint) into a GDI
/// [`Canvas`](crate::gdi::Canvas). A widget that needs anti-aliasing opts into
/// [`Renderer::Direct2D`] and draws with
/// [`CustomWidget::paint_d2d`](super::CustomWidget::paint_d2d) instead.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Renderer {
    /// Paint with GDI through [`CustomWidget::paint`](super::CustomWidget::paint).
    #[default]
    Gdi,
    /// Paint with Direct2D through
    /// [`CustomWidget::paint_d2d`](super::CustomWidget::paint_d2d).
    Direct2D,
}

/// An input event delivered to a [`CustomWidget`](super::CustomWidget).
///
/// This is the widget-layer subset of [`Message`] that a custom widget needs:
/// mouse, keyboard, focus and hover. The [`Custom`](super::Custom) handler
/// decodes these from the raw window messages and passes them to
/// [`CustomWidget::input`](super::CustomWidget::input).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Input {
    /// A mouse button went down.
    MouseDown {
        /// Cursor x in client coordinates.
        x: i32,
        /// Cursor y in client coordinates.
        y: i32,
        /// Which button.
        button: MouseButton,
    },
    /// A mouse button was released.
    MouseUp {
        /// Cursor x in client coordinates.
        x: i32,
        /// Cursor y in client coordinates.
        y: i32,
        /// Which button.
        button: MouseButton,
    },
    /// The cursor moved.
    MouseMove {
        /// Cursor x in client coordinates.
        x: i32,
        /// Cursor y in client coordinates.
        y: i32,
    },
    /// A mouse button was double-clicked.
    MouseDoubleClick {
        /// Cursor x in client coordinates.
        x: i32,
        /// Cursor y in client coordinates.
        y: i32,
        /// Which button.
        button: MouseButton,
    },
    /// The wheel was rolled.
    MouseWheel {
        /// Wheel rotation, in multiples of `WHEEL_DELTA`.
        delta: i16,
        /// Whether this is a horizontal wheel.
        horizontal: bool,
        /// Cursor x in client coordinates.
        x: i32,
        /// Cursor y in client coordinates.
        y: i32,
        /// Which modifiers were held.
        modifiers: Modifiers,
    },
    /// The cursor left the widget.
    MouseLeave,
    /// Another window took the mouse capture, ending any drag.
    CaptureChanged,
    /// A key went down.
    KeyDown {
        /// The virtual key.
        key: Key,
        /// Which modifiers were held.
        modifiers: Modifiers,
        /// Auto-repeat count (`1` on the first press).
        repeat: u16,
        /// Whether this came from a system key (an Alt combination).
        system: bool,
    },
    /// A key was released.
    KeyUp {
        /// The virtual key.
        key: Key,
        /// Which modifiers were held.
        modifiers: Modifiers,
        /// Whether this came from a system key.
        system: bool,
    },
    /// A translated character.
    Char(char),
    /// One tick of the animation timer requested with
    /// [`WidgetCx::request_animation`].
    Tick,
    /// A frame was painted. A widget that coalesces its events per frame
    /// flushes them here.
    Frame,
    /// The widget gained the keyboard focus.
    SetFocus,
    /// The widget lost the keyboard focus.
    KillFocus,
}

impl Input {
    /// The subset of [`Message`] that maps to an [`Input`], or `None`.
    pub(crate) fn from_message(message: Message) -> Option<Input> {
        Some(match message {
            Message::MouseDown { x, y, button } => Input::MouseDown { x, y, button },
            Message::MouseUp { x, y, button } => Input::MouseUp { x, y, button },
            Message::MouseMove { x, y } => Input::MouseMove { x, y },
            Message::MouseDoubleClick { x, y, button } => Input::MouseDoubleClick { x, y, button },
            Message::MouseWheel {
                delta,
                horizontal,
                x,
                y,
                modifiers,
            } => Input::MouseWheel {
                delta,
                horizontal,
                x,
                y,
                modifiers,
            },
            Message::MouseLeave => Input::MouseLeave,
            Message::CaptureChanged => Input::CaptureChanged,
            Message::KeyDown {
                key,
                modifiers,
                repeat,
                system,
            } => Input::KeyDown {
                key,
                modifiers,
                repeat,
                system,
            },
            Message::KeyUp {
                key,
                modifiers,
                system,
            } => Input::KeyUp {
                key,
                modifiers,
                system,
            },
            Message::Char(c) => Input::Char(c),
            Message::SetFocus => Input::SetFocus,
            Message::KillFocus => Input::KillFocus,
            _ => return None,
        })
    }
}

/// What [`CustomWidget::key`](super::CustomWidget::key) did with a navigation
/// key: consumed it, or left it to the scroll host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyResult {
    /// The widget handled the key; the scroll host must not also act on it.
    Handled,
    /// The widget did not handle the key, so the scroll host may act on it.
    Ignored,
}

/// The context a [`CustomWidget`](super::CustomWidget) is given while handling
/// [`Input`].
///
/// It maps the widget's events to the app's `Msg` (through the same queue as
/// every other widget), and offers the window operations a widget might need
/// while an input is in progress.
pub struct WidgetCx<E> {
    hwnd: Hwnd,
    bounds: Rc<Cell<Rect>>,
    emit: Rc<dyn Fn(E)>,
    dpi: u32,
    animate: Rc<Cell<bool>>,
}

impl<E> WidgetCx<E> {
    pub(crate) fn new(
        hwnd: Hwnd,
        bounds: Rc<Cell<Rect>>,
        emit: Rc<dyn Fn(E)>,
        dpi: u32,
        animate: Rc<Cell<bool>>,
    ) -> WidgetCx<E> {
        WidgetCx {
            hwnd,
            bounds,
            emit,
            dpi,
            animate,
        }
    }

    /// The widget's dots-per-inch, to convert input coordinates (device
    /// pixels) to device-independent pixels.
    pub fn dpi(&self) -> u32 {
        self.dpi
    }

    /// Starts (`true`) or stops (`false`) a repeating animation timer that
    /// delivers [`Input::Tick`] about every frame. A widget requests it only
    /// while it is animating, so an idle widget costs no timer.
    pub fn request_animation(&self, on: bool) {
        self.animate.set(on);
    }

    /// Maps `event` to the app's `Msg` through the widget's
    /// [`Custom::on_event`](super::Custom::on_event) closure and enqueues it.
    /// Like every widget event, the resulting `Msg` is delivered to
    /// [`App::update`](crate::App::update) after the current one returns —
    /// never re-entered.
    pub fn emit(&self, event: E) {
        (self.emit)(event);
    }

    /// The widget's current client bounds, in device pixels.
    pub fn bounds(&self) -> Rect {
        self.bounds.get()
    }

    /// Schedules a repaint of the widget.
    pub fn invalidate(&self) {
        sys::window::invalidate(self.hwnd);
    }

    /// Schedules a repaint of `rect` only — the widget's client coordinates, in
    /// device pixels. The framework clips the paint to it (GDI via `rcPaint`,
    /// Direct2D via the update region), so a widget that changed one row or one
    /// selection repaints just that. Windows unions successive calls, so the
    /// next paint reports their bounding rectangle.
    pub fn invalidate_rect(&self, rect: Rect) {
        sys::window::invalidate_rect(self.hwnd, rect);
    }

    /// Shows `text` while the pointer rests inside `rect` — a region in the
    /// widget's client coordinates (device pixels). The widget's top-level
    /// window shares one lazily created tooltip; on a dark theme it is
    /// owner-drawn from theme tokens. Calling this again with a new rectangle
    /// or text updates the same region tool.
    pub fn set_tooltip_region(&self, rect: Rect, text: &str) {
        crate::controls::tooltip::set_region_tooltip(self.hwnd, 0, rect, text);
    }

    /// Captures the mouse, so all mouse input goes to the widget until
    /// [`WidgetCx::release_capture`] is called.
    pub fn capture(&self) {
        sys::window_input::set_capture(self.hwnd);
    }

    /// Releases the mouse capture, if the widget holds it.
    pub fn release_capture(&self) {
        sys::window_input::release_capture();
    }

    /// Sets the cursor shown over the widget.
    pub fn cursor(&self, shape: CursorShape) {
        sys::window_input::set_cursor(self.hwnd, shape);
    }

    /// Gives the widget the keyboard focus.
    pub fn focus(&self) {
        sys::window_input::focus(self.hwnd);
    }
}
