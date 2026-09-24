//! Window classes, creation, and per-window operations.

use core::cell::Cell;
use core::ffi::c_void;

use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    GetUpdateRect, HBRUSH, InvalidateRect, RDW_ALLCHILDREN, RDW_ERASE, RDW_INVALIDATE,
    RedrawWindow, UpdateWindow, ValidateRect,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows::Win32::UI::Shell::SUBCLASSPROC;
use windows::Win32::UI::WindowsAndMessaging::{
    CS_DBLCLKS, CreateWindowExW, DestroyWindow, GWL_STYLE, GetClientRect, GetWindowLongPtrW,
    GetWindowRect, HCURSOR, HMENU, HWND_BOTTOM, IDC_ARROW, KillTimer, LoadCursorW, MoveWindow,
    RegisterClassExW, SW_HIDE, SW_SHOW, SW_SHOWMAXIMIZED, SW_SHOWMINIMIZED, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SetTimer, SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow,
    UnregisterClassW, WINDOW_EX_STYLE, WINDOW_STYLE, WNDCLASSEXW, WS_TABSTOP,
};
use windows::core::{HSTRING, PCWSTR};

use crate::error::{Error, Result};
use crate::geometry::{Point, Rect};
use crate::hwnd::Hwnd;
use crate::window::WindowHandler;

use super::{raw_hwnd, win32, win32_error};

/// How a window should be shown by [`show`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShowKind {
    Normal,
    Minimized,
    Maximized,
    Hidden,
}

/// The module (`HINSTANCE`) of the running executable.
fn module_instance() -> Result<HINSTANCE> {
    // SAFETY: a null module name asks for the current process's module, which
    // always exists.
    let module = unsafe { GetModuleHandleW(None) }.map_err(win32_error)?;
    Ok(HINSTANCE(module.0))
}

/// The shared arrow cursor.
fn arrow_cursor() -> Result<HCURSOR> {
    // SAFETY: `IDC_ARROW` is a system resource constant and a null module name
    // selects the shared system cursor.
    unsafe { LoadCursorW(None, IDC_ARROW) }.map_err(win32_error)
}

/// Registers a window class whose instances share [`window_proc`].
pub(crate) fn register_class(name: &[u16], display: &str, background: HBRUSH) -> Result<()> {
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        // `CS_DBLCLKS` makes the class receive `WM_*BUTTONDBLCLK` messages.
        style: CS_DBLCLKS,
        lpfnWndProc: Some(super::dispatch::window_proc),
        hInstance: module_instance()?,
        hCursor: arrow_cursor()?,
        hbrBackground: background,
        lpszClassName: PCWSTR(name.as_ptr()),
        ..Default::default()
    };
    // SAFETY: `class` is fully initialised; `lpszClassName` points at `name`,
    // which the caller keeps alive for as long as any window uses the class.
    let atom = unsafe { RegisterClassExW(&class) };
    if atom == 0 {
        return Err(Error::ClassRegistration {
            name: display.to_string(),
        });
    }
    Ok(())
}

/// Unregisters a class previously passed to [`register_class`].
pub(crate) fn unregister_class(name: &[u16]) {
    if let Ok(instance) = module_instance() {
        // SAFETY: `name` is the same nul-terminated string used to register the
        // class; unregistering a class with no live windows is allowed.
        unsafe {
            let _ = UnregisterClassW(PCWSTR(name.as_ptr()), Some(instance));
        }
    }
}

/// The geometry and style bits for [`create`].
pub(crate) struct CreateParams<'a> {
    pub class_name: &'a [u16],
    pub title: &'a str,
    pub style: u32,
    pub ex_style: u32,
    pub bounds: Rect,
    pub parent: Option<Hwnd>,
    pub menu: isize,
}

/// Creates a window of a class registered by [`register_class`], taking
/// ownership of `handler` for the window's lifetime.
pub(crate) fn create<H: WindowHandler + 'static>(
    params: CreateParams<'_>,
    handler: H,
) -> Result<HWND> {
    let boxed: Box<dyn WindowHandler> = Box::new(handler);
    // A `*mut Box<dyn Trait>` is a thin pointer, which is what `lpCreateParams`
    // can carry; the fat vtable pointer lives inside the inner `Box`.
    let raw = Box::into_raw(Box::new(boxed));
    let title = HSTRING::from(params.title);
    let instance = module_instance()?;

    // SAFETY: all pointers passed are valid for the call: the class name and
    // `title` outlive it, and `raw` is an owned allocation reclaimed either
    // below or in `window_proc`.
    let created = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(params.ex_style),
            PCWSTR(params.class_name.as_ptr()),
            &title,
            WINDOW_STYLE(params.style),
            params.bounds.left,
            params.bounds.top,
            params.bounds.width(),
            params.bounds.height(),
            params.parent.map(raw_hwnd),
            Some(HMENU(params.menu as *mut c_void)),
            Some(instance),
            Some(raw as *const c_void),
        )
    };

    match created {
        Ok(hwnd) => Ok(hwnd),
        Err(source) => {
            // SAFETY: creation failed before WM_NCCREATE could adopt the
            // handler, so this is the only reference to it.
            unsafe { drop(Box::from_raw(raw)) };
            let end = params.class_name.len().saturating_sub(1);
            Err(Error::CreateWindow {
                class: String::from_utf16_lossy(&params.class_name[..end]),
                source: win32(source),
            })
        }
    }
}

