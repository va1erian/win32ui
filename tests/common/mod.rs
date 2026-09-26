//! Shared scaffolding for the integration tests: class registration, a
//! watchdog timer and the run loop, so each test only describes its handler.
//!
//! If the CI session cannot create windows at all, [`run_with_watchdog`]
//! returns `None` and the test skips rather than fails.

// Each test binary compiles this module separately, so a helper used by one
// test is "dead" in the others.
#![allow(dead_code)]

use std::cell::Cell;
use std::rc::Rc;

use win32ui::prelude::*;

pub mod uia;

/// Milliseconds after which the watchdog gives up on a handler.
const WATCHDOG_MS: u32 = 5000;
/// How a watched run ended.
pub struct Run {
    /// Whether the watchdog fired before the handler quit.
    pub timed_out: bool,
    /// The id of the watchdog timer, so a test can prove its own timer got a
    /// different id.
    pub watchdog: Option<TimerId>,
    /// The window, for liveness assertions after the run.
    pub window: Window,
}

/// Creates a window of a freshly registered class, runs its message loop under
/// a watchdog, and reports how it ended.
///
/// `make` builds the handler. The helper starts a watchdog timer on the
/// window's `Create` (before forwarding it to the handler) and quits the loop
/// if it fires, so a handler that never finishes fails the test instead of
/// hanging it. Returns `None` when the session cannot create windows.
pub fn run_with_watchdog<H, F>(name: &str, make: F) -> Option<Run>
where
    H: WindowHandler + 'static,
    F: FnOnce() -> H,
{
    win32ui::init();

    let theme = Theme::light();
    let Ok(class) = WindowClass::register(name, theme.background) else {
        return None;
    };
    let timed_out = Rc::new(Cell::new(false));
    let watchdog = Rc::new(Cell::new(None));
    let handler = WatchdogHandler {
        inner: make(),
        watchdog: Rc::clone(&watchdog),
        timed_out: Rc::clone(&timed_out),
    };
    let Ok(window) = Window::create(
        class,
        None,
        WindowStyle::overlapped(),
        WindowExStyle::new(),
        Rect::new(0, 0, 320, 240),
        name,
        handler,
    ) else {
        return None;
    };

    window.show();
    win32ui::run();
    Some(Run {
        timed_out: timed_out.get(),
        watchdog: watchdog.get(),
        window,
    })
}

/// Wraps a test handler to own the watchdog timer and quit the loop if it
/// fires.
struct WatchdogHandler<H> {
    inner: H,
    watchdog: Rc<Cell<Option<TimerId>>>,
    timed_out: Rc<Cell<bool>>,
}

impl<H: WindowHandler> WindowHandler for WatchdogHandler<H> {
    fn message(&self, window: &Window, message: Message) -> Option<LResult> {
        match message {
            Message::Create => {
                self.watchdog.set(window.set_timer(WATCHDOG_MS).ok());
                self.inner.message(window, message)
            }
            Message::Timer { id } if Some(id) == self.watchdog.get() => {
                self.timed_out.set(true);
                window.destroy();
                win32ui::quit(1);
                Some(0)
            }
            _ => self.inner.message(window, message),
        }
    }
}

/// A handler that ignores every message.
pub struct NullHandler;

impl WindowHandler for NullHandler {
    fn message(&self, _window: &Window, _message: Message) -> Option<LResult> {
        None
    }
}

/// How a watched widget-layer run ended.
pub struct RunApp {
    /// Whether the watchdog fired before the app quit.
    pub timed_out: bool,
    /// The id of the watchdog timer.
    pub watchdog: Option<TimerId>,
}

