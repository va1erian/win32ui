#![forbid(unsafe_code)]

//! Window placement: the normal bounds and show state an app can persist, plus
//! monitor work areas and a helper to bring an off-screen placement back.

use crate::error::Result;
use crate::geometry::Rect;
use crate::sys;
use crate::window::Window;

/// How a window is shown, as persisted in a [`Placement`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShowState {
    /// Restored to its normal bounds.
    #[default]
    Normal,
    /// Minimized to the taskbar.
    Minimized,
    /// Maximized to the monitor's work area.
    Maximized,
}

/// A window's restorable position and show state.
///
/// Plain data with public fields, so an application can write it to a config
/// file and read it back; use [`Window::placement`] and
/// [`Window::set_placement`] to talk to the window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Placement {
    /// The restored (neither minimized nor maximized) outer rectangle, in
    /// screen coordinates.
    pub normal: Rect,
    /// The show state.
    pub show: ShowState,
}

impl Placement {
    /// Returns a placement that is visible on a monitor.
    ///
    /// If [`normal`](Placement::normal) already overlaps a monitor's work area
    /// the placement is returned unchanged. Otherwise it is moved onto the work
    /// area nearest its centre and shrunk to fit, so a window last used on a
    /// monitor that has since been unplugged still opens on screen.
    pub fn clamp_to_work_areas(self) -> Placement {
        let areas = monitor_work_areas();
        if areas.is_empty() || areas.iter().any(|area| overlaps(*area, self.normal)) {
            return self;
        }
        let Some(target) = areas
            .iter()
            .copied()
            .min_by_key(|area| distance_squared(*area, self.normal))
        else {
            return self;
        };
        let width = self.normal.width().min(target.width());
        let height = self.normal.height().min(target.height());
        let left = self.normal.left.clamp(target.left, target.right - width);
        let top = self.normal.top.clamp(target.top, target.bottom - height);
        Placement {
            normal: Rect::new(left, top, left + width, top + height),
            ..self
        }
    }
}

/// The work area (the monitor's screen minus the taskbar and any docked bars)
/// of every monitor, in virtual-screen coordinates.
pub fn monitor_work_areas() -> Vec<Rect> {
    sys::window_ext::monitor_work_areas()
}

impl Window {
    /// The window's current placement, or the default when it is gone.
    pub fn placement(&self) -> Placement {
        sys::window_ext::get_placement(self.hwnd()).unwrap_or_default()
    }

    /// Restores the window to `placement`: its normal bounds and show state.
    pub fn set_placement(&self, placement: &Placement) -> Result<()> {
        sys::window_ext::set_placement(self.hwnd(), placement)
    }
}

/// Whether the two rectangles share any area.
fn overlaps(a: Rect, b: Rect) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}

/// Squared distance between the centres of two rectangles, for choosing the
/// nearest monitor.
fn distance_squared(a: Rect, b: Rect) -> i64 {
    let dx = ((a.left + a.right) - (b.left + b.right)) as i64 / 2;
    let dy = ((a.top + a.bottom) - (b.top + b.bottom)) as i64 / 2;
    dx * dx + dy * dy
}
