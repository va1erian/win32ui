#![forbid(unsafe_code)]

//! The private plumbing behind [`Custom`](super::custom::Custom): the shared
//! widget state and the child window's handler.
//!
//! `Custom` owns the child `HWND`; this handler is what that window runs. It
//! decodes input messages into [`Input`](super::custom::Input), runs
//! [`CustomWidget::paint`](super::custom::CustomWidget::paint) (or
//! [`CustomWidget::paint_d2d`](super::custom::CustomWidget::paint_d2d)) on
//! `WM_PAINT`, drives the optional vertical scroll host, and hands each input
//! to the widget with a fresh [`WidgetCx`](super::custom::WidgetCx).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::app::Ui;
use crate::controls::custom::{
    CustomScroll, CustomWidget, Input, KeyResult, Renderer, RendererState, WidgetCx, is_scroll_key,
};
use crate::d2d::{D2dSurface, pixels_to_dips};
use crate::gdi::{Canvas, Paint};
use crate::geometry::Rect;
use crate::hwnd::Hwnd;
use crate::message::{Key, LResult, Message, Modifiers, TimerId};
use crate::window::{Window, WindowHandler};

/// Maps a widget event to an optional app message.
type EventMapper<W, M> = Box<dyn Fn(<W as CustomWidget>::Event) -> Option<M>>;

/// A callback run when the widget's client area changes size.
type ResizeFn = Rc<dyn Fn(Rect)>;

/// The state shared between [`Custom`](super::custom::Custom) and its handler:
/// the widget itself, the event mapper set by `on_event`, the `Ui` used to
/// enqueue mapped messages, and the optional vertical scroll host.
pub(super) struct CustomShared<W: CustomWidget, M> {
    pub(super) widget: Rc<RefCell<W>>,
    pub(super) mapper: RefCell<Option<EventMapper<W, M>>>,
    pub(super) scroll: RefCell<Option<Rc<CustomScroll<M>>>>,
    /// Runs when the widget's client area changes size; see
    /// [`Custom::on_resize`](super::custom::Custom::on_resize).
    pub(super) resize: RefCell<Option<ResizeFn>>,
    /// The running animation timer, if any: it exists only while the widget
    /// has asked for animation ticks.
    pub(super) timer: Cell<Option<TimerId>>,
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
    pub(super) renderer: Rc<RefCell<RendererState>>,
    /// Whether the widget asked for animation ticks.
    pub(super) animate: Rc<Cell<bool>>,
    /// Whether a `WM_MOUSELEAVE` is armed, so a move re-arms it only after a leave.
    pub(super) tracking_mouse: Cell<bool>,
}

/// The animation timer period: one 60 Hz frame.
const ANIMATION_FRAME_MS: u32 = 16;

impl<W: CustomWidget, M: 'static> CustomHandler<W, M> {
    fn paint(&self, hwnd: Hwnd) {
        let theme = self.shared.ui.theme();
        let widget = self.shared.widget.borrow();
        let bounds = self.bounds.get();

        match widget.renderer() {
            Renderer::Gdi => {
                if let Some(paint) = Paint::begin(hwnd) {
                    widget.paint(paint.canvas(), bounds, &theme);
                }
                return;
            }
            Renderer::Gl => {
                let painted = self.renderer.borrow_mut().paint_gl(
                    hwnd,
                    theme.background,
                    |gl| {
                        widget.paint_gl(gl, bounds, &theme);
                    },
                    |gl| widget.gl_teardown(gl),
                );
                // OpenGL could not draw this frame: fall back to the theme
                // background, exactly as the Direct2D path does.
                if !painted && let Some(paint) = Paint::begin(hwnd) {
                    paint.canvas().fill_rect(bounds, theme.background);
                }
                return;
            }
            Renderer::Direct2D => {}
        }

        let scroll = self.shared.scroll.borrow();
        let offset = scroll.as_ref().map_or(0, |s| s.offset());
        let dpi = self.shared.ui.dpi();
        let viewport_offset = pixels_to_dips(offset, dpi);
        drop(scroll);

        // The dirty rectangle the Direct2D frame clips to; the GDI path reads
        // the same region from `PAINTSTRUCT.rcPaint` in `Paint::begin`.
        let dirty = crate::sys::window::update_rect(hwnd);
        let mut renderer = self.renderer.borrow_mut();
        let painted = renderer.paint(hwnd, dirty, |canvas| {
            canvas.clear(theme.background);
            let viewport = canvas.bounds();
            // Always reset the translation: the render target keeps its
            // transform between frames, so skipping it at offset zero would
            // leave the previous scroll transform applied.
            canvas.set_translation(0.0, -viewport_offset);
            widget.paint_d2d(canvas, viewport, &theme);
        });
        drop(renderer);

        // Direct2D could not draw this frame: fall back to the theme background.
        if !painted && let Some(paint) = Paint::begin(hwnd) {
            paint.canvas().fill_rect(bounds, theme.background);
        }
    }

    /// Paints the widget into the device context `dc` with GDI, whatever its
    /// [`renderer`](CustomWidget::renderer): the buffered DC of an opaque
    /// top-bar slot is not a window surface Direct2D or OpenGL could bind.
    fn paint_into(&self, dc: usize) {
        let theme = self.shared.ui.theme();
        let widget = self.shared.widget.borrow();
        widget.paint(&Canvas::from_raw_dc(dc), self.bounds.get(), &theme);
    }

    /// Hands `input` to the widget with a fresh context, then starts or stops
    /// the animation timer to match what the widget asked for.
    fn dispatch(&self, window: &Window, input: Input) {
        let mut cx = self.make_cx(window);
        self.shared.widget.borrow().input(input, &mut cx);
        self.sync_timer(window);
    }

    /// Gives the widget first refusal on a navigation key, then lets the scroll
    /// host act. Returns `true` when the key is fully handled: the widget
    /// claimed it ([`KeyResult::Handled`]) or the host scrolled. A widget
    /// without a scroll host always returns `false`, so the key falls through
    /// to [`CustomWidget::input`] as before.
    fn dispatch_navigation(&self, window: &Window, key: Key, modifiers: Modifiers) -> bool {
        if !is_scroll_key(key) {
            return false;
        }
        let Some(scroll) = self.shared.scroll.borrow().as_ref().cloned() else {
            return false;
        };
        let mut cx = self.make_cx(window);
        let result = self.shared.widget.borrow().key(key, modifiers, &mut cx);
        self.sync_timer(window);
        result == KeyResult::Handled || scroll.key(key)
    }

    /// A fresh context for one `paint`/`input`/`key` call.
    fn make_cx(&self, window: &Window) -> WidgetCx<W::Event> {
        WidgetCx::new(
            window.hwnd(),
            Rc::clone(&self.bounds),
            Rc::clone(&self.emit),
            self.shared.ui.dpi(),
            Rc::clone(&self.animate),
        )
    }

    fn sync_timer(&self, window: &Window) {
        match (self.animate.get(), self.shared.timer.get()) {
            (true, None) => self
                .shared
                .timer
                .set(window.set_timer(ANIMATION_FRAME_MS).ok()),
            (false, Some(id)) => {
                window.kill_timer(id);
                self.shared.timer.set(None);
            }
            _ => {}
        }
    }
}

