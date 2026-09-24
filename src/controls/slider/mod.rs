#![forbid(unsafe_code)]

//! A smooth, Direct2D-painted slider for seek bars and volume controls.
//!
//! The native trackbar snaps to integer positions, draws its thumb without
//! anti-aliasing and ignores dark mode, so this is a [`Custom`] widget instead:
//! the value is an `f64` end to end, the thumb is drawn at fractional pixel
//! positions, and a drag keeps the mouse captured so it tracks outside the
//! widget. While dragging, `on_change` fires at most once per painted frame
//! (the last value wins); `on_commit` fires once when the gesture ends.
//! Hover, press and keyboard focus ease over about 120 ms on a timer that runs
//! only while something is animating.
//!
//! ```ignore
//! let seek = Slider::new(ui, 0.0..=duration_secs)?
//!     .value(position)
//!     .on_change(|v| Some(Msg::SeekPreview(v)))
//!     .on_commit(|v| Some(Msg::Seek(v)))
//!     .on_hover(|v| Some(Msg::HoverTime(v)));
//! let volume = Slider::new(ui, 0.0..=1.0)?.vertical().on_change(|v| Some(Msg::Volume(v)));
//! ```

mod anim;
mod paint;
mod state;
mod widget;

use std::cell::RefCell;
use std::ops::{Range, RangeInclusive};
use std::rc::Rc;

use crate::app::Ui;
use crate::controls::control::{AsControl, Control, ControlExt};
use crate::controls::custom::Custom;
use crate::error::Result;
use crate::geometry::Rect;
use crate::theme::{Theme, Themed};
use crate::units::dip;

pub(crate) use state::SliderState;
use widget::{LONG_SIDE, SHORT_SIDE, SliderEvent, SliderWidget};

type ValueMapper<M> = RefCell<Option<Box<dyn Fn(f64) -> Option<M>>>>;

/// The app's closures for the slider's three events.
struct Handlers<M> {
    change: ValueMapper<M>,
    commit: ValueMapper<M>,
    hover: ValueMapper<M>,
}

impl<M> Handlers<M> {
    fn map(&self, event: SliderEvent) -> Option<M> {
        let (mapper, value) = match event {
            SliderEvent::Change(value) => (&self.change, value),
            SliderEvent::Commit(value) => (&self.commit, value),
            SliderEvent::Hover(value) => (&self.hover, value),
        };
        mapper.borrow().as_ref().and_then(|map| map(value))
    }
}

/// A slider over a continuous `f64` range.
///
/// Build it with [`Slider::new`] and the chaining setters; map its events to
/// the app's `Msg` with [`Slider::on_change`], [`Slider::on_commit`] and
/// [`Slider::on_hover`]. Size it through the layout, or with
/// [`ControlExt::set_bounds`].
pub struct Slider<M: 'static> {
    custom: Custom<SliderWidget, M>,
    handlers: Rc<Handlers<M>>,
}

