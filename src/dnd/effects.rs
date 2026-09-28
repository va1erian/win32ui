#![forbid(unsafe_code)]

//! Drop effects: what a drop does with the dragged data.

use std::ops::BitOr;

use windows::Win32::System::Ole::{DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE};

use crate::message::Modifiers;

/// What a drop will do, shown to the user as the drag cursor's glyph.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DropEffect {
    /// The target does not accept the drop (the "not allowed" cursor).
    #[default]
    None,
    /// The data is copied.
    Copy,
    /// The data is moved.
    Move,
    /// A link to the data is created.
    Link,
}

impl DropEffect {
    /// The raw `DROPEFFECT_*` bits (`oleidl.h`).
    pub(crate) fn bits(self) -> u32 {
        match self {
            DropEffect::None => 0,
            DropEffect::Copy => DROPEFFECT_COPY.0,
            DropEffect::Move => DROPEFFECT_MOVE.0,
            DropEffect::Link => DROPEFFECT_LINK.0,
        }
    }

    /// The effect in raw `bits`: a single flag normally; the first of
    /// move, copy, link when several are set; `None` for none.
    pub(crate) fn from_bits(bits: u32) -> DropEffect {
        if bits & DROPEFFECT_MOVE.0 != 0 {
            DropEffect::Move
        } else if bits & DROPEFFECT_COPY.0 != 0 {
            DropEffect::Copy
        } else if bits & DROPEFFECT_LINK.0 != 0 {
            DropEffect::Link
        } else {
            DropEffect::None
        }
    }
}

/// The set of [`DropEffect`]s a drag source allows.
///
/// Combine them with `|`: `DropEffects::COPY | DropEffects::MOVE`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DropEffects(u32);

impl DropEffects {
    /// No effect: nothing may be dropped.
    pub const NONE: DropEffects = DropEffects(0);
    /// Copying is allowed.
    pub const COPY: DropEffects = DropEffects(DROPEFFECT_COPY.0);
    /// Moving is allowed.
    pub const MOVE: DropEffects = DropEffects(DROPEFFECT_MOVE.0);
    /// Linking is allowed.
    pub const LINK: DropEffects = DropEffects(DROPEFFECT_LINK.0);

    pub(crate) const fn from_bits(bits: u32) -> DropEffects {
        DropEffects(bits & (Self::COPY.0 | Self::MOVE.0 | Self::LINK.0))
    }

    pub(crate) const fn bits(self) -> u32 {
        self.0
    }

    /// Whether `effect` is in the set. [`DropEffect::None`] is always
    /// contained.
    pub fn contains(self, effect: DropEffect) -> bool {
        self.0 & effect.bits() == effect.bits()
    }

    /// The effect the modifier keys ask for among the allowed ones, following
    /// Explorer's convention: Ctrl prefers copy, anything else prefers move (Shift
    /// included); then the remaining allowed effects in the order move, copy,
    /// link, else [`DropEffect::None`].
    pub fn preferred(self, modifiers: Modifiers) -> DropEffect {
        let order = if modifiers.ctrl {
            [DropEffect::Copy, DropEffect::Move, DropEffect::Link]
        } else {
            [DropEffect::Move, DropEffect::Copy, DropEffect::Link]
        };
        order
            .into_iter()
            .find(|effect| self.contains(*effect))
            .unwrap_or(DropEffect::None)
    }
}

impl BitOr for DropEffects {
    type Output = DropEffects;

    fn bitor(self, other: DropEffects) -> DropEffects {
        DropEffects(self.0 | other.0)
    }
}

impl From<DropEffect> for DropEffects {
    fn from(effect: DropEffect) -> DropEffects {
        DropEffects(effect.bits())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effects_round_trip_through_bits() {
        for effect in [
            DropEffect::None,
            DropEffect::Copy,
            DropEffect::Move,
            DropEffect::Link,
        ] {
            assert_eq!(DropEffect::from_bits(effect.bits()), effect);
        }
        assert_eq!(DropEffect::from_bits(3), DropEffect::Move);
    }

    #[test]
    fn sets_combine_and_contain() {
        let both = DropEffects::COPY | DropEffects::MOVE;
        assert!(both.contains(DropEffect::Copy));
        assert!(both.contains(DropEffect::Move));
        assert!(!both.contains(DropEffect::Link));
        assert!(DropEffects::NONE.contains(DropEffect::None));
        assert_eq!(DropEffects::from(DropEffect::Link), DropEffects::LINK);
    }

    #[test]
    fn preferred_follows_the_modifiers() {
        let both = DropEffects::COPY | DropEffects::MOVE;
        let none = Modifiers::NONE;
        let ctrl = Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        };
        assert_eq!(both.preferred(none), DropEffect::Move);
        assert_eq!(both.preferred(ctrl), DropEffect::Copy);
        assert_eq!(DropEffects::COPY.preferred(none), DropEffect::Copy);
        assert_eq!(DropEffects::LINK.preferred(ctrl), DropEffect::Link);
        assert_eq!(DropEffects::NONE.preferred(none), DropEffect::None);
    }
}
