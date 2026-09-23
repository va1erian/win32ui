#![forbid(unsafe_code)]

//! The slider's pure state machine: the value, the value <-> position mapping,
//! the drag, the keyboard steps and the coalesced events. Everything is `f64`
//! and in device-independent pixels, so it is DPI-agnostic and unit-tested
//! without a window; the widget shell only feeds it input and paints from it.

use super::anim::{Anim, Targets};

/// Space kept free at both ends of the main axis, so the thumb (and its focus
/// ring) fits inside the window at either extreme.
pub(super) const END_PADDING: f64 = 12.0;
/// How far from the thumb's centre a press still grabs the thumb instead of
/// jumping to the pointer.
const THUMB_GRAB_RADIUS: f64 = 12.0;

/// The track's placement along the widget's main axis, in device-independent
/// pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Axis {
    span: f64,
    reversed: bool,
}

impl Axis {
    /// The axis of a widget `length` dip long. `reversed` puts the minimum at
    /// the far end (vertical sliders, right-to-left horizontal ones).
    pub(super) fn new(length: f64, reversed: bool) -> Axis {
        Axis {
            span: (length - 2.0 * END_PADDING).max(0.0),
            reversed,
        }
    }

    /// The fraction `0.0..=1.0` of the range at `pos`, clamped: a pointer far
    /// outside the widget still maps to an end.
    pub(super) fn fraction_at(&self, pos: f64) -> f64 {
        if self.span == 0.0 {
            return 0.0;
        }
        let fraction = ((pos - END_PADDING) / self.span).clamp(0.0, 1.0);
        if self.reversed {
            1.0 - fraction
        } else {
            fraction
        }
    }

    /// The position of `fraction` of the range.
    pub(super) fn pos_of(&self, fraction: f64) -> f64 {
        let along = if self.reversed {
            1.0 - fraction
        } else {
            fraction
        };
        END_PADDING + along * self.span
    }
}

/// A keyboard or wheel action on the value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Step {
    Small(f64),
    Large(f64),
    Wheel(f64),
    Min,
    Max,
}

/// An active drag: how far from the thumb's centre the pointer grabbed it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Drag {
    grab: f64,
}

/// The slider's data and interaction state.
#[derive(Clone, Debug)]
pub(super) struct SliderState {
    min: f64,
    max: f64,
    value: f64,
    pub(super) buffered: Option<(f64, f64)>,
    pub(super) small_step: f64,
    pub(super) large_step: f64,
    pub(super) wheel_step: f64,
    pub(super) vertical: bool,
    pub(super) rtl: bool,
    pub(super) ticks: u32,
    pub(super) enabled: bool,
    drag: Option<Drag>,
    hover: bool,
    focused: bool,
    focus_visible: bool,
    pub(super) anim: Anim,
    pending_change: Option<f64>,
    pending_hover: Option<f64>,
}

impl SliderState {
    pub(super) fn new(min: f64, max: f64) -> SliderState {
        let (min, max) = if min <= max { (min, max) } else { (max, min) };
        let span = max - min;
        SliderState {
            min,
            max,
            value: min,
            buffered: None,
            small_step: span / 100.0,
            large_step: span / 10.0,
            wheel_step: span / 20.0,
            vertical: false,
            rtl: false,
            ticks: 0,
            enabled: true,
            drag: None,
            hover: false,
            focused: false,
            focus_visible: false,
            anim: Anim::default(),
            pending_change: None,
            pending_hover: None,
        }
    }

    pub(super) fn value(&self) -> f64 {
        self.value
    }

    pub(super) fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// The axis for a widget whose main-axis length is `length` dip.
    pub(super) fn axis(&self, length: f64) -> Axis {
        Axis::new(length, self.vertical || self.rtl)
    }

    /// The fraction `0.0..=1.0` of the range that `value` sits at.
    pub(super) fn fraction(&self, value: f64) -> f64 {
        let span = self.max - self.min;
        if span == 0.0 {
            0.0
        } else {
            ((value - self.min) / span).clamp(0.0, 1.0)
        }
    }

