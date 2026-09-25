//! Borderless fullscreen: restyle a top-level window into a `WS_POPUP` that
//! covers one monitor's full rectangle, topmost and without a DWM frame.
//!
//! The window's original style, placement and extended-title-bar flag are saved
//! on entry and restored on leave, so the window comes back exactly as it was.

use core::cell::RefCell;
use core::mem::size_of;
use std::collections::HashMap;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, GetWindowPlacement, HWND_NOTOPMOST, HWND_TOPMOST, SET_WINDOW_POS_FLAGS,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_SHOWWINDOW,
    SetWindowLongPtrW, SetWindowPlacement, SetWindowPos, WINDOWPLACEMENT, WS_CLIPCHILDREN,
    WS_CLIPSIBLINGS, WS_POPUP, WS_VISIBLE,
};

use crate::error::Result;
use crate::geometry::Rect;
use crate::hwnd::Hwnd;

use super::{raw_hwnd, win32_error};

/// What a window looked like before it entered fullscreen.
struct Saved {
    style: isize,
    placement: WINDOWPLACEMENT,
    /// Whether the extended title bar was active, so it can be restored.
    extended: bool,
}

thread_local! {
    /// The saved state of every window currently in fullscreen, keyed by
    /// `HWND`. Windows are thread-affine, so the owning thread's map is the one
    /// that serves them.
    static FULLSCREEN: RefCell<HashMap<isize, Saved>> = RefCell::new(HashMap::new());
}

/// Whether `hwnd` is currently in fullscreen.
pub(crate) fn is_fullscreen(hwnd: Hwnd) -> bool {
    FULLSCREEN.with(|map| map.borrow().contains_key(&(hwnd.raw() as isize)))
}

/// Enters fullscreen on `rect` (a monitor's full rectangle, in screen
/// coordinates). Idempotent: a second call just moves the window.
pub(crate) fn enter(hwnd: Hwnd, rect: Rect) -> Result<()> {
    let key = hwnd.raw() as isize;
    let already = FULLSCREEN.with(|map| map.borrow().contains_key(&key));
    if !already {
        let mut placement = WINDOWPLACEMENT {
            length: size_of::<WINDOWPLACEMENT>() as u32,
            ..Default::default()
        };
        // SAFETY: `placement` is a valid out-pointer whose `length` describes
        // its size, as `GetWindowPlacement` requires.
        let _ = unsafe { GetWindowPlacement(raw_hwnd(hwnd), &mut placement) };
        let saved = Saved {
            // SAFETY: `hwnd` is a live window; reading its style is safe.
            style: unsafe {
                GetWindowLongPtrW(
                    raw_hwnd(hwnd),
                    windows::Win32::UI::WindowsAndMessaging::GWL_STYLE,
                )
            },
            placement,
            extended: crate::window::nc::is_extended(hwnd),
        };
        FULLSCREEN.with(|map| map.borrow_mut().insert(key, saved));
    }

    // A borderless popup: no caption, frame, menu, minimize/maximize or system
    // menu, clipped so child windows do not flicker while the frame changes.
    let popup = (WS_POPUP.0 | WS_VISIBLE.0 | WS_CLIPCHILDREN.0 | WS_CLIPSIBLINGS.0) as isize;
    // SAFETY: `hwnd` is a live top-level window; the new style is a plain bit
    // set and `SetWindowPos` receives the monitor rectangle.
    unsafe {
        SetWindowLongPtrW(
            raw_hwnd(hwnd),
            windows::Win32::UI::WindowsAndMessaging::GWL_STYLE,
            popup,
        );
        set_pos(hwnd, HWND_TOPMOST, rect, SWP_FRAMECHANGED | SWP_SHOWWINDOW)?;
    }
    // The extended title strip would draw a caption the popup no longer has.
    crate::window::nc::set_extended(hwnd, false);
    super::dwm::disable_rounding(hwnd);
    Ok(())
}

/// Leaves fullscreen, restoring the saved style, topmost state and placement.
/// A no-op for a window that is not in fullscreen.
pub(crate) fn leave(hwnd: Hwnd) -> Result<()> {
    let key = hwnd.raw() as isize;
    let saved = FULLSCREEN.with(|map| map.borrow_mut().remove(&key));
    let Some(saved) = saved else {
        return Ok(());
    };
    let restore = SWP_NOMOVE | SWP_NOSIZE | SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOOWNERZORDER;
    // SAFETY: `hwnd` is live; `saved.style` is the exact style read on entry and
    // `saved.placement` a `WINDOWPLACEMENT` filled by `GetWindowPlacement`.
    unsafe {
        SetWindowLongPtrW(
            raw_hwnd(hwnd),
            windows::Win32::UI::WindowsAndMessaging::GWL_STYLE,
            saved.style,
        );
        let _ = SetWindowPos(raw_hwnd(hwnd), Some(HWND_NOTOPMOST), 0, 0, 0, 0, restore);
        let _ = SetWindowPlacement(raw_hwnd(hwnd), &saved.placement);
    }
    crate::window::nc::set_extended(hwnd, saved.extended);
    Ok(())
}

/// Forgets a destroyed window's fullscreen state.
pub(crate) fn forget(hwnd: Hwnd) {
    FULLSCREEN.with(|map| {
        map.borrow_mut().remove(&(hwnd.raw() as isize));
    });
}

fn set_pos(hwnd: Hwnd, insert_after: HWND, rect: Rect, flags: SET_WINDOW_POS_FLAGS) -> Result<()> {
    // SAFETY: `hwnd` is live and `rect` describes the target frame.
    unsafe {
        SetWindowPos(
            raw_hwnd(hwnd),
            Some(insert_after),
            rect.left,
            rect.top,
            rect.width(),
            rect.height(),
            flags,
        )
    }
    .map_err(win32_error)
}
