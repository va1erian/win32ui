//! Window classes, creation, the shared window procedure, and per-window
//! operations.

use core::cell::RefCell;
use core::ffi::c_void;
use std::collections::HashSet;
use std::panic::{AssertUnwindSafe, catch_unwind};

use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{HBRUSH, InvalidateRect, UpdateWindow};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::SUBCLASSPROC;
use windows::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA, GetClientRect,
    GetWindowLongPtrW, GetWindowRect, HCURSOR, HMENU, IDC_ARROW, KillTimer, LoadCursorW,
    MoveWindow, RegisterClassExW, SW_HIDE, SW_SHOW, SW_SHOWMAXIMIZED, SW_SHOWMINIMIZED, SetTimer,
    SetWindowLongPtrW, SetWindowTextW, ShowWindow, UnregisterClassW, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_NCCREATE, WM_NCDESTROY, WM_NOTIFY, WNDCLASSEXW,
};
use windows::core::{HSTRING, PCWSTR};

use crate::error::{Error, Result};
use crate::geometry::Rect;
use crate::hwnd::Hwnd;
use crate::window::WindowHandler;

use super::{hwnd_from, raw_hwnd, win32, win32_error};

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
        lpfnWndProc: Some(window_proc),
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
    /// `HWND`s (as `isize`) currently inside [`dispatch`], to detect and
    /// break reentrant calls into the same handler.
    static ACTIVE_HANDLERS: RefCell<HashSet<isize>> = RefCell::new(HashSet::new());
}

/// The window procedure shared by every class registered by this crate.
///
/// # Safety
/// Called by Windows with a valid `hwnd` for a window created through
/// [`create`]; `msg`/`wparam`/`lparam` follow the documented Win32 contract.
unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_NCCREATE {
        // SAFETY: for WM_NCCREATE, lparam is a CREATESTRUCTW* owned by the
        // system for the duration of the call; the handler pointer was boxed
        // by `create` and is reclaimed exactly once on WM_NCDESTROY.
        unsafe {
            let create = &*(lparam.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        }
    }

    // SAFETY: reads back the pointer stored above (null for foreign windows).
    let raw = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Box<dyn WindowHandler>;

    // Guards against two live `&mut` to the same handler: a message handled
    // here can synchronously trigger another message to the same window
    // (e.g. a Win32 call that sends rather than posts). Without this, the
    // reentrant call would take a second `&mut` to the boxed handler while
    // the first is still on the stack, which is undefined behaviour.
    let key = hwnd.0 as isize;
    let reentrant = ACTIVE_HANDLERS.with(|active| active.borrow().contains(&key));

    let handled = if !raw.is_null() && msg != WM_NCDESTROY && !reentrant {
        ACTIVE_HANDLERS.with(|active| active.borrow_mut().insert(key));
        // SAFETY: `raw` was produced by `Box::into_raw` in `create` and is
        // freed exactly once on WM_NCDESTROY; `reentrant` rules out a second
        // live `&mut` to the same allocation.
        let handler: &mut Box<dyn WindowHandler> = unsafe { &mut *raw };
        // A panic unwinding across this `extern "system"` boundary is
        // undefined behaviour; isolate it instead of letting it propagate.
        let result = catch_unwind(AssertUnwindSafe(|| {
            dispatch(hwnd, msg, wparam, lparam, handler)
        }))
        .unwrap_or(None);
        ACTIVE_HANDLERS.with(|active| active.borrow_mut().remove(&key));
        result
    } else {
        None
    };

    let result = match handled {
        Some(value) => LRESULT(value),
        None => default_proc(hwnd, msg, wparam, lparam),
    };

    if msg == WM_NCDESTROY && !raw.is_null() {
        // SAFETY: the window is gone; drop the handler (and anything it owns)
        // now, exactly once.
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            drop(Box::from_raw(raw));
        }
    }
    result
}

/// Default handling for a message the application did not claim.
fn default_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: `DefWindowProcW` is the documented default for any message a
    // window procedure does not handle.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn dispatch(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    handler: &mut Box<dyn WindowHandler>,
) -> Option<isize> {
    // Registered controls get first refusal on their own notifications
    // (owner-data requests, custom draw, lazy expansion…).
    if msg == WM_NOTIFY
        && let Some((from, _id, code)) = super::message::notify_header(lparam)
        && let Some(result) =
            crate::controls::registry::dispatch(hwnd_from(from), code, wparam.0, lparam.0)
    {
        return Some(result);
    }

    let message = super::message::decode(hwnd, msg, wparam, lparam);
    let window = crate::window::Window::from_raw(hwnd_from(hwnd));
    handler.message(&window, message)
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

/// Sets the window title.
pub(crate) fn set_title(hwnd: Hwnd, title: &str) -> Result<()> {
    let title = HSTRING::from(title);
    // SAFETY: `title` outlives the call.
    unsafe { SetWindowTextW(raw_hwnd(hwnd), &title) }.map_err(win32_error)
}

/// Schedules a full repaint.
pub(crate) fn invalidate(hwnd: Hwnd) {
    // SAFETY: `None`/true means "erase and repaint the whole client area".
    unsafe {
        let _ = InvalidateRect(Some(raw_hwnd(hwnd)), None, true);
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

/// Starts a timer, returning its id.
pub(crate) fn set_timer(hwnd: Hwnd, millis: u32) -> Result<usize> {
    // SAFETY: `None` installs a WM_TIMER message rather than a callback.
    let id = unsafe { SetTimer(Some(raw_hwnd(hwnd)), 0, millis, None) };
    if id == 0 {
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