    fn value_at(&self, fraction: f64) -> f64 {
        self.min + fraction * (self.max - self.min)
    }

    /// Sets the value from the app. Ignored while the user drags (the widget
    /// owns the value then). Returns whether anything changed; it never
    /// raises an event.
    pub(super) fn set_value(&mut self, value: f64) -> bool {
        if self.drag.is_some() || value.is_nan() {
            return false;
        }
        let value = value.clamp(self.min, self.max);
        let changed = value != self.value;
        self.value = value;
        changed
    }

    /// Moves the value on behalf of the user and queues an `on_change`.
    fn move_to(&mut self, value: f64) -> bool {
        let value = value.clamp(self.min, self.max);
        if value == self.value {
            return false;
        }
        self.value = value;
        self.pending_change = Some(value);
        true
    }

    /// A press at `pos`: grabs the thumb if it is under the pointer, else
    /// jumps the thumb to the pointer; either way a drag begins. Returns
    /// whether the value changed.
    pub(super) fn press(&mut self, axis: &Axis, pos: f64) -> bool {
        let thumb = axis.pos_of(self.fraction(self.value));
        let on_thumb = (pos - thumb).abs() <= THUMB_GRAB_RADIUS;
        let grab = if on_thumb { pos - thumb } else { 0.0 };
        self.drag = Some(Drag { grab });
        self.hover = true;
        self.pending_hover = None;
        !on_thumb && self.move_to(self.value_at(axis.fraction_at(pos)))
    }

    /// The pointer moved to `pos`: continues the drag, or records the value
    /// under the pointer for `on_hover`. Returns whether a repaint is needed.
    pub(super) fn pointer_moved(&mut self, axis: &Axis, pos: f64) -> bool {
        if let Some(drag) = self.drag {
            return self.move_to(self.value_at(axis.fraction_at(pos - drag.grab)));
        }
        self.hover = true;
        self.pending_hover = Some(self.value_at(axis.fraction_at(pos)));
        true
    }

    /// The pointer left the widget.
    pub(super) fn pointer_left(&mut self) {
        self.hover = false;
        self.pending_hover = None;
    }

    /// The button was released or the capture lost. Returns the final value
    /// when a drag ended, for `on_commit`.
    pub(super) fn release(&mut self) -> Option<f64> {
        self.drag.take().map(|_| self.value)
    }

    /// Applies a keyboard or wheel step with `sign` (`1.0` up, `-1.0` down).
    /// Returns whether the value changed.
    pub(super) fn step(&mut self, step: Step, sign: f64) -> bool {
        let target = match step {
            Step::Small(size) | Step::Large(size) | Step::Wheel(size) => self.value + sign * size,
            Step::Min => self.min,
            Step::Max => self.max,
        };
        self.move_to(target)
    }

    /// Records keyboard focus; the ring shows only for keyboard focus.
    pub(super) fn set_focus(&mut self, focused: bool, visible: bool) {
        self.focused = focused;
        self.focus_visible = focused && visible;
    }

    /// Makes an already-focused slider show its focus ring (a key was pressed).
    pub(super) fn show_focus_ring(&mut self) {
        self.focus_visible = self.focused;
    }

    /// The animation targets implied by the interaction state.
    pub(super) fn targets(&self) -> Targets {
        Targets {
            hover: self.enabled && (self.hover || self.drag.is_some()),
            press: self.enabled && self.drag.is_some(),
            focus: self.enabled && self.focus_visible,
        }
    }

    /// Takes the `on_change` value waiting to be flushed, if any. Moves in
    /// one frame overwrite each other, so the last value wins.
    pub(super) fn take_pending_change(&mut self) -> Option<f64> {
        self.pending_change.take()
    }

    /// Takes the `on_hover` value waiting to be flushed, if any.
    pub(super) fn take_pending_hover(&mut self) -> Option<f64> {
        self.pending_hover.take()
    }

    /// Ends any drag and hover (the slider was disabled).
    pub(super) fn disable(&mut self) {
        self.enabled = false;
        self.drag = None;
        self.hover = false;
        self.pending_hover = None;
    }
}

#[cfg(test)]
mod tests;
