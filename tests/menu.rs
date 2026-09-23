//! Menus (#16): a menu bar mapped to `Msg`, owner-drawn dark items and the
//! live theme switch.
//!
//! Window-creating tests use the shared watchdog helper so failures fail
//! instead of hanging.

#![cfg(windows)]

mod common;

use common::{
    capture_screen, is_near_white, near_white_fraction, run_app_spec_with_watchdog,
    run_app_with_watchdog, screen_rect,
};
use win32ui::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq)]
enum MenuMsg {
    Start,
    Toggled,
    Done,
}

struct MenuApp;

impl App for MenuApp {
    type Msg = MenuMsg;

    fn update(&mut self, msg: MenuMsg, ui: &mut Ui<MenuMsg>) {
        match msg {
            MenuMsg::Start => ui.emit(MenuMsg::Toggled),
            // Switching the theme rebuilds the owner-drawn menu bar live.
            MenuMsg::Toggled => {
                ui.set_theme(Theme::dark());
                ui.emit(MenuMsg::Done);
            }
            MenuMsg::Done => ui.quit(),
        }
    }
}

/// Installing a menu bar (with a submenu, a checked and a disabled item) and
/// re-theming the window both complete and shut down cleanly.
#[test]
fn menu_bar_installs_and_rethemes() {
    let Some(run) = run_app_with_watchdog("win32ui.menu.bar", |ui| {
        let menu = Menu::new()
            .item("&Reset", Shortcut::ctrl(Key::R), || MenuMsg::Done)
            .separator()
            .checked_item("&Check", None, true, || MenuMsg::Done)
            .radio_item("&Radio", None, true, || MenuMsg::Done)
            .disabled_item("&Off", None, || MenuMsg::Done)
            .submenu("&More", Menu::new().item("&Deep", None, || MenuMsg::Done));
        ui.set_menu_bar(menu);
        ui.emit(MenuMsg::Start);
        MenuApp
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
}

/// An empty menu still installs and clears without panicking.
#[test]
fn empty_menu_is_fine() {
    struct Empty;
    impl App for Empty {
        type Msg = ();
        fn update(&mut self, _msg: (), ui: &mut Ui<()>) {
            ui.quit();
        }
    }

    let Some(run) = run_app_with_watchdog("win32ui.menu.empty", |ui| {
        ui.set_menu_bar(Menu::new());
        ui.emit(());
        Empty
    }) else {
        return;
    };
    assert!(!run.timed_out, "the watchdog fired before the app quit");
}

/// #67: a dark owner-drawn popup (checkbox, radio, disabled item and a
/// submenu) must paint a dark background. Popups are separate top-level windows
/// that `PrintWindow` on the owner misses, so a worker thread captures the
/// screen region of the popup (class `#32768`, from `Winuser.h`) and then
/// cancels the tracked menu so the app can quit.
#[test]
fn dark_menu_popup_is_dark() {
    enum PopupMsg {
        Start,
        Done,
    }

    struct PopupApp {
        menu: Menu<PopupMsg>,
    }

    impl App for PopupApp {
        type Msg = PopupMsg;

        fn update(&mut self, msg: PopupMsg, ui: &mut Ui<PopupMsg>) {
            match msg {
                PopupMsg::Start => {
                    let rect = ui.window_rect();
                    // Track the popup over the window; the worker thread finds
                    // its real rectangle and captures that.
                    let at = Point::new(rect.left + 60, rect.top + 120);
                    ui.popup(&self.menu, at);
                    ui.emit(PopupMsg::Done);
                }
                PopupMsg::Done => ui.quit(),
            }
        }
    }

    let image = std::sync::Arc::new(std::sync::Mutex::new(None));
    let image_for_thread = std::sync::Arc::clone(&image);
    let owner = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let owner_for_thread = std::sync::Arc::clone(&owner);

    let Some(run) = run_app_spec_with_watchdog(
        WindowSpec::new("win32ui.menu.popup").theme(Theme::dark()),
        move |ui| {
            use std::sync::atomic::Ordering;
            owner.store(ui.hwnd().raw(), Ordering::Relaxed);
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(700));
                if let Some(captured) = capture_menu_popup() {
                    *image_for_thread.lock().expect("popup image") = Some(captured);
                }
                // Cancel the tracked menu so `TrackPopupMenuEx` returns.
                let raw = owner_for_thread.load(Ordering::Relaxed);
                if raw != 0 {
                    use core::ffi::c_void;
                    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
                    use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CANCELMODE};
                    // SAFETY: `raw` is the live owner window handle;
                    // `PostMessageW` only queues a message.
                    unsafe {
                        let _ = PostMessageW(
                            Some(HWND(raw as *mut c_void)),
                            WM_CANCELMODE,
                            WPARAM(0),
                            LPARAM(0),
                        );
                    }
                }
            });
            let menu = Menu::new()
                .item("&Play", None, || PopupMsg::Done)
                .checked_item("&Loop", None, true, || PopupMsg::Done)
                .radio_item("&Shuffle", None, true, || PopupMsg::Done)
                .disabled_item("&Transcode", None, || PopupMsg::Done)
                .separator()
                .submenu(
                    "&Copy to",
                    Menu::new().item("&Clipboard", None, || PopupMsg::Done),
                );
            ui.emit(PopupMsg::Start);
            PopupApp { menu }
        },
    ) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    // A headless session cannot blit the screen; skip rather than fail.
    let Some(image) = image.lock().expect("popup image").clone() else {
        return;
    };
    assert!(
        image.width > 40 && image.height > 40,
        "the captured popup is too small to be a menu: {}x{}",
        image.width,
        image.height
    );
    let white = near_white_fraction(&image);
    assert!(
        white < 0.2,
        "the dark popup is mostly near-white ({:.0}% white)",
        white * 100.0
    );
    assert!(
        !image
            .pixel(image.width / 2, image.height / 2)
            .is_some_and(is_near_white),
        "the dark popup's centre is near-white"
    );
}

/// The active popup menu's pixels, found by its documented window class name
/// (`#32768`).
fn capture_menu_popup() -> Option<RgbaImage> {
    use windows::Win32::UI::WindowsAndMessaging::FindWindowW;
    use windows::core::PCWSTR;

    // SAFETY: a class-name lookup only; null means no popup is open.
    let hwnd = unsafe { FindWindowW(windows::core::w!("#32768"), PCWSTR::null()) }.ok()?;
    if hwnd.0.is_null() {
        return None;
    }
    let rect = screen_rect(Hwnd::from_raw(hwnd.0 as usize))?;
    capture_screen(rect)
}