impl<W: CustomWidget, M: 'static> WindowHandler for CustomHandler<W, M> {
    fn message(&self, window: &Window, message: Message) -> Option<LResult> {
        // Every widget paints its whole client area (the GDI path through a
        // back buffer), so the default erase to the class brush would only show
        // as a flash before each repaint.
        if D2dSurface::is_erase_background(&message) {
            return Some(1);
        }
        match message {
            Message::Paint => {
                self.paint(window.hwnd());
                self.dispatch(window, Input::Frame);
                Some(0)
            }
            // A `WM_PAINT` with a device context in `wparam`: the opaque top-bar
            // subclass asking the widget to draw into its buffered DC, so a
            // widget in a native slot is not left blank over the material.
            Message::Other { code, wparam, .. }
                if crate::sys::window_input::is_paint(code) && wparam != 0 =>
            {
                self.paint_into(wparam);
                Some(0)
            }
            Message::Timer { id } if Some(id) == self.shared.timer.get() => {
                self.dispatch(window, Input::Tick);
                Some(0)
            }
            Message::Other { code, .. }
                if crate::sys::window_input::is_get_dlg_code(code)
                    && self.shared.widget.borrow().wants_arrow_keys() =>
            {
                Some(crate::sys::window_input::DLGC_WANTARROWS)
            }
            Message::Size { width, height } => {
                let bounds = Rect::new(0, 0, width, height);
                self.bounds.set(bounds);
                if let Some(scroll) = self.shared.scroll.borrow().as_ref() {
                    scroll.on_size();
                }
                self.renderer.borrow().resize(width, height);
                // Drop the borrow before the callback: it may move windows (a
                // composite widget resizing its own content extent), which
                // re-enters this handler.
                let resize = self.shared.resize.borrow().as_ref().cloned();
                if let Some(resize) = resize {
                    resize(bounds);
                }
                Some(0)
            }
            Message::MouseWheel {
                delta, horizontal, ..
            } if !horizontal && self.shared.scroll.borrow().is_some() => {
                if let Some(scroll) = self.shared.scroll.borrow().as_ref() {
                    scroll.wheel(delta);
                }
                Some(0)
            }
            Message::Other { code, wparam, .. } if crate::sys::scroll::is_vscroll(code) => {
                if let Some(scroll) = self.shared.scroll.borrow().as_ref()
                    && let Some(request) = crate::sys::scroll::decode_vscroll(wparam)
                {
                    scroll.scroll_request(request);
                }
                Some(0)
            }
            message => {
                if let Message::KeyDown { key, modifiers, .. } = &message
                    && self.dispatch_navigation(window, *key, *modifiers)
                {
                    return Some(0);
                }
                match message {
                    Message::MouseMove { .. } if !self.tracking_mouse.replace(true) => {
                        let _ = window.track_mouse_leave();
                    }
                    Message::MouseLeave => self.tracking_mouse.set(false),
                    _ => {}
                }
                let input = Input::from_message(message)?;
                self.dispatch(window, input);
                Some(0)
            }
        }
    }
}
