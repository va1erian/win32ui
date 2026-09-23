#![forbid(unsafe_code)]

//! The private plumbing behind [`Custom`](super::custom::Custom): the shared
//! widget state and the child window's handler.
//!
//! `Custom` owns the child `HWND`; this handler is what that window runs. It
//! decodes input messages into [`Input`](super::custom::Input), runs
//! [`CustomWidget::paint`](super::custom::CustomWidget::paint) on `WM_PAINT`,
//! and hands each input to the widget with a fresh
//! [`WidgetCx`](super::custom::WidgetCx).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::app::Ui;
use crate::controls::custom::{CustomWidget, Input, WidgetCx};
use crate::d2d::D2dSurface;
use crate::gdi::Paint;
use crate::geometry::Rect;
use crate::message::{LResult, Message};
use crate::window::{Window, WindowHandler};

/// How a custom widget is drawn. Direct2D is tried on the first paint; if it
/// cannot be created, the widget stays on GDI for good.
pub(super) enum Renderer {
    Untried,
    Direct2d(Box<D2dSurface>),
    Gdi,
}

/// Maps a widget event to an optional app message.
type EventMapper<W, M> = Box<dyn Fn(<W as CustomWidget>::Event) -> Option<M>>;

/// The state shared between [`Custom`](super::custom::Custom) and its handler:
/// the widget itself, the event mapper set by `on_event`, and the `Ui` used to
/// enqueue mapped messages.
pub(super) struct CustomShared<W: CustomWidget, M> {
    pub(super) widget: Rc<RefCell<W>>,
    pub(super) mapper: RefCell<Option<EventMapper<W, M>>>,
    pub(super) ui: Ui<M>,
}

impl<W: CustomWidget, M: 'static> CustomShared<W, M> {
    /// Maps `event` through the `on_event` closure and enqueues the result.
    pub(super) fn emit(&self, event: W::Event) {
        if let Some(mapper) = self.mapper.borrow().as_ref()
            && let Some(msg) = mapper(event)
        {
            self.ui.emit(msg);
        }
    }
}

/// The [`WindowHandler`] for a custom widget's child window.
pub(super) struct CustomHandler<W: CustomWidget, M> {
    pub(super) shared: Rc<CustomShared<W, M>>,
    /// The widget's client bounds (origin at zero), updated on `WM_SIZE`.
    pub(super) bounds: Rc<Cell<Rect>>,
    /// Emits an event by mapping it to the app's `Msg`; built once so painting
    /// and input never allocate.
    pub(super) emit: Rc<dyn Fn(W::Event)>,
    /// The renderer, chosen on first paint (Direct2D when available).
    pub(super) renderer: RefCell<Renderer>,
}

impl<W: CustomWidget, M: 'static> CustomHandler<W, M> {
    /// Draws with Direct2D; `false` means this paint must fall back to GDI.
    fn paint_d2d(&self, window: &Window) -> bool {
        let mut renderer = self.renderer.borrow_mut();
        if matches!(*renderer, Renderer::Untried) {
            *renderer = D2dSurface::new(window.hwnd()).map_or(Renderer::Gdi, |surface| {
                Renderer::Direct2d(Box::new(surface))
            });
        }
        let Renderer::Direct2d(surface) = &*renderer else {
            return false;
        };
        let Ok(mut canvas) = surface.begin_draw() else {
            return false;
        };
        let theme = self.shared.ui.theme();
        let bounds = canvas.bounds();
        self.shared
            .widget
            .borrow()
            .paint_d2d(&mut canvas, bounds, &theme);
        if canvas.end_draw().is_err() {
            *renderer = Renderer::Gdi;
            window.invalidate();
        }
        true
    }
}

impl<W: CustomWidget, M: 'static> WindowHandler for CustomHandler<W, M> {
    fn message(&self, window: &Window, message: Message) -> Option<LResult> {
        match message {
            Message::Paint => {
                if self.paint_d2d(window) {
                    return Some(0);
                }
                if let Some(paint) = Paint::begin(window.hwnd()) {
                    let theme = self.shared.ui.theme();
                    self.shared
                        .widget
                        .borrow()
                        .paint(paint.canvas(), self.bounds.get(), &theme);
                }
                Some(0)
            }
            Message::Size { width, height } => {
                self.bounds.set(Rect::new(0, 0, width, height));
                if let Renderer::Direct2d(surface) = &*self.renderer.borrow() {
                    surface.resize(width, height);
                }
                Some(0)
            }
            Message::DpiChanged { dpi, .. } => {
                if let Renderer::Direct2d(surface) = &*self.renderer.borrow() {
                    surface.set_dpi(dpi);
                }
                None
            }
            _ if D2dSurface::is_erase_background(&message)
                && matches!(*self.renderer.borrow(), Renderer::Direct2d(_)) =>
            {
                Some(1)
            }
            message => {
                let input = Input::from_message(message)?;
                let mut cx = WidgetCx::new(
                    window.hwnd(),
                    Rc::clone(&self.bounds),
                    Rc::clone(&self.emit),
                );
                self.shared.widget.borrow().input(input, &mut cx);
                Some(0)
            }
        }
    }
}