/// Creates a child control from a system window class (e.g. `SysListView32`).
pub(crate) fn create_control(
    class: &str,
    style: u32,
    ex_style: u32,
    parent: Hwnd,
    id: usize,
    bounds: Rect,
) -> Result<HWND> {
    let class = HSTRING::from(class);
    let title = HSTRING::new();
    // SAFETY: `class` and `title` outlive the call; no create-params are used.
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(ex_style),
            &class,
            &title,
            WINDOW_STYLE(style),
            bounds.left,
            bounds.top,
            bounds.width(),
            bounds.height(),
            Some(raw_hwnd(parent)),
            Some(HMENU(id as *mut c_void)),
            Some(module_instance()?),
            None,
        )
    }
    .map_err(win32_error)
}

thread_local! {
    /// Source of unique, non-zero `SetTimer` ids for this thread.
    static NEXT_TIMER_ID: Cell<usize> = const { Cell::new(0) };
}

/// Destroys a window. Errors (e.g. an already-destroyed handle) are ignored.
pub(crate) fn destroy(hwnd: Hwnd) {
    // SAFETY: `DestroyWindow` on a stale handle is a documented failure, not
    // undefined behaviour.
    unsafe {
        let _ = DestroyWindow(raw_hwnd(hwnd));
    }
}

/// Installs a subclass procedure on `hwnd`, returning whether it succeeded.
pub(crate) fn set_subclass(hwnd: Hwnd, proc: SUBCLASSPROC, id: usize, refdata: usize) -> bool {
    // SAFETY: `hwnd` is live and `proc`/`refdata` follow the subclass contract;
    // the caller keeps `refdata` valid until `remove_subclass`.
    unsafe { windows::Win32::UI::Shell::SetWindowSubclass(raw_hwnd(hwnd), proc, id, refdata) }
        .as_bool()
}

/// Removes a subclass procedure previously installed by [`set_subclass`].
pub(crate) fn remove_subclass(hwnd: Hwnd, proc: SUBCLASSPROC, id: usize) -> bool {
    // SAFETY: `proc`/`id` identify a previously installed subclass.
    unsafe { windows::Win32::UI::Shell::RemoveWindowSubclass(raw_hwnd(hwnd), proc, id) }.as_bool()
}

/// Whether `hwnd` still identifies a live window.
pub(crate) fn is_window(hwnd: Hwnd) -> bool {
    // SAFETY: `IsWindow` only inspects the handle.
    unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(raw_hwnd(hwnd))).as_bool() }
}

/// The client area of `hwnd`, in pixels.
pub(crate) fn client_rect(hwnd: Hwnd) -> Rect {
    let mut rect = windows::Win32::Foundation::RECT::default();
    // SAFETY: `rect` is a valid out-pointer.
    if unsafe { GetClientRect(raw_hwnd(hwnd), &mut rect) }.is_ok() {
        Rect::new(rect.left, rect.top, rect.right, rect.bottom)
    } else {
        Rect::default()
    }
}

/// Invalidates `hwnd` and all its children for a repaint.
///
/// Used when a window comes back from minimized/maximized: DWM re-creates the
/// frame and the material surface plus every child must repaint. Unlike
/// `RDW_UPDATENOW`, this only marks for painting, so it cannot re-enter
/// `WM_PAINT` from inside a size/activation handler.
pub(crate) fn redraw_children(hwnd: Hwnd) {
    // SAFETY: `hwnd` is live; a null update rectangle means the whole window,
    // and only documented redraw flags are passed.
    unsafe {
        let _ = RedrawWindow(
            Some(raw_hwnd(hwnd)),
            None,
            None,
            RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN,
        );
    }
}

/// Converts a client-relative `point` of `hwnd` to screen coordinates.
pub(crate) fn client_to_screen(hwnd: Hwnd, point: Point) -> Point {
    let mut raw = windows::Win32::Foundation::POINT {
        x: point.x,
        y: point.y,
    };
    // SAFETY: `hwnd` is live and `raw` is a valid in/out point.
    unsafe {
        let _ = windows::Win32::Graphics::Gdi::ClientToScreen(raw_hwnd(hwnd), &mut raw);
    }
    Point::new(raw.x, raw.y)
}

