#![forbid(unsafe_code)]

//! The thin shell that turns window input into [`SliderState`] transitions and
//! the state's coalesced results into [`SliderEvent`]s.

use std::cell::{Cell, RefCell};
use std::time::Instant;

use crate::controls::custom::{CustomWidget, Input, Renderer, WidgetCx};
use crate::d2d::{D2dCanvas, RectF, pixels_to_dips};
use crate::gdi::Canvas;
use crate::geometry::{Rect, Size};
use crate::message::{Key, MouseButton};
use crate::sys;
use crate::theme::Theme;
use crate::units::dip;

use super::state::{Axis, SliderState, Step};

/// The natural size of a horizontal slider, in dip; a vertical one swaps them.
pub(super) const LONG_SIDE: f32 = 160.0;
pub(super) const SHORT_SIDE: f32 = 28.0;
/// The wheel rotation of one notch (`WHEEL_DELTA` in `winuser.h`).
const WHEEL_NOTCH: f64 = 120.0;
/// The wheel step is divided by this while Shift is held.
const FINE_DIVISOR: f64 = 10.0;
/// The longest gap one animation tick may advance, so a stalled UI thread does
/// not make the animation jump.
const MAX_TICK_S: f32 = 0.05;

/// What a slider tells its owner.
pub(super) enum SliderEvent {
    /// The value moved (at most once per painted frame while dragging).
    Change(f64),
    /// The user finished a gesture: release, a key step or a wheel notch.
    Commit(f64),
    /// The pointer is over `value`.
    Hover(f64),
}

pub(super) struct SliderWidget {
    pub(super) state: RefCell<SliderState>,
    /// When the animation last advanced.
    last_tick: Cell<Option<Instant>>,
    /// Whether the focus that is arriving came from a click (no focus ring).
    pointer_focus: Cell<bool>,
}

impl SliderWidget {
    pub(super) fn new(state: SliderState) -> SliderWidget {
        SliderWidget {
            state: RefCell::new(state),
            last_tick: Cell::new(None),
            pointer_focus: Cell::new(false),
        }
    }

    /// The main-axis coordinate of a client-space point, and the axis, in dip.
    fn locate(&self, cx: &WidgetCx<SliderEvent>, x: i32, y: i32) -> (Axis, f64) {
        let state = self.state.borrow();
        let bounds = cx.bounds();
        let to_dip = |pixels: i32| f64::from(pixels_to_dips(pixels, cx.dpi()));
        if state.vertical {
            (state.axis(to_dip(bounds.height())), to_dip(y))
        } else {
            (state.axis(to_dip(bounds.width())), to_dip(x))
        }
    }

    /// Repaints, and starts the easing timer when the interaction state
    /// changed which way the animation is heading. Animation effects turned
    /// off in Windows snap instead.
    fn refresh(&self, cx: &WidgetCx<SliderEvent>) {
        let mut state = self.state.borrow_mut();
        let targets = state.targets();
        if state.anim.retarget(targets) {
            if sys::animation::client_area_animation() {
                if self.last_tick.get().is_none() {
                    self.last_tick.set(Some(Instant::now()));
                }
                cx.request_animation(true);
            } else {
                state.anim.snap();
            }
        }
        drop(state);
        cx.invalidate();
    }

    fn tick(&self, cx: &WidgetCx<SliderEvent>) {
        let now = Instant::now();
        let dt = self
            .last_tick
            .replace(Some(now))
            .map_or(0.0, |last| (now - last).as_secs_f32().min(MAX_TICK_S));
        if !self.state.borrow_mut().anim.advance(dt) {
            self.last_tick.set(None);
            cx.request_animation(false);
        }
        cx.invalidate();
    }

    /// Sends the coalesced `on_change` and `on_hover` values that piled up
    /// since the last painted frame.
    fn flush(&self, cx: &WidgetCx<SliderEvent>) {
        let (change, hover) = {
            let mut state = self.state.borrow_mut();
            (state.take_pending_change(), state.take_pending_hover())
        };
        if let Some(value) = change {
            cx.emit(SliderEvent::Change(value));
        }
        if let Some(value) = hover {
            cx.emit(SliderEvent::Hover(value));
        }
    }

    fn release(&self, cx: &WidgetCx<SliderEvent>) {
        let ended = self.state.borrow_mut().release();
        if let Some(value) = ended {
            self.flush(cx);
            cx.emit(SliderEvent::Commit(value));
            self.refresh(cx);
        }
    }