/// Runs a widget-layer app under a watchdog: the helper starts a watchdog timer
/// on the window before `make` runs and quits the loop (recording `timed_out`)
/// if it fires, so an app that never quits fails the test instead of hanging.
///
/// `make` receives the `Ui` and must enqueue its first message (typically
/// `ui.emit(..)`) and arrange for the app to quit once done. Returns `None`
/// when the session cannot create windows.
pub fn run_app_with_watchdog<A, F>(name: &str, make: F) -> Option<RunApp>
where
    A: App + 'static,
    F: FnOnce(&mut Ui<A::Msg>) -> A,
{
    run_app_spec_with_watchdog(WindowSpec::new(name).theme(Theme::light()), make)
}

/// Like [`run_app_with_watchdog`], but with a caller-built [`WindowSpec`], so a
/// test can exercise spec options such as [`Backdrop`] or [`TitleBar`].
pub fn run_app_spec_with_watchdog<A, F>(spec: WindowSpec, make: F) -> Option<RunApp>
where
    A: App + 'static,
    F: FnOnce(&mut Ui<A::Msg>) -> A,
{
    run_app_spec_with_watchdog_ms(spec, WATCHDOG_MS, make)
}

/// Like [`run_app_spec_with_watchdog`], with a longer watchdog for a
/// measurement that legitimately runs for many seconds.
pub fn run_app_spec_with_watchdog_ms<A, F>(
    spec: WindowSpec,
    watchdog_ms: u32,
    make: F,
) -> Option<RunApp>
where
    A: App + 'static,
    F: FnOnce(&mut Ui<A::Msg>) -> A,
{
    win32ui::init();

    let timed_out = Rc::new(Cell::new(false));
    let watchdog = Rc::new(Cell::new(None));
    let timed_out_for_timer = Rc::clone(&timed_out);
    let watchdog_for_timer = Rc::clone(&watchdog);
    let result = win32ui::run_app(spec, move |ui| {
        let id = ui.set_timer(watchdog_ms).ok();
        watchdog_for_timer.set(id);
        ui.on_timer(move |fired| {
            if Some(fired) == id {
                timed_out_for_timer.set(true);
                win32ui::quit(1);
            }
            None
        });
        make(ui)
    });

    result.ok()?;
    Some(RunApp {
        timed_out: timed_out.get(),
        watchdog: watchdog.get(),
    })
}

/// One row of the shared five-row test model.
pub struct TestRow {
    /// Display text for the first column.
    pub label: String,
}

/// Five list rows, enough to exercise owner-data requests.
pub fn test_rows() -> Vec<TestRow> {
    (0..5)
        .map(|item| TestRow {
            label: format!("{item}/0"),
        })
        .collect()
}

/// `hwnd` as a screen-coordinate rectangle, or `None` if it is not a window.
///
/// `Window::capture`/`PrintWindow` do not capture popups (menus and combo
/// drop-down lists are separate top-level windows), so tests that need their
/// pixels capture the screen region they occupy.
pub fn screen_rect(hwnd: win32ui::Hwnd) -> Option<Rect> {
    use core::ffi::c_void;
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;

    let mut rect = RECT::default();
    // SAFETY: `hwnd` is a live window handle; `rect` is a valid out-pointer.
    if unsafe { GetWindowRect(HWND(hwnd.raw() as *mut c_void), &mut rect) }.is_err() {
        return None;
    }
    Some(Rect::new(rect.left, rect.top, rect.right, rect.bottom))
}

