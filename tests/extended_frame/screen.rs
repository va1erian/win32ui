//! Screen-capture scaffolding for the extended-frame checks: run a window,
//! bring it to the foreground, park the real pointer and capture the screen.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Mutex;
use std::time::Duration;

use win32ui::prelude::*;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN, SWP_NOACTIVATE, SWP_NOSIZE,
    SWP_NOZORDER, SetCursorPos, SetWindowPos,
};

use crate::common;

/// Two overlapping windows (tests run in parallel) would capture each other.
static WINDOW_LOCK: Mutex<()> = Mutex::new(());

/// Lets DWM's appear animation and the activation settle before capturing.
const SETTLE: Duration = Duration::from_millis(900);

/// One captured window and the geometry the checks need, all in capture
/// coordinates (pixels from the window's top-left).
#[allow(dead_code, reason = "not every capture test reads every field")]
pub struct Shot {
    pub image: RgbaImage,
    pub theme: Theme,
    pub strip_height: i32,
    pub buttons: Rect,
    pub menu_bar: Rect,
    pub backdrop_active: bool,
    pub foreground: bool,
}

#[derive(Clone)]
pub enum Msg {
    Start,
    Capture,
}

pub struct ShotApp {
    shot: Rc<RefCell<Option<Shot>>>,
}

impl win32ui::App for ShotApp {
    type Msg = Msg;
    fn update(&mut self, msg: Msg, ui: &mut Ui<Msg>) {
        // The window is shown by now, so the foreground request can take effect.
        if let Msg::Start = msg {
            place_window(ui);
            ui.set_foreground();
            return;
        }
        let origin = ui.window_rect();
        let relative = |rect: Rect| {
            if rect.is_empty() {
                rect
            } else {
                Rect::new(
                    rect.left - origin.left,
                    rect.top - origin.top,
                    rect.right - origin.left,
                    rect.bottom - origin.top,
                )
            }
        };
        if let Ok(image) = ui.capture_screen() {
            *self.shot.borrow_mut() = Some(Shot {
                image,
                theme: ui.theme(),
                strip_height: ui.strip_height().value(),
                buttons: ui.caption_buttons(),
                menu_bar: relative(ui.menu_bar_rect()),
                backdrop_active: ui.backdrop_active(),
                foreground: ui.is_foreground(),
            });
        }
        ui.quit();
    }
}

fn menu_bar() -> Menu<Msg> {
    Menu::new()
        .submenu("&File", Menu::new().item("&Scan", None, || Msg::Capture))
        .submenu("&View", Menu::new().item("&Refresh", None, || Msg::Capture))
}

/// Parks the real pointer in the primary screen's far bottom-right corner, so a
/// stray hover cannot tint a caption button, and puts it back where the user
/// left it when dropped (including when a test fails). Harmless without an
/// interactive desktop.
pub struct PointerGuard(Option<POINT>);

impl PointerGuard {
    pub fn park() -> PointerGuard {
        let mut saved = POINT::default();
        // SAFETY: `saved` is a valid out-pointer; the calls take plain integers
        // and a failure (no interactive desktop) is ignored.
        unsafe {
            let saved = GetCursorPos(&mut saved).ok().map(|()| saved);
            let _ = SetCursorPos(
                GetSystemMetrics(SM_CXSCREEN) - 1,
                GetSystemMetrics(SM_CYSCREEN) - 1,
            );
            PointerGuard(saved)
        }
    }
}

impl Drop for PointerGuard {
    fn drop(&mut self) {
        if let Some(point) = self.0 {
            // SAFETY: plain integer arguments; a failure is ignored.
            unsafe {
                let _ = SetCursorPos(point.x, point.y);
            }
        }
    }
}

/// Runs `spec` with a menu bar, brings it to the foreground and captures the
/// screen once it has settled. `None` (after printing why) when the session
/// cannot show or capture the window.
pub fn capture(spec: WindowSpec) -> Option<Shot> {
    let _guard = WINDOW_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let shot = Rc::new(RefCell::new(None));
    let shot_for_app = Rc::clone(&shot);
    let run = common::run_app_spec_with_watchdog(spec, move |ui| {
        ui.set_menu_bar(menu_bar());
        let proxy = ui.proxy();
        std::thread::spawn(move || {
            std::thread::sleep(SETTLE / 3);
            let _ = proxy.send(Msg::Start);
            std::thread::sleep(SETTLE);
            let _ = proxy.send(Msg::Capture);
        });
        ShotApp { shot: shot_for_app }
    });
    let Some(run) = run else {
        eprintln!("skipping: this session cannot create windows");
        return None;
    };
    assert!(!run.timed_out, "the watchdog fired before the capture");
    let taken = shot.borrow_mut().take();
    if taken.is_none() {
        eprintln!("skipping: the screen capture failed (no interactive desktop?)");
    }
    taken
}

/// Whether the material can be checked on this machine; prints why not.
pub fn material_visible(shot: &Shot, name: &str) -> bool {
    if !shot.backdrop_active {
        eprintln!(
            "skipping {name}: the backdrop is not active (Windows build < 22621, transparency \
             effects off, high contrast, or the extended frame was refused)"
        );
        return false;
    }
    if !shot.foreground {
        eprintln!("skipping {name}: not the foreground window, so DWM shows no material");
        return false;
    }
    true
}

/// Puts the window at a fixed spot on the primary monitor, away from the corner
/// the pointer is parked in and from the taskbar.
fn place_window(ui: &Ui<Msg>) {
    // SAFETY: the handle is the live window of this thread; the call only moves it.
    unsafe {
        let _ = SetWindowPos(
            HWND(ui.hwnd().raw() as *mut core::ffi::c_void),
            None,
            100,
            100,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}
