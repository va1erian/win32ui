//! Theming for the tab control's overflow scroller.
//!
//! When the tabs do not fit, the control creates a child `msctls_updown32`
//! (the small arrow pair). It is not owner-drawn, so it is darkened through
//! `SetWindowTheme` like the other native parts.

use windows::Win32::UI::WindowsAndMessaging::FindWindowExW;
use windows::core::w;

use crate::hwnd::Hwnd;
use crate::sys::{NativeControlKind, apply_native_theme, hwnd_from, raw_hwnd};

/// Applies the light or dark visual style to the scroller of `tabs`, if the
/// control currently has one. The control creates it lazily, so callers
/// re-apply after a resize.
pub(crate) fn apply_scroller_theme(tabs: Hwnd, is_dark: bool) {
    // SAFETY: `tabs` is a live tab control; `FindWindowExW` only enumerates its
    // children and the class name is a static literal.
    let found = unsafe { FindWindowExW(Some(raw_hwnd(tabs)), None, w!("msctls_updown32"), None) };
    if let Ok(updown) = found {
        apply_native_theme(hwnd_from(updown), NativeControlKind::Scrollable, is_dark);
    }
}
