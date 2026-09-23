#![forbid(unsafe_code)]

//! Custom owner-drawn widgets: the single owner-draw pattern every hand-rolled
//! child window in the crate uses.
//!
//! A [`CustomWidget`] is the application's view of a child window that paints
//! itself from semantic theme tokens and maps its input to typed [`Input`]
//! values. [`Custom`] owns the child `HWND` (and its window class), exposes the
//! widget through [`AsControl`]/[`Themed`], and maps the widget's [`CustomWidget::Event`]s
//! to the app's `Msg` through the same per-window queue as every other widget,
//! so [`App::update`](crate::App::update) is never re-entered.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::app::Ui;
use crate::controls::control::{AsControl, Control};
use crate::controls::custom_inner::{CustomHandler, CustomShared, Renderer};
use crate::d2d::{D2dCanvas, RectF};
use crate::error::Result;
use crate::gdi::Canvas;
use crate::geometry::{Rect, Size};
use crate::hwnd::Hwnd;
use crate::message::{Key, Message, Modifiers, MouseButton};
use crate::sys;
use crate::theme::{Theme, Themed};
use crate::window::{CursorShape, Window, WindowClass, WindowExStyle, WindowStyle};

/// An input event delivered to a [`CustomWidget`].
///
/// This is the widget-layer subset of [`Message`] that a custom widget needs:
/// mouse, keyboard, focus and hover. The [`Custom`] handler decodes these from
/// the raw window messages and passes them to [`CustomWidget::input`].
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

/// An application-defined owner-drawn widget.
///
/// Implement this, then wrap the value in a [`Custom`] to host it in a child
/// window. `paint` draws into the double-buffered [`Canvas`]; `input` receives
/// typed [`Input`] and can raise [`CustomWidget::Event`]s through the [`WidgetCx`].
/// The widget is shared (`&self`), so any state that changes during `paint` or
/// `input` must live in `Cell`/`RefCell` fields.
pub trait CustomWidget: 'static {
    /// The events the widget raises through [`WidgetCx::emit`].
    type Event: 'static;

    /// Paints the widget's whole client area (`bounds`, in device pixels, is
    /// the widget's current size at the origin). Paint only from `theme`'s
    /// semantic tokens so live light/dark switching just works.
    fn paint(&self, canvas: &Canvas, bounds: Rect, theme: &Theme);

    /// Paints the widget with Direct2D instead of GDI, when it can. `bounds`
    /// is the client area in device-independent pixels. The default does
    /// nothing, so the widget stays on the GDI [`paint`](CustomWidget::paint).
    fn paint_d2d(&self, _canvas: &mut D2dCanvas, _bounds: RectF, _theme: &Theme) {}

    /// Handles one input event. The default ignores everything.
    fn input(&self, _input: Input, _cx: &mut WidgetCx<Self::Event>) {}

    /// The widget's natural size in device pixels, if it has one. [`Custom`]
    /// uses this for its initial bounds, so a layout that keeps a widget's
    /// natural size picks it up.
    fn preferred_size(&self, _dpi: u32) -> Option<Size> {
        None
    }
}

/// The context a [`CustomWidget`] is given while handling [`Input`].
///
/// It maps the widget's events to the app's `Msg` (through the same queue as
/// every other widget), and offers the window operations a widget might need
/// while an input is in progress.
pub struct WidgetCx<E> {
    hwnd: Hwnd,
    bounds: Rc<Cell<Rect>>,
    emit: Rc<dyn Fn(E)>,
}

impl<E> WidgetCx<E> {
    pub(crate) fn new(hwnd: Hwnd, bounds: Rc<Cell<Rect>>, emit: Rc<dyn Fn(E)>) -> WidgetCx<E> {
        WidgetCx { hwnd, bounds, emit }
    }