    /// Applies a keyboard or wheel step and raises its events at once.
    fn step(&self, cx: &WidgetCx<SliderEvent>, step: Step, sign: f64) {
        if self.state.borrow().is_dragging() {
            return;
        }
        let moved = self.state.borrow_mut().step(step, sign);
        if moved {
            self.flush(cx);
            cx.emit(SliderEvent::Commit(self.state.borrow().value()));
            cx.invalidate();
        }
    }

    fn key(&self, cx: &WidgetCx<SliderEvent>, key: Key) {
        let (small, large, rtl) = {
            let state = self.state.borrow();
            (state.small_step, state.large_step, state.rtl)
        };
        let (forward, backward) = if rtl {
            (Key::LEFT, Key::RIGHT)
        } else {
            (Key::RIGHT, Key::LEFT)
        };
        let (step, sign) = match key {
            k if k == forward || k == Key::UP => (Step::Small(small), 1.0),
            k if k == backward || k == Key::DOWN => (Step::Small(small), -1.0),
            Key::PAGE_UP => (Step::Large(large), 1.0),
            Key::PAGE_DOWN => (Step::Large(large), -1.0),
            Key::HOME => (Step::Min, -1.0),
            Key::END => (Step::Max, 1.0),
            _ => return,
        };
        self.state.borrow_mut().show_focus_ring();
        self.step(cx, step, sign);
        self.refresh(cx);
    }
}

impl CustomWidget for SliderWidget {
    type Event = SliderEvent;

    /// Never called: the slider paints with Direct2D, and when Direct2D is
    /// unavailable the host fills the theme background instead.
    fn paint(&self, _canvas: &Canvas, _bounds: Rect, _theme: &Theme) {}

    fn renderer(&self) -> Renderer {
        Renderer::Direct2D
    }

    fn paint_d2d(&self, canvas: &mut D2dCanvas<'_>, bounds: RectF, theme: &Theme) {
        self.state.borrow().draw(canvas, bounds, theme);
    }

    fn wants_arrow_keys(&self) -> bool {
        true
    }

    fn preferred_size(&self, dpi: u32) -> Option<Size> {
        let (long, short) = (
            dip(LONG_SIDE).to_px(dpi).value(),
            dip(SHORT_SIDE).to_px(dpi).value(),
        );
        Some(if self.state.borrow().vertical {
            Size::new(short, long)
        } else {
            Size::new(long, short)
        })
    }

    fn input(&self, input: Input, cx: &mut WidgetCx<SliderEvent>) {
        if !self.state.borrow().enabled && !matches!(input, Input::Frame | Input::Tick) {
            return;
        }
        match input {
            Input::MouseDown {
                x,
                y,
                button: MouseButton::Left,
                ..
            } => {
                let (axis, pos) = self.locate(cx, x, y);
                self.state.borrow_mut().press(&axis, pos);
                cx.capture();
                self.pointer_focus.set(true);
                cx.focus();
                self.refresh(cx);
            }
            Input::MouseMove { x, y, .. } => {
                let (axis, pos) = self.locate(cx, x, y);
                if self.state.borrow_mut().pointer_moved(&axis, pos) {
                    self.refresh(cx);
                }
            }
            Input::MouseUp {
                button: MouseButton::Left,
                ..
            }
            | Input::CaptureChanged => {
                self.release(cx);
                cx.release_capture();
            }
            Input::MouseLeave => {
                self.state.borrow_mut().pointer_left();
                self.refresh(cx);
            }
            Input::MouseWheel {
                delta,
                horizontal: false,
                modifiers,
                ..
            } => {
                let base = self.state.borrow().wheel_step;
                let size = if modifiers.shift {
                    base / FINE_DIVISOR
                } else {
                    base
                };
                self.step(cx, Step::Wheel(size), f64::from(delta) / WHEEL_NOTCH);
            }
            Input::KeyDown {
                key, system: false, ..
            } => self.key(cx, key),
            Input::SetFocus => {
                let visible = !self.pointer_focus.replace(false);
                self.state.borrow_mut().set_focus(true, visible);
                self.refresh(cx);
            }
            Input::KillFocus => {
                self.state.borrow_mut().set_focus(false, false);
                self.refresh(cx);
            }
            Input::Tick => self.tick(cx),
            Input::Frame => self.flush(cx),
            _ => {}
        }
    }
}
