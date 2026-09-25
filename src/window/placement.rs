#![forbid(unsafe_code)]

//! Window placement: the normal bounds and show state an app can persist, plus
//! monitor work areas and a helper to bring an off-screen placement back.

use crate::error::Result;
use crate::geometry::{Point, Rect, Size};
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

/// A `size` rectangle centred on the work area of the monitor that contains
/// `anchor`, shrunk to fit and falling back to the nearest monitor.
///
/// Used as the default placement for a new window: the anchor is the owner
/// window's centre (so a dialog opens on the monitor its opener is on) or the
/// primary monitor's origin for a standalone window, instead of the primary
/// monitor's top-left corner. With no monitors reported, the rectangle keeps
/// the origin as its top-left.
pub fn centered_in_work_area(size: Size, anchor: Point) -> Rect {
    centered_in(size, anchor, &monitor_work_areas())
}

/// The pure half of [`centered_in_work_area`], over an explicit work-area list.
fn centered_in(size: Size, anchor: Point, areas: &[Rect]) -> Rect {
    let found = areas.iter().copied().find(|area| area.contains(anchor));
    let Some(area) = found.or_else(|| {
        areas
            .iter()
            .copied()
            .min_by_key(|area| distance_squared_to_point(*area, anchor))
    }) else {
        return Rect::from_size(size);
    };
    let width = size.width.min(area.width());
    let height = size.height.min(area.height());
    let left = area.left + (area.width() - width) / 2;
    let top = area.top + (area.height() - height) / 2;
    Rect::new(left, top, left + width, top + height)
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

/// Squared distance between a rectangle's centre and `point`, for choosing the
/// nearest monitor.
fn distance_squared_to_point(a: Rect, point: Point) -> i64 {
    let dx = ((a.left + a.right) as i64 - 2 * point.x as i64) / 2;
    let dy = ((a.top + a.bottom) as i64 - 2 * point.y as i64) / 2;
    dx * dx + dy * dy
}

/// Squared distance between the centres of two rectangles, for choosing the
/// nearest monitor.
fn distance_squared(a: Rect, b: Rect) -> i64 {
    let dx = ((a.left + a.right) - (b.left + b.right)) as i64 / 2;
    let dy = ((a.top + a.bottom) - (b.top + b.bottom)) as i64 / 2;
    dx * dx + dy * dy
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitors() -> [Rect; 2] {
        [Rect::new(0, 0, 1920, 1040), Rect::new(1920, 0, 3840, 1040)]
    }

    #[test]
    fn centres_on_the_monitor_containing_the_anchor() {
        let bounds = centered_in(Size::new(800, 600), Point::new(2500, 500), &monitors());
        assert_eq!(bounds, Rect::new(2480, 220, 3280, 820));
    }

    #[test]
    fn a_standalone_window_centres_on_the_primary_monitor() {
        let bounds = centered_in(Size::new(1000, 700), Point::new(0, 0), &monitors());
        assert_eq!(bounds, Rect::new(460, 170, 1460, 870));
    }

    #[test]
    fn an_anchor_off_every_monitor_falls_back_to_the_nearest() {
        let bounds = centered_in(Size::new(200, 200), Point::new(4000, 500), &monitors());
        assert_eq!(bounds.left, 1920 + (1920 - 200) / 2);
        assert_eq!(bounds.top, (1040 - 200) / 2);
    }

    #[test]
    fn oversized_windows_are_shrunk_to_the_work_area() {
        let bounds = centered_in(Size::new(4000, 3000), Point::new(100, 100), &monitors());
        assert_eq!(bounds, monitors()[0]);
    }

    #[test]
    fn no_monitors_keeps_the_origin() {
        let bounds = centered_in(Size::new(640, 480), Point::new(0, 0), &[]);
        assert_eq!(bounds, Rect::new(0, 0, 640, 480));
    }
}