    /// Maps `event` to the app's `Msg` through the widget's [`Custom::on_event`]
    /// closure and enqueues it. Like every widget event, the resulting `Msg` is
    /// delivered to [`App::update`](crate::App::update) after the current one
    /// returns — never re-entered.
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

/// A custom owner-drawn widget hosted in its own child window.
///
/// `Custom<W, M>` owns the child `HWND` (and its window class), gives the app a
/// shared [`Custom::widget`] handle to mutate `W` between paints, and maps the
/// widget's events to `M` through [`Custom::on_event`].
pub struct Custom<W: CustomWidget, M> {
    window: Window,
    control: Control,
    shared: Rc<CustomShared<W, M>>,
}

impl<W: CustomWidget, M: 'static> Custom<W, M> {
    /// Creates the widget as a child of the window behind `ui`, adopting `ui`'s
    /// theme. The child's initial size comes from [`CustomWidget::preferred_size`],
    /// or zero when the widget reports none (position it with
    /// [`ControlExt::set_bounds`](crate::ControlExt::set_bounds) or a layout).
    pub fn new(ui: &mut Ui<M>, widget: W) -> Result<Custom<W, M>> {
        let dpi = ui.dpi();
        let bounds = widget
            .preferred_size(dpi)
            .map(Rect::from_size)
            .unwrap_or_default();
        let background = ui.theme().background;

        let shared = Rc::new(CustomShared {
            widget: Rc::new(RefCell::new(widget)),
            mapper: RefCell::new(None),
            ui: ui.clone(),
        });
        let client_bounds = Rc::new(Cell::new(bounds));
        let emit: Rc<dyn Fn(W::Event)> = {
            let shared = Rc::clone(&shared);
            Rc::new(move |event| shared.emit(event))
        };
        let handler = CustomHandler {
            shared: Rc::clone(&shared),
            bounds: Rc::clone(&client_bounds),
            emit,
            renderer: RefCell::new(Renderer::Untried),
        };

        let class = WindowClass::register("win32ui.custom", background)?;
        let window = Window::create(
            class,
            Some(ui.hwnd()),
            WindowStyle::new().child().visible(),
            WindowExStyle::new(),
            bounds,
            "",
            handler,
        )?;
        let control = Control::borrowed(window.hwnd(), bounds);

        {
            let weak = Rc::downgrade(&shared);
            let hwnd = window.hwnd();
            let parent = ui.hwnd();
            crate::theme::register_themed(
                parent,
                hwnd,
                Rc::new(move |applied| {
                    if let Some(_shared) = weak.upgrade() {
                        sys::set_class_background(hwnd, applied.background);
                        sys::window::invalidate(hwnd);
                    }
                }),
            );
        }

        Ok(Custom {
            window,
            control,
            shared,
        })
    }

    /// Maps the widget's events to an app message: the closure returns
    /// `Some(msg)` to raise it, or `None` to ignore the event.
    pub fn on_event(self, f: impl Fn(W::Event) -> Option<M> + 'static) -> Custom<W, M> {
        self.shared.mapper.replace(Some(Box::new(f)));
        self
    }

    /// A shared handle to the widget, for app-side mutation between paints.
    ///
    /// `paint` and `input` take `&self`, so the widget's own mutable state lives
    /// in `Cell`/`RefCell` fields; the app mutates it through this handle.
    pub fn widget(&self) -> Rc<RefCell<W>> {
        Rc::clone(&self.shared.widget)
    }

    /// Schedules a repaint of the widget.
    pub fn invalidate(&self) {
        self.window.invalidate();
    }

    /// The widget's rectangle in screen coordinates.
    pub fn window_rect(&self) -> Rect {
        self.window.window_rect()
    }
}

impl<W: CustomWidget, M> AsControl for Custom<W, M> {
    fn control(&self) -> &Control {
        &self.control
    }
}

impl<W: CustomWidget, M> Themed for Custom<W, M> {
    fn apply_theme(&self, theme: &Theme) {
        sys::set_class_background(self.control.hwnd(), theme.background);
        self.window.invalidate();
    }
}

impl<W: CustomWidget, M> Drop for Custom<W, M> {
    fn drop(&mut self) {
        crate::theme::unregister_themed(self.control.hwnd());
    }
}
