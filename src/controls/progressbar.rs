#![forbid(unsafe_code)]

//! An owner-drawn progress bar.
//!
//! The native `msctls_progress32` ignores dark mode through documented APIs
//! (it has no bar/track colour API that survives visual styles), so this is a
//! small custom child window that paints its track and fill from semantic
//! theme tokens — the same approach as the owner-drawn status bar and toolbar.

use std::cell::RefCell;
use std::ops::RangeInclusive;
use std::rc::Rc;

use crate::app::Ui;
use crate::color::Color;
use crate::controls::control::{AsControl, Control};
use crate::controls::progressbar_theme::ProgressBarTheme;
use crate::error::Result;
use crate::gdi::Paint;
use crate::geometry::Rect;
use crate::message::{Message, TimerId};
use crate::sys;
use crate::theme::{Theme, Themed};
use crate::units::dip;
use crate::window::{Window, WindowClass, WindowExStyle, WindowHandler, WindowStyle};

/// How often the marquee animation advances, in milliseconds.
const MARQUEE_MS: u32 = 30;
/// How far the marquee highlight travels per tick, as a fraction of its range.
const MARQUEE_STEP: f32 = 0.02;
/// The marquee highlight's width, as a fraction of the bar's width.
const MARQUEE_FRACTION: f32 = 0.3;

/// The visual state of a progress bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgressState {
    /// Normal progress (accent fill).
    Normal,
    /// Paused (caution fill).
    Paused,
    /// Error (danger fill).
    Error,
}

struct ProgressBarState {
    theme: ProgressBarTheme,
    min: i32,
    max: i32,
    value: i32,
    state: ProgressState,
    marquee: bool,
    offset: f32,
    timer: Option<TimerId>,
    bounds: Rect,
}

impl ProgressBarState {
    fn fill_color(&self) -> Color {
        match self.state {
            ProgressState::Normal => self.theme.fill,
            ProgressState::Paused => self.theme.paused,
            ProgressState::Error => self.theme.error,
        }
    }

    /// Stores `range`, swapping a reversed range so `min <= max` always holds.
    fn set_range(&mut self, range: RangeInclusive<i32>) {
        let (mut start, mut end) = (*range.start(), *range.end());
        if start > end {
            std::mem::swap(&mut start, &mut end);
        }
        self.min = start;
        self.max = end;
        self.value = self.value.clamp(start, end);
    }

    /// Stores `value`, clamped to the current range.
    fn set_value(&mut self, value: i32) {
        self.value = value.clamp(self.min, self.max);
    }

    /// Advances the marquee highlight, wrapping at the end.
    fn advance_marquee(&mut self) {
        if !self.marquee {
            return;
        }
        self.offset += MARQUEE_STEP;
        if self.offset > 1.0 {
            self.offset = 0.0;
        }
    }

    /// The rectangle of the filled portion for the current value, if any.
    fn value_fill(&self) -> Option<Rect> {
        let span = self.max - self.min;
        if span <= 0 || self.bounds.width() <= 0 {
            return None;
        }
        let fraction = (self.value - self.min) as f32 / span as f32;
        let width = (self.bounds.width() as f32 * fraction).round() as i32;
        if width <= 0 {
            return None;
        }
        let width = width.min(self.bounds.width());
        Some(Rect::new(
            self.bounds.left,
            self.bounds.top,
            self.bounds.left + width,
            self.bounds.bottom,
        ))
    }

    /// The rectangle of the marquee highlight for the current offset.
    fn marquee_fill(&self) -> Option<Rect> {
        let width = (self.bounds.width() as f32 * MARQUEE_FRACTION) as i32;
        if width <= 0 {
            return None;
        }
        let travel = (self.bounds.width() - width).max(0);
        let left = self.bounds.left + (travel as f32 * self.offset).round() as i32;
        Some(Rect::new(
            left,
            self.bounds.top,
            left + width,
            self.bounds.bottom,
        ))
    }

    fn draw(&self, canvas: &crate::gdi::Canvas) {
        let radius = self.bounds.height().max(1);
        canvas.round_rect(self.bounds, radius, self.theme.track, None);

        let fill = if self.marquee {
            self.marquee_fill()
        } else {
            self.value_fill()
        };
        if let Some(rect) = fill {
            let radius = self.bounds.height().min(rect.width()).max(1);
            canvas.round_rect(rect, radius, self.fill_color(), None);
        }
    }
}

struct ProgressBarHandler {
    state: Rc<RefCell<ProgressBarState>>,
}

impl WindowHandler for ProgressBarHandler {
    fn message(&self, window: &Window, message: Message) -> Option<isize> {
        match message {
            Message::Paint => {
                if let Some(paint) = Paint::begin(window.hwnd()) {
                    self.state.borrow().draw(paint.canvas());
                }
                Some(0)
            }
            Message::Size { width, height } => {
                self.state.borrow_mut().bounds = Rect::new(0, 0, width, height);
                Some(0)
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
