//! The one-pixel line the system draws under a menu bar.
//!
//! An owner-drawn dark bar (`MIM_BACKGROUND` plus themed items) still gets a
//! light line along its bottom edge from the system menu frame. Only the
//! undocumented `WM_UAHDRAWMENU` would replace it, so instead the line is
//! painted over with the bar's colour after every `WM_NCPAINT` and
//! `WM_NCACTIVATE`, using `GetMenuBarInfo` and the window DC — documented APIs
//! only.
//!
//! All `unsafe` in this crate lives under `sys`; every block below carries a
//! `// SAFETY:` note.

use std::cell::RefCell;
use std::collections::HashMap;

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{ClientToScreen, GetWindowDC, ReleaseDC};
use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, WM_NCACTIVATE, WM_NCPAINT};

use crate::color::Color;
use crate::geometry::Rect;
use crate::hwnd::Hwnd;

use super::{hwnd_from, raw_hwnd};

thread_local! {
    /// The bar colour to paint the seam with, per window with a dark bar.
    static SEAMS: RefCell<HashMap<isize, Color>> = RefCell::new(HashMap::new());
}

/// Sets the colour the seam under `hwnd`'s menu bar is painted with, or `None`
/// to leave the system's line (a light bar).
pub(crate) fn set(hwnd: Hwnd, color: Option<Color>) {
    SEAMS.with(|map| {
        let mut map = map.borrow_mut();
        match color {
            Some(color) => map.insert(hwnd.raw() as isize, color),
            None => map.remove(&(hwnd.raw() as isize)),
        };
    });
    paint(raw_hwnd(hwnd));
}

/// Drops `hwnd`'s seam colour when the window is destroyed.
pub(crate) fn forget(hwnd: Hwnd) {
    SEAMS.with(|map| map.borrow_mut().remove(&(hwnd.raw() as isize)));
}

/// Repaints the seam after the system has drawn the non-client area.
pub(crate) fn after_message(hwnd: HWND, msg: u32) {
    if msg == WM_NCPAINT || msg == WM_NCACTIVATE {
        paint(hwnd);
    }
}

/// Paints the row just above the client area, across the bar's width.
fn paint(hwnd: HWND) {
    let window = hwnd_from(hwnd);
    let Some(color) = SEAMS.with(|map| map.borrow().get(&(window.raw() as isize)).copied()) else {
        return;
    };
    // The extended title bar draws its own menu (or none) inside the client.
    if crate::window::nc::is_extended(window) {
        return;
    }
    let Some(bar) = super::nc::menu_bar_rect(window) else {
        return;
    };
    let Some(brush) = crate::gdi::cache_brush(color) else {
        return;
    };
    let mut frame = RECT::default();
    let mut client = POINT::default();
    // SAFETY: `hwnd` is live; both out-parameters are valid for the call.
    let placed = unsafe {
        GetWindowRect(hwnd, &mut frame).is_ok() && ClientToScreen(hwnd, &mut client).as_bool()
    };
    if !placed {
        return;
    }
    let bottom = client.y - frame.top;
    let seam = Rect::new(
        bar.left - frame.left,
        bottom - 1,
        bar.right - frame.left,
        bottom,
    );
    // SAFETY: `GetWindowDC`/`ReleaseDC` are the documented pair for the
    // non-client device context of a live window.
    unsafe {
        let dc = GetWindowDC(Some(hwnd));
        if dc.0.is_null() {
            return;
        }
        super::gdi::fill_rect(dc, seam, brush);
        ReleaseDC(Some(hwnd), dc);
    }
}