impl<M: 'static> Slider<M> {
    /// Runs `f` on the slider's state.
    fn with_state<R>(&self, f: impl FnOnce(&mut SliderState) -> R) -> R {
        f(&mut self.custom.widget().borrow().state.borrow_mut())
    }

    /// Creates a horizontal slider over `range` as a child of the window
    /// behind `ui`, at the range's start. A reversed range is swapped.
    pub fn new(ui: &mut Ui<M>, range: RangeInclusive<f64>) -> Result<Slider<M>> {
        let widget = SliderWidget::new(SliderState::new(*range.start(), *range.end()));
        let handlers = Rc::new(Handlers {
            change: RefCell::new(None),
            commit: RefCell::new(None),
            hover: RefCell::new(None),
        });
        let mapper = Rc::clone(&handlers);
        let custom = Custom::new(ui, widget)?.on_event(move |event| mapper.map(event));
        custom.set_tab_stop(true);
        Ok(Slider { custom, handlers })
    }

    /// Sets the value, clamped to the range.
    pub fn value(self, value: f64) -> Self {
        self.set_value(value);
        self
    }

    /// Maps the value to a message while the user drags, at most once per
    /// painted frame with the last value winning. Use it for a live preview
    /// or a volume; use [`Slider::on_commit`] to act once.
    pub fn on_change(self, f: impl Fn(f64) -> Option<M> + 'static) -> Self {
        self.handlers.change.replace(Some(Box::new(f)));
        self
    }

    /// Maps the final value to a message once per gesture: when the drag is
    /// released, or on each keyboard step or wheel notch.
    pub fn on_commit(self, f: impl Fn(f64) -> Option<M> + 'static) -> Self {
        self.handlers.commit.replace(Some(Box::new(f)));
        self
    }

    /// Maps the value under the pointer to a message as it moves over the
    /// slider (coalesced per frame), for a hover preview such as a timestamp.
    pub fn on_hover(self, f: impl Fn(f64) -> Option<M> + 'static) -> Self {
        self.handlers.hover.replace(Some(Box::new(f)));
        self
    }

    /// Lays the slider out vertically, minimum at the bottom, and swaps its
    /// natural size.
    pub fn vertical(self) -> Self {
        self.with_state(|state| state.vertical = true);
        let dpi = self.custom.dpi();
        self.set_bounds(Rect::new(
            0,
            0,
            dip(SHORT_SIDE).to_px(dpi).value(),
            dip(LONG_SIDE).to_px(dpi).value(),
        ));
        self
    }

    /// Mirrors a horizontal slider for right-to-left layouts: the minimum is
    /// at the right, and the arrow keys follow.
    pub fn right_to_left(self, rtl: bool) -> Self {
        self.with_state(|state| state.rtl = rtl);
        self
    }

    /// Sets the keyboard steps in value units: an arrow key moves by `small`,
    /// PageUp/PageDown by `large`. The defaults are 1% and 10% of the range.
    pub fn key_steps(self, small: f64, large: f64) -> Self {
        self.with_state(|state| {
            state.small_step = small;
            state.large_step = large;
        });
        self
    }

    /// Sets how far one wheel notch moves the value (Shift divides it by ten).
    /// The default is 5% of the range.
    pub fn wheel_step(self, step: f64) -> Self {
        self.with_state(|state| state.wheel_step = step);
        self
    }

    /// Draws `count` evenly spaced tick marks (plus one for the far end)
    /// beside the track. There are none by default.
    pub fn tick_marks(self, count: u32) -> Self {
        self.with_state(|state| state.ticks = count);
        self.custom.invalidate();
        self
    }

    /// Sets the value, clamped to the range. Ignored while the user drags the
    /// thumb, so a playback timer can call it freely; it never raises an event
    /// and animates nothing, it only repaints at the new position.
    pub fn set_value(&self, value: f64) {
        if self.with_state(|state| state.set_value(value)) {
            self.custom.invalidate();
        }
    }

    /// The current value.
    pub fn current_value(&self) -> f64 {
        self.with_state(|state| state.value())
    }

    /// Shows the loaded part of the media as a lighter second fill: the
    /// values in `loaded`, clamped to the range. An empty range hides it.
    pub fn set_buffered(&self, loaded: Range<f64>) {
        let buffered = (loaded.start < loaded.end).then_some((loaded.start, loaded.end));
        self.with_state(|state| state.buffered = buffered);
        self.custom.invalidate();
    }

    /// Enables or disables the slider. A disabled slider ignores input, ends
    /// any drag and paints muted.
    pub fn set_enabled(&self, enabled: bool) {
        self.with_state(|state| {
            if enabled {
                state.enabled = true;
            } else {
                state.disable();
                let targets = state.targets();
                state.anim.retarget(targets);
                state.anim.snap();
            }
        });
        ControlExt::set_enabled(self, enabled);
        self.custom.invalidate();
    }

    /// Whether the user is dragging the thumb.
    pub fn is_dragging(&self) -> bool {
        self.with_state(|state| state.is_dragging())
    }

    /// Whether the easing timer is running. It runs only while a hover, press
    /// or focus animation is in flight, so an idle slider reports `false`.
    pub fn is_animating(&self) -> bool {
        self.custom.animation_timer_running()
    }
}

impl<M: 'static> AsControl for Slider<M> {
    fn control(&self) -> &Control {
        self.custom.control()
    }
}

impl<M: 'static> Themed for Slider<M> {
    fn apply_theme(&self, theme: &Theme) {
        self.custom.apply_theme(theme);
    }
}

#[cfg(test)]
mod tests {
    use super::state::SliderState;
    use super::widget::SliderWidget;
    use crate::controls::custom::{CustomWidget, Renderer};

    #[test]
    fn the_widget_paints_with_direct2d_and_grabs_the_arrow_keys() {
        let widget = SliderWidget::new(SliderState::new(0.0, 1.0));
        assert_eq!(widget.renderer(), Renderer::Direct2D);
        assert!(widget.wants_arrow_keys());
    }

    #[test]
    fn the_natural_size_swaps_for_a_vertical_slider() {
        let widget = SliderWidget::new(SliderState::new(0.0, 1.0));
        let horizontal = widget.preferred_size(96).unwrap();
        widget.state.borrow_mut().vertical = true;
        let vertical = widget.preferred_size(96).unwrap();
        assert_eq!(
            (horizontal.width, horizontal.height),
            (vertical.height, vertical.width)
        );
    }
}
