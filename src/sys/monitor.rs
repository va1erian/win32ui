//! Monitor enumeration and per-window monitor lookup.
//!
//! Wraps `EnumDisplayMonitors`/`GetMonitorInfo` plus the friendly-name
//! (`EnumDisplayDevicesW`) and DPI (`GetDpiForMonitor`) queries into the safe
//! [`MonitorInfo`](crate::MonitorInfo) the window layer exposes.

use core::mem::size_of;

use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    DISPLAY_DEVICEW, EnumDisplayDevicesW, EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR,
    MONITOR_DEFAULTTONEAREST, MONITORINFOEXW, MonitorFromWindow,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;
use windows::core::{BOOL, PCWSTR};

use crate::geometry::Rect;
use crate::hwnd::Hwnd;
use crate::window::MonitorInfo;

use super::raw_hwnd;

/// Every monitor attached to the desktop, in the order Windows reports them.
pub(crate) fn monitors() -> Vec<MonitorInfo> {
    unsafe extern "system" fn collect(
        monitor: HMONITOR,
        _dc: HDC,
        _rect: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        // SAFETY: `data` is the `&mut Vec<MonitorInfo>` passed to
        // `EnumDisplayMonitors` below, which outlives every callback it makes.
        let monitors = unsafe { &mut *(data.0 as *mut Vec<MonitorInfo>) };
        if let Some(info) = monitor_info(monitor) {
            monitors.push(info);
        }
        BOOL(1)
    }

    let mut monitors = Vec::new();
    // SAFETY: `monitors` outlives the call and the callback only appends to it.
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(collect),
            LPARAM(&mut monitors as *mut Vec<MonitorInfo> as isize),
        );
    }
    monitors
}

/// The monitor nearest to `hwnd`, with the same fields as [`monitors`].
pub(crate) fn monitor_of(hwnd: Hwnd) -> Option<MonitorInfo> {
    // SAFETY: `hwnd` is a live window handle; `MonitorFromWindow` only reads it.
    let monitor = unsafe { MonitorFromWindow(raw_hwnd(hwnd), MONITOR_DEFAULTTONEAREST) };
    if monitor.0.is_null() {
        None
    } else {
        monitor_info(monitor)
    }
}

/// Reads one monitor's info: device and friendly name, rectangles, primary
/// flag and DPI.
fn monitor_info(monitor: HMONITOR) -> Option<MonitorInfo> {
    let mut info = MONITORINFOEXW::default();
    // `MONITORINFOEXW`'s first field is a `MONITORINFO`; setting `cbSize` to
    // the larger size is what makes Windows fill the trailing device name.
    info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    // SAFETY: `&mut info.monitorInfo` points at a `MONITORINFO` with `cbSize`
    // set to the enclosing `MONITORINFOEXW`, as `GetMonitorInfoW` requires.
    if !unsafe { GetMonitorInfoW(monitor, &mut info.monitorInfo) }.as_bool() {
        return None;
    }
    let device_name = wide_to_string(&info.szDevice);
    Some(MonitorInfo {
        friendly_name: friendly_name(&device_name),
        device_name,
        rect: rect_of(info.monitorInfo.rcMonitor),
        work_area: rect_of(info.monitorInfo.rcWork),
        primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
        dpi: dpi_of(monitor),
    })
}

/// The monitor's friendly name (e.g. `"DELL U2720Q"`), or the empty string
/// when the device has none.
fn friendly_name(device_name: &str) -> String {
    let name: Vec<u16> = device_name.encode_utf16().chain([0]).collect();
    let mut device = DISPLAY_DEVICEW {
        cb: size_of::<DISPLAY_DEVICEW>() as u32,
        ..Default::default()
    };
    // SAFETY: `name` is a nul-terminated wide string; `device` is a valid
    // out-struct with `cb` set, and `EnumDisplayDevicesW` copies into it.
    if !unsafe { EnumDisplayDevicesW(PCWSTR(name.as_ptr()), 0, &mut device, 0) }.as_bool() {
        return String::new();
    }
    wide_to_string(&device.DeviceString)
}

/// The monitor's effective DPI, falling back to 96 when the query fails.
fn dpi_of(monitor: HMONITOR) -> u32 {
    let mut x = 96u32;
    let mut y = 96u32;
    // SAFETY: `GetDpiForMonitor` writes through the two out-pointers; on
    // failure the 96 defaults stand.
    if unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y) }.is_err() {
        return 96;
    }
    x
}

/// Decodes a fixed-size wide buffer up to its nul terminator.
fn wide_to_string(buffer: &[u16]) -> String {
    let end = buffer
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}

fn rect_of(rect: RECT) -> Rect {
    Rect::new(rect.left, rect.top, rect.right, rect.bottom)
}