/// The outer rectangle of `hwnd` (screen coordinates).
pub(crate) fn window_rect(hwnd: Hwnd) -> Rect {
    let mut rect = windows::Win32::Foundation::RECT::default();
    // SAFETY: `rect` is a valid out-pointer.
    if unsafe { GetWindowRect(raw_hwnd(hwnd), &mut rect) }.is_ok() {
        Rect::new(rect.left, rect.top, rect.right, rect.bottom)
    } else {
        Rect::default()
    }
}

/// Moves/resizes a window.
pub(crate) fn move_window(hwnd: Hwnd, bounds: Rect) {
    // SAFETY: only integer geometry is passed; the handle may be stale, which
    // is a no-op failure.
    unsafe {
        let _ = MoveWindow(
            raw_hwnd(hwnd),
            bounds.left,
            bounds.top,
            bounds.width(),
            bounds.height(),
            true,
        );
    }
}

/// Sends `hwnd` to the bottom of its sibling z-order, so a container created
/// after its content (a tab strip) draws behind the content windows.
pub(crate) fn send_to_back(hwnd: Hwnd) {
    // SAFETY: only state flags and a positioning constant are passed; a stale
    // handle is a documented no-op failure.
    unsafe {
        let _ = SetWindowPos(
            raw_hwnd(hwnd),
            Some(HWND_BOTTOM),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

/// Sets the window title.
pub(crate) fn set_title(hwnd: Hwnd, title: &str) -> Result<()> {
    let title = HSTRING::from(title);
    // SAFETY: `title` outlives the call.
    unsafe { SetWindowTextW(raw_hwnd(hwnd), &title) }.map_err(win32_error)
}

/// Reads the window's title text.
pub(crate) fn get_title(hwnd: Hwnd) -> String {
    const WM_GETTEXT: u32 = 0x000D;
    let mut buffer = vec![0u16; 256];
    let length =
        send_message(hwnd, WM_GETTEXT, buffer.len(), buffer.as_mut_ptr() as isize) as usize;
    if length == 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buffer[..length.min(buffer.len())])
}

/// Enables or disables a window (greyed out and unclickable when disabled).
pub(crate) fn enable_window(hwnd: Hwnd, enabled: bool) {
    // SAFETY: only a state flag is passed; a stale handle is a documented no-op.
    unsafe {
        let _ = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(raw_hwnd(hwnd), enabled);
    }
}

/// Includes or excludes `hwnd` from the Tab order (`WS_TABSTOP`).
pub(crate) fn set_tab_stop(hwnd: Hwnd, tab_stop: bool) {
    // SAFETY: only the window's style bits are read and written; a stale handle
    // is a documented no-op.
    unsafe {
        let style = GetWindowLongPtrW(raw_hwnd(hwnd), GWL_STYLE);
        let updated = if tab_stop {
            style | WS_TABSTOP.0 as isize
        } else {
            style & !(WS_TABSTOP.0 as isize)
        };
        let _ = SetWindowLongPtrW(raw_hwnd(hwnd), GWL_STYLE, updated);
    }
}

/// Re-parents a child window, keeping its coordinates in the new parent.
pub(crate) fn set_parent(child: Hwnd, parent: Hwnd) {
    // SAFETY: both handles are live child windows; `SetParent` only changes the
    // parent linkage and returns the previous parent, which needs no cleanup.
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::SetParent(
            raw_hwnd(child),
            Some(raw_hwnd(parent)),
        );
    }
}

/// Sets the keyboard focus to `hwnd`.
pub(crate) fn set_focus(hwnd: Hwnd) {
    // SAFETY: only the handle is passed; the returned previous focus needs no
    // cleanup.
    unsafe {
        let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(raw_hwnd(hwnd)));
    }
}

/// Schedules a full repaint.
pub(crate) fn invalidate(hwnd: Hwnd) {
    // SAFETY: `None`/true means "erase and repaint the whole client area".
    unsafe {
        let _ = InvalidateRect(Some(raw_hwnd(hwnd)), None, true);
    }
}

/// Schedules a repaint of `rect` (device pixels, in the window's client
/// coordinates) *without* erasing it first: the widget repaints those pixels
/// itself.
///
/// Windows unions the rectangle into the window's update region, so several
/// calls before the next paint coalesce into one `WM_PAINT` whose dirty
/// rectangle is their bounding box.
pub(crate) fn invalidate_rect(hwnd: Hwnd, rect: Rect) {
    if rect.is_empty() {
        return;
    }
    let raw = RECT {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
    };
    // SAFETY: `raw` is a valid rectangle for the duration of the call; a stale
    // handle makes the call fail harmlessly.
    unsafe {
        let _ = InvalidateRect(Some(raw_hwnd(hwnd)), Some(&raw), false);
    }
}

