#![forbid(unsafe_code)]

//! A scroll container: a native vertical scrollbar over content taller than the
//! viewport.
//!
//! [`ScrollView`] owns a viewport child window with a standard vertical
//! scrollbar (`WS_VSCROLL` + `SCROLLINFO`). Give it a content control with
//! [`ScrollView::set_content`]; the content is re-parented into the viewport and
//! moved as the bar or the wheel scrolls it. [`ScrollView::scroll_to`] sets the
//! offset from code. The bar is themed dark through the same documented
//! `SetWindowTheme` path as the bundled list/tree controls.

use std::cell::Cell;
use std::rc::Rc;

use crate::app::Ui;
use crate::controls::control::{AsControl, Control, ControlExt};
use crate::controls::style;
use crate::error::Result;
use crate::geometry::Rect;
use crate::hwnd::Hwnd;
use crate::message::{LResult, Message};
use crate::sys;
use crate::theme::{Theme, Themed};
use crate::units::{Dip, Px};
use crate::window::{Window, WindowClass, WindowExStyle, WindowHandler, WindowStyle};

/// How far one wheel notch scrolls, in design units.
const WHEEL_NOTCH_DIP: f32 = 48.0;

/// The state the viewport and the [`ScrollView`] handle share.
pub(crate) struct ScrollShared {
    viewport: Cell<Hwnd>,
    content: Cell<Hwnd>,
    content_height: Cell<i32>,
    page: Cell<i32>,
    offset: Cell<i32>,
    notch: Cell<i32>,
    wheel_accum: Cell<i32>,
    /// The bounds last applied to the content, so a redundant `on_size` (the
    /// content height recomputed to the same value) does not re-issue a
    /// `MoveWindow` and repaint the whole content.
    last_bounds: Cell<Rect>,
    /// The viewport height `last_bounds` was computed against. The content's
    /// bounds (position within its own, always-full-document coordinate
    /// space) do not depend on the viewport's height at all, so a viewport
    /// resize that only changes height — e.g. the Albums grid shrinking to
    /// make room for the track list — leaves `bounds` unchanged and skips the
    /// `MoveWindow` above. But the *visible*, clipped portion of the content
    /// did change, and nothing else repaints the area the viewport gave up:
    /// the OS does not retroactively erase a child's already-composited
    /// pixels just because its ancestor's client rect shrank. Tracking the
    /// page size separately lets `move_content` still force that repaint.
    last_page: Cell<i32>,
}

impl ScrollShared {
    fn new(notch: i32) -> ScrollShared {
        ScrollShared {
            viewport: Cell::new(Hwnd::NULL),
            content: Cell::new(Hwnd::NULL),
            content_height: Cell::new(0),
            page: Cell::new(0),
            offset: Cell::new(0),
            notch: Cell::new(notch.max(1)),
            wheel_accum: Cell::new(0),
            last_bounds: Cell::new(Rect::default()),
            last_page: Cell::new(0),
        }
    }

    fn max_offset(&self) -> i32 {
        (self.content_height.get() - self.page.get()).max(0)
    }

    fn clamped(&self) -> i32 {
        self.offset.get().clamp(0, self.max_offset())
    }

    /// Sets the content height in device pixels and resyncs the bar/content.
    pub(crate) fn set_content_height_px(&self, height: i32) {
        self.content_height.set(height.max(0));
        self.on_size();
    }

    /// Recomputes the page size from the viewport and repositions the content.
    fn on_size(&self) {
        let viewport = self.viewport.get();
        if !viewport.is_alive() {
            return;
        }
        self.page.set(sys::window::client_rect(viewport).height());
        self.offset.set(self.clamped());
        self.push_info();
        self.move_content();
    }

    /// Writes the range/page/position to the native scrollbar.
    fn push_info(&self) {
        let viewport = self.viewport.get();
        if viewport.is_alive() {
            sys::scroll::set_vertical_info(
                viewport,
                self.content_height.get(),
                self.page.get(),
                self.clamped(),
            );
        }
    }

