//! Opaque painting for a native child control that sits on the DWM material.
//!
//! GDI leaves the alpha byte of every pixel it draws at zero. Inside the
//! window's extended frame DWM honours per-pixel alpha, so an ordinary child
//! control there is dropped and the material shows instead. This stateless
//! subclass routes the child's `WM_PAINT` and `WM_NCPAINT` through the opaque
//! buffered painting of [`glass_paint`](super::glass_paint).
//!
//! Controls such as `Edit` also draw directly, outside `WM_PAINT`, while the
//! user types, selects or focuses them; after those messages the child is
//! redrawn synchronously, so the zero-alpha pixels are replaced before the
//! message loop (and DWM) sees them.
//!
//! Outside the extended frame the result looks the same as ordinary painting,
//! so the subclass stays installed until the child is destroyed.
//!
//! All `unsafe` in this crate lives under `sys`; every block below carries a
//! `// SAFETY:` note.

use std::cell::RefCell;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, PAINTSTRUCT, RDW_FRAME, RDW_INVALIDATE, RDW_NOERASE, RDW_UPDATENOW,
    RedrawWindow,
};
use windows::Win32::UI::Controls::{
    BufferedPaintInit, BufferedPaintUnInit, EM_REPLACESEL, EM_SCROLLCARET, EM_SETSEL, EM_UNDO,
};
use windows::Win32::UI::Input::KeyboardAndMouse::GetCapture;
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    CWP_SKIPINVISIBLE, ChildWindowFromPointEx, WM_CHAR, WM_CLEAR, WM_CUT, WM_IME_COMPOSITION,
    WM_IME_ENDCOMPOSITION, WM_IME_STARTCOMPOSITION, WM_KEYDOWN, WM_KEYUP, WM_KILLFOCUS,
    WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCDESTROY, WM_NCPAINT,
    WM_PAINT, WM_PASTE, WM_SETFOCUS, WM_SETTEXT, WM_TIMER, WM_UNDO,
};

use crate::geometry::Point;
use crate::hwnd::Hwnd;

use super::glass_paint::{paint_client, paint_frame, print_client};
use super::raw_hwnd;

const OPAQUE_SUBCLASS_ID: usize = 0x6f70_6171; // "opaq"

