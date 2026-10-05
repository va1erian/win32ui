//! Regression (#152): dialog navigation must not hang on a control nested in a
//! scrolled panel.
//!
//! `IsDialogMessageW` walks the direct children of the window it is given. The
//! pump used to hand it the app window for every message, so with a default
//! push button on the app window, a click (or Tab) on a push button nested in a
//! `ScrollView` + `Panel` — a settings page — never found its way back to its
//! starting control and the loop spun at full CPU forever.
//!
//! A spin inside the message loop cannot be interrupted by the window watchdog,
//! so a separate thread aborts the test process if the run does not finish.

#![cfg(windows)]

mod common;

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use common::run_app_with_watchdog;
use win32ui::prelude::*;

unsafe extern "system" {
    fn PostMessageW(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> i32;
}

const WM_KEYDOWN: u32 = 0x0100;
const WM_KEYUP: u32 = 0x0101;
const WM_LBUTTONDOWN: u32 = 0x0201;
const WM_LBUTTONUP: u32 = 0x0202;
const VK_TAB: usize = 0x09;

struct Shell;

impl App for Shell {
    type Msg = ();

    fn update(&mut self, _msg: (), ui: &mut Ui<()>) {
        ui.quit();
    }
}

#[test]
fn clicking_and_tabbing_a_nested_button_does_not_hang() {
    let finished = Arc::new(AtomicBool::new(false));
    let guard = Arc::clone(&finished);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(15));
        if !guard.load(Ordering::SeqCst) {
            eprintln!("the message loop hung on a button nested in a scrolled panel");
            std::process::exit(1);
        }
    });

    let keep = RefCell::new(Vec::<Box<dyn std::any::Any>>::new());
    let Some(run) = run_app_with_watchdog("win32ui.dialognav", |ui| {
        let view = ScrollView::new(ui).unwrap();
        let panel = Panel::new(ui).unwrap();
        view.set_content(&panel);
        let mut inner = panel.ui(ui);
        let nested = Button::<()>::new(&mut inner, "Nested").unwrap();
        nested.set_bounds(Rect::new(10, 10, 110, 40));
        // The app window's own default push button is what sends the old
        // pump's default-button search around the wrong list of controls.
        let default = Button::<()>::new(ui, "Default").unwrap().default();
        default.set_bounds(Rect::new(0, 250, 100, 280));
        panel.set_bounds(Rect::new(0, 0, 300, 300));
        view.set_bounds(Rect::new(0, 0, 300, 300));

        let target = nested.hwnd().raw() as isize;
        let proxy = ui.proxy();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            // SAFETY: plain posted messages to a live window.
            unsafe {
                PostMessageW(target, WM_LBUTTONDOWN, 1, (15 << 16) | 15);
                PostMessageW(target, WM_LBUTTONUP, 0, (15 << 16) | 15);
                PostMessageW(target, WM_KEYDOWN, VK_TAB, 0);
                PostMessageW(target, WM_KEYUP, VK_TAB, 0);
            }
            std::thread::sleep(Duration::from_millis(700));
            let _ = proxy.send(());
        });
        keep.borrow_mut()
            .push(Box::new((view, panel, nested, default)));
        Shell
    }) else {
        finished.store(true, Ordering::SeqCst);
        return;
    };
    finished.store(true, Ordering::SeqCst);
    assert!(!run.timed_out, "the watchdog fired before the app quit");
}