    /// Moves the content to the current offset, clipping to the viewport.
    fn move_content(&self) {
        let content = self.content.get();
        let viewport = self.viewport.get();
        if !content.is_alive() || !viewport.is_alive() {
            return;
        }
        let width = sys::window::client_rect(viewport).width();
        let bounds = Rect::new(0, -self.offset.get(), width, self.content_height.get());
        let page = self.page.get();
        // A recomputed content height that lands on the same size (the
        // `GridView` resize callback recomputes it from the new width) must not
        // re-issue `MoveWindow`: that would send another `WM_SIZE` and loop.
        if self.last_bounds.get() == bounds {
            // `bounds` is in the content's own coordinate space and never
            // depends on the viewport's height, so a viewport resize that
            // only changes height (a sibling being shown/hidden next to it,
            // say) lands here even though the visible, clipped portion of
            // the content changed. Nothing else repaints the area the
            // viewport gave up or reclaimed — a shrunk/grown parent does not
            // retroactively invalidate a child's already-composited pixels —
            // so force that repaint directly instead of skipping entirely.
            if self.last_page.get() != page {
                self.last_page.set(page);
                sys::window::invalidate(content);
            }
            return;
        }
        self.last_bounds.set(bounds);
        self.last_page.set(page);
        sys::window::move_window(content, bounds);
    }

    /// Scrolls to `offset` device pixels, clamped to the valid range.
    pub(crate) fn scroll_to_px(&self, offset: i32) {
        self.offset.set(offset.clamp(0, self.max_offset()));
        let viewport = self.viewport.get();
        if viewport.is_alive() {
            sys::scroll::set_vertical_pos(viewport, self.offset.get());
        }
        self.move_content();
    }

    /// Scrolls by `delta` device pixels (positive scrolls down).
    pub(crate) fn scroll_by_px(&self, delta: i32) {
        self.scroll_to_px(self.offset.get() + delta);
    }

    /// Accumulates a raw wheel delta and scrolls whole notches.
    pub(crate) fn wheel(&self, delta: i16) {
        let unit = sys::scroll::wheel_delta().max(1);
        let accumulated = self.wheel_accum.get() + i32::from(delta);
        let notches = accumulated / unit;
        self.wheel_accum.set(accumulated % unit);
        if notches != 0 {
            self.scroll_by_px(-notches * self.notch.get());
        }
    }
}

/// A vertical scroll container with a native, themed scrollbar.
pub struct ScrollView {
    shared: Rc<ScrollShared>,
    wheel: std::cell::RefCell<Option<sys::scroll::WheelForwarder>>,
    /// Owns the viewport (and its class registration); dropping it destroys the
    /// window, which destroys the re-parented content with it.
    _window: Window,
    control: Control,
}

impl ScrollView {
    /// Creates an empty viewport as a child of the window behind `ui`,
    /// adopting `ui`'s theme.
    pub fn new<M: 'static>(ui: &mut Ui<M>) -> Result<ScrollView> {
        let dpi = ui.dpi();
        let notch = Dip(WHEEL_NOTCH_DIP).to_px(dpi).value();
        let shared = Rc::new(ScrollShared::new(notch));
        let background = ui.theme().background;

        let class = WindowClass::register("win32ui.scroll", background)?;
        let window = Window::create(
            class,
            Some(ui.hwnd()),
            WindowStyle::new()
                .child()
                .visible()
                .tab_stop()
                .with(style::WS_VSCROLL),
            WindowExStyle::new(),
            Rect::default(),
            "scroll",
            ScrollHandler {
                shared: Rc::clone(&shared),
            },
        )?;
        let hwnd = window.hwnd();
        shared.viewport.set(hwnd);
        sys::apply_native_theme(hwnd, sys::NativeControlKind::Scrollable, ui.theme().is_dark);

