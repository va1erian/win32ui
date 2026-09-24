//! The subclass on the tab control: themed background, hover and `Ctrl+Tab`.

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_SHIFT};
use windows::Win32::UI::Shell::DefSubclassProc;
use windows::Win32::UI::WindowsAndMessaging::{
    DLGC_WANTARROWS, DLGC_WANTTAB, WM_ERASEBKGND, WM_GETDLGCODE, WM_KEYDOWN, WM_LBUTTONDOWN,
    WM_MOUSEMOVE, WM_PAINT, WM_SIZE,
};

use crate::geometry::Rect;
use crate::hwnd::Hwnd;

/// An event a [`TabHost`] forwards from the tab control's window procedure.
pub(crate) enum TabEvent {
    /// `WM_ERASEBKGND`: paint the strip background into the raw DC.
    Erase {
        /// The raw `HDC`.
        dc: isize,
        /// The control's client rectangle.
        bounds: Rect,
    },
    /// `WM_MOUSEMOVE`.
    MouseMove {
        /// Client x.
        x: i32,
        /// Client y.
        y: i32,
    },
    /// `WM_MOUSELEAVE`.
    MouseLeave,
    /// After the control painted itself: paint over the parts of the native
    /// chrome (its frame) that ignore dark mode.
    Chrome {
        /// The raw `HDC`, valid for the call.
        dc: isize,
        /// The control's client rectangle.
        bounds: Rect,
    },
    /// `WM_SIZE`, after the control re-laid out its tabs and (possibly)
    /// created or removed its overflow scroller.
    Resized,
    /// `WM_KEYDOWN`, with the modifier state at the time.
    Key {
        /// The virtual key.
        key: u32,
        /// Ctrl held.
        ctrl: bool,
        /// Shift held.
        shift: bool,
    },
}

/// A callback invoked for a forwarded [`TabEvent`]; a `Some` result overrides
/// the default window procedure's return value.
type TabHandler = Box<dyn Fn(TabEvent) -> Option<isize>>;

struct TabRefdata {
    handler: TabHandler,
}

/// A subclass on the tab control that forwards hover, background and key
/// events to a closure. Dropping it removes the subclass and frees the closure.
pub(crate) struct TabHost {
    child: Hwnd,
    raw: *mut TabRefdata,
}

impl TabHost {
    /// Installs the subclass on `child`. Returns `None` if subclassing fails.
    pub(crate) fn install(child: Hwnd, handler: TabHandler) -> Option<TabHost> {
        let raw = Box::into_raw(Box::new(TabRefdata { handler }));
        if !crate::sys::window::set_subclass(child, Some(tab_proc), TAB_SUBCLASS_ID, raw as usize) {
            // SAFETY: install failed before the subclass could adopt it.
            unsafe { drop(Box::from_raw(raw)) };
            return None;
        }
        Some(TabHost { child, raw })
    }
}

impl Drop for TabHost {
    fn drop(&mut self) {
        crate::sys::window::remove_subclass(self.child, Some(tab_proc), TAB_SUBCLASS_ID);
        // The control is not created through the crate's window class, so its
        // off-screen paint buffer is not released on `WM_NCDESTROY`.
        crate::sys::gdi::release_back_buffer(self.child);
        // SAFETY: allocated in `install` and reclaimed exactly once.
        unsafe { drop(Box::from_raw(self.raw)) };
    }
}

const TAB_SUBCLASS_ID: usize = 0x7461_6273; // "tabs"

/// Calls the installed handler, swallowing a panic so it cannot unwind across
/// the `extern "system"` boundary.
///
/// # Safety
/// `refdata` must be the live `TabRefdata` installed by [`TabHost::install`].
unsafe fn forward(refdata: usize, event: TabEvent) -> Option<isize> {
    let data = unsafe { &*(refdata as *const TabRefdata) };
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (data.handler)(event)))
        .ok()
        .flatten()
}

/// The subclass procedure behind [`TabHost`].
///
/// # Safety
/// Called by Windows for a subclass installed by [`TabHost::install`];
/// `refdata` is the live `TabRefdata` pointer.
unsafe extern "system" fn tab_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    refdata: usize,
) -> LRESULT {
    // Clicking a tab should focus the strip so the arrow keys and `Ctrl+Tab`
    // work; a plain child of a non-dialog window does not always take focus
    // from a click, so set it explicitly.
    if msg == WM_LBUTTONDOWN {
        crate::sys::window::set_focus(crate::sys::hwnd_from(hwnd));
    }
    // The app window runs `IsDialogMessage`, which would otherwise claim Tab
    // and the arrow keys for focus navigation and starve the tab control. Tell
    // it the control wants them.
    if msg == WM_GETDLGCODE {
        return LRESULT((DLGC_WANTARROWS | DLGC_WANTTAB) as isize);
    }
    let bounds = crate::sys::window::client_rect(crate::sys::hwnd_from(hwnd));
    if msg == WM_PAINT {
        // The control draws the tabs (through `WM_DRAWITEM`) and its frame into
        // an off-screen buffer, then the widget repaints the frame there from
        // theme tokens because the native one ignores dark mode.
        // SAFETY: called from the subclass procedure while handling `WM_PAINT`;
        // `refdata` is the live `TabRefdata`.
        return unsafe {
            super::buffered::paint(hwnd, bounds, |dc| {
                let _ = forward(refdata, TabEvent::Chrome { dc, bounds });
            })
        };
    }
    // The off-screen pass erases its own buffer; erasing the screen first would
    // blank the strip before the finished frame is blitted.
    if msg == WM_ERASEBKGND && !super::buffered::printing() {
        return LRESULT(1);
    }
    if msg == WM_SIZE {
        // SAFETY: forward to the subclass chain's original window procedure.
        let result = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
        // SAFETY: `refdata` is the live `TabRefdata`.
        let _ = unsafe { forward(refdata, TabEvent::Resized) };
        return result;
    }
    let event = match msg {
        WM_ERASEBKGND => TabEvent::Erase {
            dc: wparam.0 as isize,
            bounds,
        },
        WM_MOUSEMOVE => TabEvent::MouseMove {
            x: (lparam.0 & 0xffff) as i16 as i32,
            y: ((lparam.0 >> 16) & 0xffff) as i16 as i32,
        },
        WM_MOUSELEAVE => TabEvent::MouseLeave,
        WM_KEYDOWN => {
            // SAFETY: `GetKeyState` only reads the keyboard state for the
            // calling thread; the key codes are documented constants.
            let (ctrl, shift) = unsafe {
                (
                    GetKeyState(VK_CONTROL.0 as i32) < 0,
                    GetKeyState(VK_SHIFT.0 as i32) < 0,
                )
            };
            TabEvent::Key {
                key: (wparam.0 & 0xffff) as u32,
                ctrl,
                shift,
            }
        }
        _ => {
            // SAFETY: forward to the subclass chain's original window procedure.
            return unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
        }
    };
    match unsafe { forward(refdata, event) } {
        Some(value) => LRESULT(value),
        // SAFETY: forward to the subclass chain's original window procedure.
        None => unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) },
    }
}
