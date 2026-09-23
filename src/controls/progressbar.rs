#![forbid(unsafe_code)]

//! An owner-drawn progress bar.
//!
//! The native `msctls_progress32` ignores dark mode through documented APIs
//! (it has no bar/track colour API that survives visual styles), so this is a
//! small custom child window that paints its track and fill from semantic
//! theme tokens — the same approach as the owner-drawn status bar and toolbar.

mod draw;
mod state;

use std::cell::RefCell;
use std::ops::RangeInclusive;
use std::rc::Rc;

use crate::app::Ui;
use crate::controls::control::{AsControl, Control};
use crate::controls::progressbar_theme::ProgressBarTheme;
use crate::d2d::D2dSurface;
use crate::error::Result;
use crate::gdi::Paint;
use crate::geometry::Rect;
use crate::message::Message;
use crate::sys;
use crate::theme::{Theme, Themed};
use crate::units::dip;
use crate::window::{Window, WindowClass, WindowExStyle, WindowHandler, WindowStyle};
use state::ProgressBarState;
pub use state::ProgressState;

/// How often the marquee animation advances, in milliseconds.
const MARQUEE_MS: u32 = 30;

/// How the bar is drawn. Direct2D is tried on the first paint; if it cannot be
/// created (a broken driver, say) the bar stays on GDI for good.
enum Renderer {
    Untried,
    Direct2d(Box<D2dSurface>),
    Gdi,
}

struct ProgressBarHandler {
    state: Rc<RefCell<ProgressBarState>>,
    renderer: RefCell<Renderer>,
}

impl ProgressBarHandler {
    fn paint(&self, window: &Window) {
        if self.paint_d2d(window) {
            return;
        }
        if let Some(paint) = Paint::begin(window.hwnd()) {
            self.state.borrow().draw_gdi(paint.canvas());
        }
    }

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
        self.state.borrow().draw_d2d(&mut canvas);
        if canvas.end_draw().is_err() {
            *renderer = Renderer::Gdi;
            window.invalidate();
        }
        true
    }

    fn with_surface(&self, apply: impl FnOnce(&D2dSurface)) {
        if let Renderer::Direct2d(surface) = &*self.renderer.borrow() {
            apply(surface);
        }
    }
}

impl WindowHandler for ProgressBarHandler {
    fn message(&self, window: &Window, message: Message) -> Option<isize> {
        match message {
            Message::Paint => {
                self.paint(window);
                Some(0)
            }
            Message::Size { width, height } => {
                self.state.borrow_mut().bounds = Rect::new(0, 0, width, height);
                self.with_surface(|surface| surface.resize(width, height));
                Some(0)
            }
            Message::DpiChanged { dpi, .. } => {
                self.with_surface(|surface| surface.set_dpi(dpi));
                None
            }
            Message::Timer { id } => {
                let mut state = self.state.borrow_mut();
                if state.timer == Some(id) {
                    state.advance_marquee();
                    drop(state);
                    window.invalidate();
                }
                Some(0)
            }
            _ if D2dSurface::is_erase_background(&message)
                && matches!(*self.renderer.borrow(), Renderer::Direct2d(_)) =>
            {
                Some(1)
            }
            _ => None,
        }
    }
}

/// An owner-drawn progress bar with a range, a value, a state and an optional
/// marquee animation.
///
/// Build it with [`ProgressBar::new`] and the chaining setters:
/// `ProgressBar::new(ui).range(0..=100).value(40)`.
pub struct ProgressBar {
    window: Window,
    control: Control,
    shared: Rc<RefCell<ProgressBarState>>,
}

impl ProgressBar {
    /// Creates the bar as a child of the window behind `ui`, adopting `ui`'s
    /// theme. Position it with [`ControlExt::set_bounds`](crate::ControlExt::set_bounds).
    pub fn new<M: 'static>(ui: &mut Ui<M>) -> Result<ProgressBar> {
        let app_theme = ui.theme();
        let theme = ProgressBarTheme::from_theme(&app_theme);
        let height = dip(8.0).to_px(ui.dpi()).value();
        let bounds = Rect::new(0, 0, 0, height);
        let shared = Rc::new(RefCell::new(ProgressBarState {
            theme,
            min: 0,
            max: 100,
            value: 0,
            state: ProgressState::Normal,
            marquee: false,
            offset: 0.0,
            timer: None,
            bounds,
        }));
        let class = WindowClass::register("win32ui.progressbar", app_theme.background)?;
        let handler = ProgressBarHandler {
            state: Rc::clone(&shared),
            renderer: RefCell::new(Renderer::Untried),
        };
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
        let bar = ProgressBar {
            window,
            control,
            shared,
        };
        {
            let weak = Rc::downgrade(&bar.shared);
            let hwnd = bar.control.hwnd();
            let parent = ui.hwnd();
            crate::theme::register_themed(
                parent,
                hwnd,
                Rc::new(move |applied| {
                    if let Some(state) = weak.upgrade() {
                        let mut state = state.borrow_mut();
                        state.theme = ProgressBarTheme::from_theme(applied);
                        drop(state);
                        sys::set_class_background(hwnd, applied.background);
                        sys::window::invalidate(hwnd);
                    }
                }),
            );
        }
        Ok(bar)
    }

    /// Sets the range the value is drawn against, as `min..=max`. A reversed
    /// range is swapped; the value is clamped into it.
    pub fn range(self, range: RangeInclusive<i32>) -> Self {
        self.set_range(range);
        self
    }

    /// Sets the current value, clamped to the range.
    pub fn value(self, value: i32) -> Self {
        self.set_value(value);
        self
    }

    /// Turns the marquee animation (an indeterminate sweep) on or off.
    pub fn marquee(self, on: bool) -> Self {
        self.set_marquee(on);
        self
    }

    /// Sets the visual state (normal, paused, error).
    pub fn state(self, state: ProgressState) -> Self {
        self.set_state(state);
        self
    }

    /// Sets the range the value is drawn against, as `min..=max`.
    pub fn set_range(&self, range: RangeInclusive<i32>) {
        self.shared.borrow_mut().set_range(range);
        self.window.invalidate();
    }

    /// Sets the current value, clamped to the range.
    pub fn set_value(&self, value: i32) {
        self.shared.borrow_mut().set_value(value);
        self.window.invalidate();
    }

    /// Sets the visual state (normal, paused, error).
    pub fn set_state(&self, state: ProgressState) {
        self.shared.borrow_mut().state = state;
        self.window.invalidate();
    }

    /// Turns the marquee animation (an indeterminate sweep) on or off.
    pub fn set_marquee(&self, on: bool) {
        {
            let mut shared = self.shared.borrow_mut();
            if shared.marquee == on {
                return;
            }
            shared.marquee = on;
            shared.offset = 0.0;
        }
        if on {
            if let Ok(id) = self.window.set_timer(MARQUEE_MS) {
                self.shared.borrow_mut().timer = Some(id);
            }
        } else if let Some(id) = self.shared.borrow_mut().timer.take() {
            self.window.kill_timer(id);
        }
        self.window.invalidate();
    }
}

impl AsControl for ProgressBar {
    fn control(&self) -> &Control {
        &self.control
    }
}

impl Themed for ProgressBar {
    fn apply_theme(&self, theme: &Theme) {
        self.shared.borrow_mut().theme = ProgressBarTheme::from_theme(theme);
        sys::set_class_background(self.control.hwnd(), theme.background);
        self.window.invalidate();
    }
}

impl Drop for ProgressBar {
    fn drop(&mut self) {
        crate::theme::unregister_themed(self.control.hwnd());
    }
}

#[cfg(test)]
mod tests;
