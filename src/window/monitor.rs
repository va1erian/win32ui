#![forbid(unsafe_code)]

//! Monitor enumeration: what displays are attached, where they are, and which
//! one a window is on.

use crate::geometry::Rect;
use crate::hwnd::Hwnd;
use crate::sys;
use crate::window::Window;

/// One display monitor.
///
/// Plain data, so an app can list the monitors in a picker, place a fullscreen
/// window on one, or persist the chosen device name across runs. Use
/// [`monitors`] to enumerate them and [`Window::monitor`] for the one a window
/// is on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonitorInfo {
    /// The stable device name Windows reports, e.g. `\\.\DISPLAY2`.
    pub device_name: String,
    /// The monitor's friendly name from the display device, e.g.
    /// `"DELL U2720Q"`. Empty when Windows reports none.
    pub friendly_name: String,
    /// The full monitor rectangle, in virtual-screen coordinates.
    pub rect: Rect,
    /// The work area (the monitor minus the taskbar and any docked bars), in
    /// virtual-screen coordinates.
    pub work_area: Rect,
    /// Whether this is the primary monitor.
    pub primary: bool,
    /// The monitor's effective dots-per-inch.
    pub dpi: u32,
}

/// Every monitor attached to the desktop, in the order Windows reports them.
pub fn monitors() -> Vec<MonitorInfo> {
    sys::monitor::monitors()
}

/// The monitor that contains (or is nearest to) `hwnd`, or `None` when the
/// display query fails.
///
/// A window straddling two monitors reports the one Windows considers current
/// — the same resolution used for `WM_DPICHANGED`.
pub fn monitor_of(hwnd: Hwnd) -> Option<MonitorInfo> {
    sys::monitor::monitor_of(hwnd)
}

impl Window {
    /// The monitor the window is currently on, or `None` when the display query
    /// fails.
    pub fn monitor(&self) -> Option<MonitorInfo> {
        monitor_of(self.hwnd())
    }
}