/// Copies the screen region `rect` into an [`RgbaImage`] with GDI `BitBlt`.
///
/// Returns `None` when the region is empty or the blit fails (for example on a
/// session without a visible desktop), so a test can skip rather than fail.
pub fn capture_screen(rect: Rect) -> Option<RgbaImage> {
    use core::ffi::c_void;
    use core::ptr::{null_mut, slice_from_raw_parts_mut};
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleDC, CreateDIBSection,
        DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, HGDIOBJ, ReleaseDC, SRCCOPY, SelectObject,
    };

    let width = rect.width();
    let height = rect.height();
    if width <= 0 || height <= 0 {
        return None;
    }

    let mut info = BITMAPINFO::default();
    info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    info.bmiHeader.biWidth = width;
    info.bmiHeader.biHeight = -height; // top-down rows
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB.0;

    // SAFETY: a null window asks for the screen DC; it is released below.
    let screen = unsafe { GetDC(None) };
    if screen.0.is_null() {
        return None;
    }
    // SAFETY: `screen` is a live DC; the memory DC is released below.
    let memory = unsafe { CreateCompatibleDC(Some(screen)) };
    if memory.0.is_null() {
        // SAFETY: `screen` came from `GetDC` just above.
        unsafe { ReleaseDC(None, screen) };
        return None;
    }

    let mut bits: *mut c_void = null_mut();
    // SAFETY: `info` is fully initialised, `bits` is a valid out-pointer.
    let bitmap = match unsafe { CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0) }
    {
        Ok(bitmap) if !bits.is_null() => bitmap,
        _ => {
            // SAFETY: both DCs are live and have no bitmap selected.
            unsafe {
                let _ = DeleteDC(memory);
                ReleaseDC(None, screen);
            }
            return None;
        }
    };

    // SAFETY: `bitmap` and `memory` are live; the old object is restored below.
    let old = unsafe { SelectObject(memory, HGDIOBJ(bitmap.0)) };
    // SAFETY: `screen` and `memory` are live, `rect` is inside the screen.
    let blitted = unsafe {
        BitBlt(
            memory,
            0,
            0,
            width,
            height,
            Some(screen),
            rect.left,
            rect.top,
            SRCCOPY,
        )
    };

    let pixels = if blitted.is_ok() {
        let bytes = width as usize * height as usize * 4;
        // SAFETY: `bits` points at `bytes` writable bytes owned by the DIB.
        let source = unsafe { &*slice_from_raw_parts_mut(bits as *mut u8, bytes) };
        let mut pixels = Vec::with_capacity(bytes);
        for pixel in source.as_chunks::<4>().0 {
            pixels.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 0xFF]);
        }
        pixels
    } else {
        Vec::new()
    };

    // SAFETY: `old` was returned by selecting `bitmap`; restoring it lets both
    // the DC and the bitmap be released without a dangling selection.
    unsafe {
        SelectObject(memory, old);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
    }

    if pixels.is_empty() {
        return None;
    }
    Some(RgbaImage {
        width: width as u32,
        height: height as u32,
        pixels,
    })
}

/// Clears a control's invalidated region, so a later [`has_pending_paint`]
/// only sees invalidations raised after this call.
pub fn clear_pending_paint(hwnd: win32ui::Hwnd) {
    use core::ffi::c_void;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::ValidateRect;

    // SAFETY: `hwnd` is a live control handle; `ValidateRect` only reads it.
    unsafe {
        let _ = ValidateRect(Some(HWND(hwnd.raw() as *mut c_void)), None);
    }
}

/// Whether a control has an invalidated region awaiting its next `WM_PAINT`.
pub fn has_pending_paint(hwnd: win32ui::Hwnd) -> bool {
    use core::ffi::c_void;
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Gdi::GetUpdateRect;

    let mut rect = RECT::default();
    // SAFETY: `hwnd` is a live control handle; `rect` is a valid out-pointer.
    unsafe { GetUpdateRect(HWND(hwnd.raw() as *mut c_void), Some(&mut rect), false).as_bool() }
}

/// Whether `color` is near-white (all channels high), the shape of the #67
/// combo regression.
pub fn is_near_white(color: [u8; 4]) -> bool {
    color[0] > 200 && color[1] > 200 && color[2] > 200
}

/// The fraction of pixels in `image` that are near-white.
pub fn near_white_fraction(image: &RgbaImage) -> f64 {
    let total = (image.width as usize * image.height as usize).max(1);
    let white = image
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| is_near_white([p[0], p[1], p[2], p[3]]))
        .count();
    white as f64 / total as f64
}
