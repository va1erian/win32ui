//! Toggling the sunken client edge (`WS_EX_CLIENTEDGE`) of an existing control.
//!
//! The native edge is a light 3D bevel that ignores dark mode, so a dark theme
//! turns it off and a light one turns it back on.
//!
//! All `unsafe` in this crate lives under `sys`; every block below carries a
//! `// SAFETY:` note.

use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetWindowLongPtrW, SWP_FRAMECHANGED, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    SetWindowLongPtrW, SetWindowPos, WS_EX_CLIENTEDGE,
};

use crate::hwnd::Hwnd;

use super::raw_hwnd;

/// Shows or hides the client edge of `hwnd` and recomputes its frame.
pub(crate) fn set(hwnd: Hwnd, on: bool) {
    let raw = raw_hwnd(hwnd);
    // SAFETY: `GWL_EXSTYLE` only reads the window's own extended style bits.
    let current = unsafe { GetWindowLongPtrW(raw, GWL_EXSTYLE) } as u32;
    let next = if on {
        current | WS_EX_CLIENTEDGE.0
    } else {
        current & !WS_EX_CLIENTEDGE.0
    };
    if next == current {
        return;
    }
    // SAFETY: writes the window's own extended style; a stale handle is a
    // documented no-op. `SWP_FRAMECHANGED` recomputes only the frame, the other
    // flags keep position, size and z-order.
    unsafe {
        SetWindowLongPtrW(raw, GWL_EXSTYLE, next as isize);
        let _ = SetWindowPos(
            raw,
            None,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
        );
    }
}

#[cfg(test)]
mod tests {
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, WINDOW_STYLE, WS_POPUP,
    };
    use windows::core::w;

    use super::*;

    fn edge_of(hwnd: Hwnd) -> bool {
        // SAFETY: reads the window's own extended style bits.
        let style = unsafe { GetWindowLongPtrW(raw_hwnd(hwnd), GWL_EXSTYLE) } as u32;
        style & WS_EX_CLIENTEDGE.0 != 0
    }

    /// The edge follows the flag in both directions and a repeat is a no-op.
    #[test]
    fn edge_can_be_turned_off_and_on() {
        // SAFETY: a hidden, parentless `STATIC` window, destroyed below.
        let raw = unsafe {
            CreateWindowExW(
                WS_EX_CLIENTEDGE,
                w!("STATIC"),
                w!(""),
                WINDOW_STYLE(WS_POPUP.0),
                0,
                0,
                10,
                10,
                None,
                None,
                None,
                None,
            )
        }
        .expect("window");
        let hwnd = super::super::hwnd_from(raw);
        assert!(edge_of(hwnd));
        set(hwnd, false);
        assert!(!edge_of(hwnd), "the edge was not removed");
        set(hwnd, false);
        assert!(!edge_of(hwnd));
        set(hwnd, true);
        assert!(edge_of(hwnd), "the edge was not restored");
        // SAFETY: `raw` was created above and is destroyed once.
        unsafe {
            let _ = DestroyWindow(raw);
        }
    }
}