        let control = Control::borrowed(hwnd, Rect::default());
        let scroll = ScrollView {
            shared,
            wheel: std::cell::RefCell::new(None),
            _window: window,
            control,
        };
        crate::theme::register_themed(
            ui.hwnd(),
            hwnd,
            Rc::new(move |applied| {
                sys::apply_native_theme(hwnd, sys::NativeControlKind::Scrollable, applied.is_dark);
                sys::set_class_background(hwnd, applied.background);
                sys::window::invalidate(hwnd);
            }),
        );
        Ok(scroll)
    }

    /// Re-parents `content` into the viewport and scrolls it. The content's
    /// current height becomes the scrollable extent unless
    /// [`ScrollView::set_content_height`] overrides it.
    pub fn set_content<C: AsControl>(&self, content: &C) {
        let viewport = self.shared.viewport.get();
        if !viewport.is_alive() {
            return;
        }
        let hwnd = content.hwnd();
        sys::window::set_parent(hwnd, viewport);
        self.shared.content.set(hwnd);
        self.shared.last_bounds.set(Rect::default());
        self.shared.last_page.set(-1);
        let height = content.bounds().height();
        if height > 0 {
            self.shared.content_height.set(height);
        }

        let shared = Rc::clone(&self.shared);
        let forwarder =
            sys::scroll::WheelForwarder::install(hwnd, Box::new(move |delta| shared.wheel(delta)));
        self.wheel.replace(forwarder);
        self.shared.on_size();
    }

    /// Sets the scrollable content height explicitly (e.g. for content that is
    /// not a single control).
    pub fn set_content_height(&self, height: Px) {
        self.shared.set_content_height_px(height.value());
    }

    /// A shared handle to the scroll state, for a composite widget that resizes
    /// its content extent from a window-size callback.
    pub(crate) fn shared(&self) -> Rc<ScrollShared> {
        Rc::clone(&self.shared)
    }

    /// Scrolls to `offset` from the top, clamped to the content.
    pub fn scroll_to(&self, offset: Px) {
        self.shared.scroll_to_px(offset.value());
    }

    /// The current scroll offset.
    pub fn scroll_offset(&self) -> Px {
        Px(self.shared.offset.get())
    }

    /// The full content extent. Call after changing the content's own size and
    /// then [`ScrollView::scroll_to`].
    pub fn content_height(&self) -> Px {
        Px(self.shared.content_height.get())
    }
}

impl AsControl for ScrollView {
    fn control(&self) -> &Control {
        &self.control
    }
}

impl Themed for ScrollView {
    fn apply_theme(&self, theme: &Theme) {
        sys::apply_native_theme(
            self.control.hwnd(),
            sys::NativeControlKind::Scrollable,
            theme.is_dark,
        );
        sys::window::invalidate(self.control.hwnd());
    }
}

impl Drop for ScrollView {
    fn drop(&mut self) {
        crate::theme::unregister_themed(self.control.hwnd());
    }
}

/// The viewport's window procedure: size, scrollbar and wheel input.
struct ScrollHandler {
    shared: Rc<ScrollShared>,
}

impl WindowHandler for ScrollHandler {
    fn message(&self, _window: &Window, message: Message) -> Option<LResult> {
        match message {
            Message::Size { .. } => {
                self.shared.on_size();
                Some(0)
            }
            Message::MouseWheel {
                delta, horizontal, ..
            } if !horizontal => {
                self.shared.wheel(delta);
                Some(0)
            }
            Message::Other { code, wparam, .. } if sys::scroll::is_vscroll(code) => {
                if let Some(request) = sys::scroll::decode_vscroll(wparam) {
                    self.handle_scroll(request);
                }
                Some(0)
            }
            _ => None,
        }
    }
}

impl ScrollHandler {
    fn handle_scroll(&self, request: sys::scroll::ScrollRequest) {
        use sys::scroll::ScrollRequest as Req;
        let shared = &self.shared;
        let page = shared.page.get();
        match request {
            Req::LineUp => shared.scroll_by_px(-shared.notch.get()),
            Req::LineDown => shared.scroll_by_px(shared.notch.get()),
            Req::PageUp => shared.scroll_by_px(-page),
            Req::PageDown => shared.scroll_by_px(page),
            Req::Top => shared.scroll_to_px(0),
            Req::Bottom => shared.scroll_to_px(shared.max_offset()),
            Req::ThumbTrack(_) | Req::ThumbPosition(_) => {
                let track = sys::scroll::track_position(shared.viewport.get());
                shared.scroll_to_px(track);
            }
            Req::EndScroll => {}
        }
    }
}
