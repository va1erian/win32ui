#![forbid(unsafe_code)]

//! The slider's easing state: three 0..1 channels (hover, press, keyboard
//! focus) that glide towards their targets. Pure arithmetic, so it is
//! unit-tested without a window.
//!
//! Only geometry is animated (the thumb and track grow, the focus ring
//! thickens); colours stay discrete theme tokens, so a running animation never
//! creates new Direct2D brushes.

/// How long a channel takes to travel from 0 to 1, in seconds.
const DURATION_S: f32 = 0.12;

/// One eased value chasing a target at a constant rate.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Channel {
    current: f32,
    target: f32,
}

impl Channel {
    fn advance(&mut self, dt: f32) {
        let step = dt / DURATION_S;
        self.current = if self.target > self.current {
            (self.current + step).min(self.target)
        } else {
            (self.current - step).max(self.target)
        };
    }

    fn settled(&self) -> bool {
        self.current == self.target
    }

    /// Smoothstep of the linear progress: slow in, slow out.
    fn eased(&self) -> f32 {
        self.current * self.current * (3.0 - 2.0 * self.current)
    }
}

/// The three animated channels of a slider.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Anim {
    hover: Channel,
    press: Channel,
    focus: Channel,
}

/// Where each channel is heading (`0.0` or `1.0`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Targets {
    pub(super) hover: bool,
    pub(super) press: bool,
    pub(super) focus: bool,
}

impl Anim {
    /// Points the channels at `targets`. Returns whether any target changed,
    /// i.e. whether an animation needs to run.
    pub(super) fn retarget(&mut self, targets: Targets) -> bool {
        let goal = |on: bool| if on { 1.0 } else { 0.0 };
        let next = Anim {
            hover: Channel {
                target: goal(targets.hover),
                ..self.hover
            },
            press: Channel {
                target: goal(targets.press),
                ..self.press
            },
            focus: Channel {
                target: goal(targets.focus),
                ..self.focus
            },
        };
        let changed = next != *self;
        *self = next;
        changed
    }

    /// Jumps every channel to its target (animation effects are off).
    pub(super) fn snap(&mut self) {
        for channel in [&mut self.hover, &mut self.press, &mut self.focus] {
            channel.current = channel.target;
        }
    }

    /// Advances every channel by `dt` seconds. Returns whether any channel is
    /// still moving.
    pub(super) fn advance(&mut self, dt: f32) -> bool {
        for channel in [&mut self.hover, &mut self.press, &mut self.focus] {
            channel.advance(dt);
        }
        !self.is_settled()
    }

    /// Whether every channel has reached its target.
    pub(super) fn is_settled(&self) -> bool {
        self.hover.settled() && self.press.settled() && self.focus.settled()
    }

    /// The eased hover amount, `0.0..=1.0`.
    pub(super) fn hover(&self) -> f32 {
        self.hover.eased()
    }

    /// The eased press amount, `0.0..=1.0`.
    pub(super) fn press(&self) -> f32 {
        self.press.eased()
    }

    /// The eased keyboard-focus amount, `0.0..=1.0`.
    pub(super) fn focus(&self) -> f32 {
        self.focus.eased()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOVER: Targets = Targets {
        hover: true,
        press: false,
        focus: false,
    };

    #[test]
    fn retargeting_reports_whether_anything_changed() {
        let mut anim = Anim::default();
        assert!(!anim.retarget(Targets::default()));
        assert!(anim.retarget(HOVER));
        assert!(!anim.retarget(HOVER));
    }

    #[test]
    fn a_channel_takes_about_the_duration_and_then_settles() {
        let mut anim = Anim::default();
        anim.retarget(HOVER);
        assert!(anim.advance(0.06));
        assert!((anim.hover() - 0.5).abs() < 1e-3, "smoothstep midpoint");
        assert!(!anim.advance(0.06));
        assert_eq!(anim.hover(), 1.0);
        assert!(anim.is_settled());
    }

    #[test]
    fn a_long_frame_never_overshoots() {
        let mut anim = Anim::default();
        anim.retarget(HOVER);
        anim.advance(10.0);
        assert_eq!(anim.hover(), 1.0);
        anim.retarget(Targets::default());
        anim.advance(10.0);
        assert_eq!(anim.hover(), 0.0);
    }

    #[test]
    fn snap_jumps_to_the_targets() {
        let mut anim = Anim::default();
        anim.retarget(Targets {
            hover: true,
            press: true,
            focus: true,
        });
        anim.snap();
        assert!(anim.is_settled());
        assert_eq!((anim.hover(), anim.press(), anim.focus()), (1.0, 1.0, 1.0));
    }

    #[test]
    fn reversing_mid_flight_eases_back_from_where_it_is() {
        let mut anim = Anim::default();
        anim.retarget(HOVER);
        anim.advance(0.06);
        anim.retarget(Targets::default());
        anim.advance(0.03);
        assert!(anim.hover() > 0.0 && anim.hover() < 0.5);
    }
}
