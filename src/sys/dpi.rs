//! DPI awareness and per-window DPI queries.

use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForSystem, GetDpiForWindow,
    SetProcessDpiAwarenessContext,
};

use crate::hwnd::Hwnd;

use super::raw_hwnd;

/// Opts the process into per-monitor-v2 DPI awareness. Failures are ignored:
/// the setting may already be active via the manifest, or unsupported.
pub(crate) fn set_per_monitor_v2() {
    // SAFETY: `SetProcessDpiAwarenessContext` takes a plain constant and only
    // affects the calling process.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

/// The dots-per-inch of the monitor a window is on (96 when unavailable).
pub(crate) fn window_dpi(hwnd: Hwnd) -> u32 {
    // SAFETY: `GetDpiForWindow` only reads the handle; it returns 0 on
    // failure, which is mapped to the 100% baseline.
    let dpi = unsafe { GetDpiForWindow(raw_hwnd(hwnd)) };
    if dpi == 0 { 96 } else { dpi }
}

/// The process-wide system DPI (96 when unavailable), used to size a window
/// before it exists.
pub(crate) fn system_dpi() -> u32 {
    // SAFETY: `GetDpiForSystem` takes no arguments; 0 means "unavailable" and
    // is mapped to the 100% baseline.
    let dpi = unsafe { GetDpiForSystem() };
    if dpi == 0 { 96 } else { dpi }
}
