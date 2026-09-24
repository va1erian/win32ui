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

/// The composited (`wgc`) capture includes DWM chrome that `PrintWindow`
/// misses: with an acrylic backdrop and an extended title bar, the
/// caption-button region of the two images differs. Read-only — no focus or
/// pointer movement — and skipped when the platform does not draw the
/// material or either backend is unavailable.
#[cfg(feature = "wgc")]
#[test]
fn composited_capture_includes_dwm_caption_chrome() {
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Clone)]
    enum Msg {
        Go,
        Capture,
    }

    struct App {
        composited: Rc<RefCell<Option<RgbaImage>>>,
        printed: Rc<RefCell<Option<RgbaImage>>>,
        backdrop_active: Rc<Cell<bool>>,
    }

    impl win32ui::App for App {
        type Msg = Msg;

        fn update(&mut self, msg: Msg, ui: &mut Ui<Msg>) {
            if let Msg::Capture = msg {
                *self.composited.borrow_mut() = ui.capture_composited().ok();
                *self.printed.borrow_mut() = ui.capture().ok();
                self.backdrop_active.set(ui.backdrop_active());
                ui.quit();
            }
        }
    }

    let composited = Rc::new(RefCell::new(None));
    let printed = Rc::new(RefCell::new(None));
    let backdrop_active = Rc::new(Cell::new(false));
    let (c, p, a) = (
        Rc::clone(&composited),
        Rc::clone(&printed),
        Rc::clone(&backdrop_active),
    );

    let Some(run) = run_app_spec_with_watchdog(
        WindowSpec::new("capture.caption")
            .backdrop(Backdrop::Acrylic)
            .title_bar(TitleBar::Extended),
        move |ui| {
            // Let DWM compose the material and the caption before capturing.
            let id = ui.set_timer(600).ok();
            ui.on_timer(move |fired| (Some(fired) == id).then_some(Msg::Capture));
            ui.emit(Msg::Go);
            App {
                composited: c,
                printed: p,
                backdrop_active: a,
            }
        },
    ) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    if !backdrop_active.get() {
        // No material on this machine; the chrome comparison is not meaningful.
        return;
    }
    let (Some(composited), Some(printed)) = (composited.borrow().clone(), printed.borrow().clone())
    else {
        return;
    };

    // The two backends size the window differently (Windows.Graphics.Capture
    // reports the composited content, `PrintWindow` the whole frame), so
    // compare the caption-button strip anchored to each image's top-right.
    let strip_w = 160u32.min(composited.width).min(printed.width);
    let strip_h = 40u32.min(composited.height).min(printed.height);
    let mut differing = 0usize;
    for y in 0..strip_h {
        for x in 0..strip_w {
            let a = composited.pixel(composited.width - 1 - x, y);
            let b = printed.pixel(printed.width - 1 - x, y);
            if a != b {
                differing += 1;
            }
        }
    }
    assert!(
        differing > 0,
        "the composited capture must include DWM caption chrome PrintWindow misses"
    );
}
