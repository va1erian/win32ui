//! The window backdrop material and themed caption: spec wiring, the
//! `backdrop_active` report, and that backdrop-aware widgets build.
//!
//! The material itself is a machine-dependent DWM feature (unsupported on
//! Windows 10, off in high-contrast mode), so these tests assert the fallback
//! contract rather than that Mica is on. The pure fallback rule is unit-tested
//! in `sys::dwm`.
//!
//! Window-creating tests use the shared watchdog helper so failures fail
//! instead of hanging.

#![cfg(windows)]

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::run_app_spec_with_watchdog;
use win32ui::prelude::*;

/// The default spec requests no material, so `backdrop_active` is `false`.
#[test]
fn default_spec_reports_no_backdrop() {
    struct App {
        active: Rc<Cell<bool>>,
    }

    impl win32ui::App for App {
        type Msg = ();
        fn update(&mut self, _msg: (), ui: &mut Ui<()>) {
            self.active.set(ui.backdrop_active());
            ui.quit();
        }
    }

    let active = Rc::new(Cell::new(true));
    let active_for_make = Rc::clone(&active);
    let Some(run) = run_app_spec_with_watchdog(WindowSpec::new("backdrop.none"), move |ui| {
        ui.emit(());
        App {
            active: active_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    assert!(!active.get(), "Backdrop::None must not report as active");
}

/// Requesting Mica and a themed caption builds and reports a bool. On a machine
/// that supports the material this exercises the active path; on one that does
/// not (Windows 10, high contrast, transparency off) it exercises the fallback.
#[test]
fn mica_and_colored_caption_build() {
    struct App;

    impl win32ui::App for App {
        type Msg = ();
        fn update(&mut self, _msg: (), ui: &mut Ui<()>) {
            ui.quit();
        }
    }

    let Some(run) = run_app_spec_with_watchdog(
        WindowSpec::new("backdrop.mica")
            .backdrop(Backdrop::Mica)
            .title_bar(TitleBar::Colored),
        move |ui| {
            let _ = ui.backdrop_active();
            ui.emit(());
            App
        },
    ) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
}