thread_local! {
    /// The children of this thread carrying the subclass. `GetWindowSubclass`
    /// would answer the same question, but comctl32 v5 (a binary without the
    /// v6 manifest) only exports it by ordinal.
    static SUBCLASSED: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

/// Whether `child` carries the subclass.
fn is_subclassed(child: HWND) -> bool {
    SUBCLASSED.with(|list| list.borrow().contains(&(child.0 as usize)))
}

/// Finds the visible child of `parent` at `point` (client
/// coordinates), makes it paint opaquely over the material and marks it for a
/// repaint, so it is drawn back on top after the parent's frame was presented.
/// No-op when no child is there. Allocates nothing once the child is
/// subclassed.
pub(crate) fn repaint_opaque_child_at(parent: Hwnd, point: Point) {
    let parent = raw_hwnd(parent);
    let raw = POINT {
        x: point.x,
        y: point.y,
    };
    // A disabled control still paints, so it is not skipped.
    // SAFETY: `parent` is live and `raw` is a client point; the call only reads.
    let child = unsafe { ChildWindowFromPointEx(parent, raw, CWP_SKIPINVISIBLE) };
    if child.0.is_null() || child == parent {
        return;
    }
    let flags = if install(child) {
        // Newly subclassed: its frame was painted with zero alpha too.
        RDW_INVALIDATE | RDW_NOERASE | RDW_FRAME
    } else {
        RDW_INVALIDATE | RDW_NOERASE
    };
    // SAFETY: `child` is a live child window; no rectangle or region means its
    // whole area, and only documented redraw flags are passed.
    unsafe {
        let _ = RedrawWindow(Some(child), None, None, flags);
    }
}

/// Subclasses `child` for opaque painting unless it already is. Returns whether
/// the subclass was newly installed.
fn install(child: HWND) -> bool {
    if is_subclassed(child) {
        return false;
    }
    // SAFETY: buffered painting is initialised once per installed subclass and
    // balanced by `BufferedPaintUnInit` in `WM_NCDESTROY`, on the child's
    // (this) thread.
    if unsafe { BufferedPaintInit() }.is_err() {
        return false;
    }
    // SAFETY: `opaque_proc` keeps no state (`refdata` is unused) and removes
    // itself on `WM_NCDESTROY`, so nothing outlives the window.
    let installed =
        unsafe { SetWindowSubclass(child, Some(opaque_proc), OPAQUE_SUBCLASS_ID, 0) }.as_bool();
    if installed {
        SUBCLASSED.with(|list| list.borrow_mut().push(child.0 as usize));
    } else {
        // SAFETY: balances the `BufferedPaintInit` above.
        let _ = unsafe { BufferedPaintUnInit() };
    }
    installed
}

/// Whether `msg` can make a control draw directly, outside `WM_PAINT`.
fn draws_directly(hwnd: HWND, msg: u32) -> bool {
    match msg {
        WM_KEYDOWN
        | WM_KEYUP
        | WM_CHAR
        | WM_LBUTTONDOWN
        | WM_LBUTTONUP
        | WM_LBUTTONDBLCLK
        | WM_SETFOCUS
        | WM_KILLFOCUS
        | WM_TIMER
        | WM_SETTEXT
        | WM_CUT
        | WM_PASTE
        | WM_CLEAR
        | WM_UNDO
        | WM_IME_STARTCOMPOSITION
        | WM_IME_COMPOSITION
        | WM_IME_ENDCOMPOSITION
        | EM_SETSEL
        | EM_REPLACESEL
        | EM_UNDO
        | EM_SCROLLCARET => true,
        // A drag-select extends the selection on each move while captured.
        // SAFETY: `GetCapture` only reads the thread's capture window.
        WM_MOUSEMOVE => (unsafe { GetCapture() }) == hwnd,
        _ => false,
    }
}

/// Delivers a paint message to the control below this subclass.
///
/// # Safety
/// Only called from inside `opaque_proc`, for its `hwnd`.
unsafe fn forward(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: the caller is the subclass procedure of `hwnd`.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

/// The subclass procedure installed by [`install`].
///
/// # Safety
/// Called by Windows for the subclass installed by `install`; it keeps no
/// state and removes itself on `WM_NCDESTROY`.
unsafe extern "system" fn opaque_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _refdata: usize,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            // SAFETY: `hwnd` is the window being painted; `ps` receives the
            // paint DC, which `EndPaint` releases.
            let dc = unsafe { BeginPaint(hwnd, &mut ps) };
            if !dc.is_invalid() && !paint_client(hwnd, dc, &ps.rcPaint, forward) {
                print_client(hwnd, dc, forward);
            }
            // SAFETY: ends the paint begun above.
            let _ = unsafe { EndPaint(hwnd, &ps) };
            LRESULT(0)
        }
        WM_NCPAINT if paint_frame(hwnd, forward) => LRESULT(0),
        WM_NCDESTROY => {
            SUBCLASSED.with(|list| list.borrow_mut().retain(|&raw| raw != hwnd.0 as usize));
            // SAFETY: removes this stateless subclass and balances the
            // `BufferedPaintInit` from `install`, then forwards the message.
            unsafe {
                let _ = RemoveWindowSubclass(hwnd, Some(opaque_proc), OPAQUE_SUBCLASS_ID);
                let _ = BufferedPaintUnInit();
                DefSubclassProc(hwnd, msg, wparam, lparam)
            }
        }
        _ => {
            // SAFETY: forward to the subclass chain's original window procedure.
            let result = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
            if draws_directly(hwnd, msg) {
                // SAFETY: `hwnd` is live; its client area is repainted now,
                // through the opaque `WM_PAINT` above.
                unsafe {
                    let _ = RedrawWindow(
                        Some(hwnd),
                        None,
                        None,
                        RDW_INVALIDATE | RDW_NOERASE | RDW_UPDATENOW,
                    );
                }
            }
            result
        }
    }
}

#[cfg(test)]
mod tests {
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, IsWindow, WINDOW_EX_STYLE, WINDOW_STYLE, WS_CHILD,
        WS_POPUP, WS_VISIBLE,
    };
    use windows::core::w;

    use super::*;

    /// The child under a point is subclassed once, and a point with no child
    /// does nothing.
    #[test]
    fn the_child_at_a_point_is_subclassed_once() {
        // SAFETY: a hidden popup parent with an `EDIT` child, both destroyed
        // below.
        let (parent, edit) = unsafe {
            let parent = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!(""),
                WINDOW_STYLE(WS_POPUP.0),
                -32000,
                -32000,
                200,
                40,
                None,
                None,
                None,
                None,
            )
            .expect("parent");
            let edit = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("EDIT"),
                w!("Hello glass"),
                WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0),
                0,
                0,
                120,
                24,
                Some(parent),
                None,
                None,
                None,
            )
            .expect("edit");
            (parent, edit)
        };
        let parent_hwnd = super::super::hwnd_from(parent);

        repaint_opaque_child_at(parent_hwnd, Point::new(10, 10));
        assert!(is_subclassed(edit), "the edit was not subclassed");
        assert!(!install(edit), "a second install must be a no-op");
        repaint_opaque_child_at(parent_hwnd, Point::new(150, 30));
        assert!(!is_subclassed(parent), "no child there: nothing to do");

        // SAFETY: both windows were created above and are destroyed once; the
        // subclass removes itself (and uninitialises buffered paint) on the
        // edit's `WM_NCDESTROY`.
        unsafe {
            let _ = DestroyWindow(parent);
            assert!(!IsWindow(Some(edit)).as_bool());
        }
        assert!(!is_subclassed(edit), "the destroyed edit is forgotten");
    }
}
