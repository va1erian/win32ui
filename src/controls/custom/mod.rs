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

mod d2d;
mod scroll;
mod widget;

pub(crate) use d2d::RendererState;
pub(crate) use scroll::{CustomScroll, WHEEL_NOTCH_DIP};
pub use widget::{Input, Renderer, WidgetCx};

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::app::Ui;
use crate::controls::control::{AsControl, Control};
use crate::controls::custom_inner::{CustomHandler, CustomShared};
use crate::d2d::{D2dCanvas, RectF};
use crate::error::Result;
use crate::gdi::Canvas;
use crate::geometry::{Rect, Size};
use crate::sys;
use crate::theme::{Theme, Themed};
use crate::units::Dip;
use crate::window::{Window, WindowClass, WindowExStyle, WindowStyle};

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

    /// Which renderer paints the widget. The default is GDI; opt into
    /// Direct2D and implement [`CustomWidget::paint_d2d`] for anti-aliased
    /// shapes and alpha.
    fn renderer(&self) -> Renderer {
        Renderer::Gdi
    }

    /// Paints the widget with Direct2D. `bounds` (device-independent pixels)
    /// is the widget's visible viewport at the origin; when the widget is
    /// hosted with [`Custom::with_vscroll`], the canvas is already translated
    /// by the scroll offset, so draw the document in its own coordinates.
    fn paint_d2d(&self, _canvas: &mut D2dCanvas<'_>, _bounds: RectF, _theme: &Theme) {}

    /// Handles one input event. The default ignores everything.
    fn input(&self, _input: Input, _cx: &mut WidgetCx<Self::Event>) {}

    /// The widget's natural size in device pixels, if it has one. [`Custom`]
    /// uses this for its initial bounds, so a layout that keeps a widget's
    /// natural size picks it up.
    fn preferred_size(&self, _dpi: u32) -> Option<Size> {
        None
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
            scroll: RefCell::new(None),
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
            renderer: RefCell::new(RendererState::Untried),
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
                    if let Some(shared) = weak.upgrade() {
                        sys::set_class_background(hwnd, applied.background);
                        if shared.scroll.borrow().is_some() {
                            sys::apply_native_theme(
                                hwnd,
                                sys::NativeControlKind::Scrollable,
                                applied.is_dark,
                            );
                        }
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

    /// Gives the widget a native vertical scrollbar and standard scrolling
    /// behaviour: thumb tracking, line/page/home/end/arrow/PageUp/PageDown
    /// keys, and the wheel. The Direct2D canvas is pre-translated by the
    /// offset, so a `paint_d2d` widget draws its document in its own
    /// coordinates.
    pub fn with_vscroll(self) -> Custom<W, M> {
        let hwnd = self.control.hwnd();
        let dpi = self.shared.ui.dpi();
        let notch = crate::units::dip(WHEEL_NOTCH_DIP).to_px(dpi).value();
        let scroll = Rc::new(CustomScroll::new(hwnd, notch, self.shared.ui.clone()));
        sys::scroll::enable_vertical(hwnd);
        sys::window::set_tab_stop(hwnd, true);
        sys::apply_native_theme(
            hwnd,
            sys::NativeControlKind::Scrollable,
            self.shared.ui.theme().is_dark,
        );
        self.shared.scroll.replace(Some(Rc::clone(&scroll)));
        self
    }

    /// Maps the scroll offset to an app message whenever the widget scrolls:
    /// the closure receives the new offset in design units and returns
    /// `Some(msg)` to raise it, or `None` to ignore it. Only meaningful with
    /// [`Custom::with_vscroll`].
    pub fn on_scroll(self, f: impl Fn(Dip) -> Option<M> + 'static) -> Custom<W, M> {
        if let Some(scroll) = self.shared.scroll.borrow().as_ref() {
            scroll.set_mapper(f);
        }
        self
    }

    /// Sets the scrollable content height in design units. Only meaningful with
    /// [`Custom::with_vscroll`].
    pub fn set_content_height(&self, height: Dip) {
        if let Some(scroll) = self.shared.scroll.borrow().as_ref() {
            scroll.set_content_height(height, self.shared.ui.dpi());
        }
    }

    /// The current scroll offset in design units, clamped to the content.
    pub fn scroll_offset(&self) -> Dip {
        self.shared
            .scroll
            .borrow()
            .as_ref()
            .map_or(crate::units::dip(0.0), |scroll| {
                scroll.offset_dip(self.shared.ui.dpi())
            })
    }

    /// Scrolls to `offset` design units from the top, clamped to the content.
    pub fn scroll_to(&self, offset: Dip) {
        if let Some(scroll) = self.shared.scroll.borrow().as_ref() {
            scroll.scroll_to(offset, self.shared.ui.dpi());
        }
    }

    /// Scrolls the minimum amount so that `rect` — a rectangle in design units
    /// (device-independent pixels) — is fully visible in the viewport.
    pub fn scroll_into_view(&self, rect: Rect) {
        if let Some(scroll) = self.shared.scroll.borrow().as_ref() {
            scroll.scroll_into_view(rect, self.shared.ui.dpi());
        }
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
        if self.shared.scroll.borrow().is_some() {
            sys::apply_native_theme(
                self.control.hwnd(),
                sys::NativeControlKind::Scrollable,
                theme.is_dark,
            );
        }
        self.window.invalidate();
    }
}

impl<W: CustomWidget, M> Drop for Custom<W, M> {
    fn drop(&mut self) {
        crate::theme::unregister_themed(self.control.hwnd());
    }
}