/// The window's pending update rectangle (device pixels), or the whole client
/// area when Windows reports none. The Direct2D paint path clips its frame to
/// this so pixels outside it are left untouched.
pub(crate) fn update_rect(hwnd: Hwnd) -> Rect {
    let mut raw = RECT::default();
    // SAFETY: `raw` is a valid out-pointer; `berase = false` leaves the erase
    // state alone and a stale handle makes the call fail harmlessly.
    let ok = unsafe { GetUpdateRect(raw_hwnd(hwnd), Some(&mut raw), false) };
    if !ok.as_bool() {
        return client_rect(hwnd);
    }
    Rect::new(raw.left, raw.top, raw.right, raw.bottom)
}

/// Marks the whole client area as painted, so Windows stops asking for it.
pub(crate) fn validate(hwnd: Hwnd) {
    // SAFETY: a null rectangle means the whole client area; a stale handle
    // makes the call fail harmlessly.
    unsafe {
        let _ = ValidateRect(Some(raw_hwnd(hwnd)), None);
    }
}

/// Marks `rect` (device pixels) as painted, so Windows stops asking for it.
pub(crate) fn validate_rect(hwnd: Hwnd, rect: Rect) {
    let raw = RECT {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
    };
    // SAFETY: `raw` is a valid rectangle for the duration of the call; a stale
    // handle makes the call fail harmlessly.
    unsafe {
        let _ = ValidateRect(Some(raw_hwnd(hwnd)), Some(&raw));
    }
}

/// Shows, hides or minimizes a window.
pub(crate) fn show(hwnd: Hwnd, kind: ShowKind) {
    let cmd = match kind {
        ShowKind::Normal => SW_SHOW,
        ShowKind::Minimized => SW_SHOWMINIMIZED,
        ShowKind::Maximized => SW_SHOWMAXIMIZED,
        ShowKind::Hidden => SW_HIDE,
    };
    // SAFETY: state flags only; a stale handle is a documented no-op.
    unsafe {
        let _ = ShowWindow(raw_hwnd(hwnd), cmd);
        let _ = UpdateWindow(raw_hwnd(hwnd));
    }
}

/// Starts a timer and returns the id it was given.
///
/// With a non-null window handle, `SetTimer` uses `nIDEvent` itself as the
/// timer id; its return value is only documented as nonzero on success, so it
/// is not the id. This passes a fresh nonzero id and returns that id, which is
/// what `WM_TIMER` reports back in `wparam`.
pub(crate) fn set_timer(hwnd: Hwnd, millis: u32) -> Result<usize> {
    let id = NEXT_TIMER_ID.with(|next| {
        let id = next.get().wrapping_add(1).max(1);
        next.set(id);
        id
    });
    // SAFETY: `None` installs a WM_TIMER message rather than a callback.
    let created = unsafe { SetTimer(Some(raw_hwnd(hwnd)), id, millis, None) };
    if created == 0 {
        Err(win32_error(windows::core::Error::from_thread()))
    } else {
        Ok(id)
    }
}

/// Stops a timer started by [`set_timer`].
pub(crate) fn kill_timer(hwnd: Hwnd, id: usize) {
    // SAFETY: killing an unknown id is a documented no-op.
    unsafe {
        let _ = KillTimer(Some(raw_hwnd(hwnd)), id);
    }
}

/// Arms one `WM_MOUSELEAVE` notification for the next time the cursor leaves
/// `hwnd`.
pub(crate) fn track_mouse_leave(hwnd: Hwnd) -> Result<()> {
    let mut event = TRACKMOUSEEVENT {
        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
        dwFlags: TME_LEAVE,
        hwndTrack: raw_hwnd(hwnd),
        dwHoverTime: 0,
    };
    // SAFETY: `event` is fully initialised and only read by the call.
    unsafe { TrackMouseEvent(&mut event) }.map_err(win32_error)
}

/// Posts a message without waiting for it to be handled.
pub(crate) fn post_message(hwnd: Hwnd, msg: u32, wparam: usize, lparam: isize) -> Result<()> {
    // SAFETY: `PostMessageW` only queues the values.
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::PostMessageW(
            Some(raw_hwnd(hwnd)),
            msg,
            WPARAM(wparam),
            LPARAM(lparam),
        )
    }
    .map_err(win32_error)
}

/// Sends a message and waits for its result.
pub(crate) fn send_message(hwnd: Hwnd, msg: u32, wparam: usize, lparam: isize) -> isize {
    // SAFETY: `SendMessageW` only forwards the raw values.
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::SendMessageW(
            raw_hwnd(hwnd),
            msg,
            Some(WPARAM(wparam)),
            Some(LPARAM(lparam)),
        )
        .0
    }
}
