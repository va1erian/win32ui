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
