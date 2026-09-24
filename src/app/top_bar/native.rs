#![forbid(unsafe_code)]

//! Native slots of the material top bar: where a slot sits vertically in the
//! band, and the child control the bar keeps positioned in it.

use std::cell::Cell;
use std::rc::Rc;

use super::state::{Item, Kind};
use crate::controls::{AsControl, ControlExt};
use crate::geometry::Rect;
use crate::hwnd::Hwnd;
use crate::sys;

/// The vertical inset of a native slot without an explicit height, so a child
/// sits comfortably in the band.
pub(super) const NATIVE_V_PAD_DIP: f32 = 6.0;

/// A child control hosted in a native slot: its handle and the shared bounds
/// cache, so `ControlExt::bounds` stays correct after the bar moves it.
///
/// The handle is not owned; the app's control destroys it. A stale handle makes
/// the move a harmless no-op.
#[derive(Clone)]
pub(crate) struct Child {
    hwnd: Hwnd,
    bounds: Rc<Cell<Rect>>,
}

impl Child {
    /// Captures `control`'s handle and bounds cache.
    pub(super) fn of(control: &impl AsControl) -> Child {
        Child {
            hwnd: control.hwnd(),
            bounds: control.control().bounds_handle(),
        }
    }

    fn place(&self, rect: Rect) {
        if self.bounds.get() != rect {
            self.bounds.set(rect);
            sys::window::move_window(self.hwnd, rect);
        }
    }
}

/// The `(top, bottom)` of a native slot in the band `top_px..top_px + height_px`:
/// `height_dip` centred, or the band minus [`NATIVE_V_PAD_DIP`] on both sides.
/// The slot never exceeds the band.
pub(super) fn slot_span(
    top_px: i32,
    height_px: i32,
    scale: f32,
    height_dip: Option<f32>,
) -> (f32, f32) {
    let band = height_px as f32;
    let slot = match height_dip {
        Some(dip) => (dip * scale).min(band),
        None => (band - NATIVE_V_PAD_DIP * scale * 2.0).max(0.0),
    };
    let top = top_px as f32 + (band - slot) / 2.0;
    (top, top + slot)
}

/// Moves each native slot's child, if any, to its rectangle.
pub(super) fn place_children(items: &[Item], rects: &[Rect]) {
    for (item, rect) in items.iter().zip(rects) {
        if item.kind == Kind::Native
            && let Some(child) = &item.child
        {
            child.place(*rect);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::slot_span;

    #[test]
    fn a_slot_without_a_height_is_inset_from_the_band() {
        assert_eq!(slot_span(30, 40, 1.0, None), (36.0, 64.0));
    }

    #[test]
    fn an_explicit_height_is_centred_and_capped_at_the_band() {
        assert_eq!(slot_span(30, 40, 1.0, Some(22.0)), (39.0, 61.0));
        assert_eq!(slot_span(30, 40, 2.0, Some(30.0)), (30.0, 70.0));
    }
}
