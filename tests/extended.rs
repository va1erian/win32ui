//! The extended title bar: `TitleBar::Extended` builds, reports a caption inset
//! and accepts caption-interactive widgets.
//!
//! The non-client geometry and the hit-test decision are pure and unit-tested in
//! `sys::nc`; the behaviour that needs a real desktop (drag, snap layouts,
//! moving between monitors) is on the PR's manual checklist.
//!
//! Window-creating tests use the shared watchdog helper so failures fail
//! instead of hanging.

#![cfg(windows)]

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::run_app_spec_with_watchdog;
use win32ui::prelude::*;

#[test]
fn extended_title_bar_builds_and_reports_an_inset() {
    struct App {
        checked: Rc<Cell<bool>>,
    }

    impl win32ui::App for App {
        type Msg = ();
        fn update(&mut self, _msg: (), ui: &mut Ui<()>) {
            // DWM may not have reported the buttons yet, so the inset can be
            // empty; it must never be negative or panic.
            let inset = ui.caption_inset();
            self.checked
                .set(inset.right.value() >= 0.0 && inset.left.value() == 0.0);
            ui.quit();
        }
    }

    let checked = Rc::new(Cell::new(false));
    let checked_for_make = Rc::clone(&checked);
    let Some(run) = run_app_spec_with_watchdog(
        WindowSpec::new("extended.titlebar").title_bar(TitleBar::Extended),
        move |ui| {
            ui.emit(());
            App {
                checked: checked_for_make,
            }
        },
    ) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    assert!(checked.get(), "caption_inset returned an unexpected value");
}

/// A widget marked caption-interactive builds and the window still runs; the
/// hit-test routing itself is unit-tested in `sys::nc`.
#[test]
fn caption_interactive_widget_builds() {
    struct App;

    impl win32ui::App for App {
        type Msg = ();
        fn update(&mut self, _msg: (), ui: &mut Ui<()>) {
            ui.quit();
        }
    }

    let Some(run) = run_app_spec_with_watchdog(
        WindowSpec::new("extended.interactive").title_bar(TitleBar::Extended),
        move |ui| {
            if let Ok(button) = Button::new(ui, "Back") {
                button.set_caption_interactive(true);
            }
            ui.emit(());
            App
        },
    ) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
}

/// Resizing an extended window moves its client edge by exactly the same
/// amount, straight away. `WM_NCCALCSIZE` once computed the client from the
/// *previous* window rectangle, so the layout ran one resize behind: short of
/// the edge after growing, past it after shrinking.
#[test]
fn extended_client_follows_every_resize() {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER, SetWindowPos,
    };

    struct App {
        deltas: Rc<Cell<Option<(i32, i32)>>>,
    }

    fn resize(ui: &Ui<()>, width: i32, height: i32) {
        let raw = HWND(ui.hwnd().raw() as *mut core::ffi::c_void);
        // SAFETY: `raw` is this test's live window; only integer geometry and
        // documented flags are passed.
        unsafe {
            let _ = SetWindowPos(
                raw,
                None,
                0,
                0,
                width,
                height,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    impl win32ui::App for App {
        type Msg = ();
        fn update(&mut self, _msg: (), ui: &mut Ui<()>) {
            let outer = ui.window_rect();
            let start = ui.client_rect().width();
            resize(ui, outer.width() + 200, outer.height());
            let grown = ui.client_rect().width();
            resize(ui, outer.width() - 100, outer.height());
            let shrunk = ui.client_rect().width();
            self.deltas.set(Some((grown - start, shrunk - start)));
            ui.quit();
        }
    }

    let deltas = Rc::new(Cell::new(None));
    let deltas_for_make = Rc::clone(&deltas);
    let Some(run) = run_app_spec_with_watchdog(
        WindowSpec::new("extended.resize")
            .size(Dip(600.0), Dip(400.0))
            .title_bar(TitleBar::Extended),
        move |ui| {
            ui.emit(());
            App {
                deltas: deltas_for_make,
            }
        },
    ) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    assert_eq!(
        deltas.get(),
        Some((200, -100)),
        "the client width must follow each resize exactly"
    );
}
