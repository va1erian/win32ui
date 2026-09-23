//! The extended title bar's frame: `TitleBar::Extended` + `Backdrop::Mica`
//! builds, and the client content stays on the solid theme background (never
//! the black "glass" colour that leaks everywhere when the frame is not
//! extended). Screen capture is used because `PrintWindow` does not reproduce
//! the DWM-drawn frame, caption buttons or backdrop.
//!
//! The caption buttons and the material itself are machine-dependent (DWM
//! version, high-contrast, transparency effects) and time-sensitive to
//! capture, so they are checked in the demo's committed screenshots, not here.
//! The non-client geometry and hit-testing stay unit-tested in `sys::nc`.
//!
//! Window-creating tests use a watchdog timer so failures fail instead of
//! hanging.

#![cfg(windows)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use win32ui::prelude::*;

/// How a screen-capture run ended: whether the watchdog fired (the capture
/// timer never fired) and the captured image, if any.
struct CaptureRun {
    timed_out: bool,
    image: Option<RgbaImage>,
}

enum Msg {
    Capture,
}

struct CaptureApp {
    image: Rc<RefCell<Option<Result<RgbaImage>>>>,
}

impl win32ui::App for CaptureApp {
    type Msg = Msg;
    fn update(&mut self, msg: Msg, ui: &mut Ui<Msg>) {
        let Msg::Capture = msg;
        *self.image.borrow_mut() = Some(ui.capture_screen());
        ui.quit();
    }
}

/// Runs `spec` and captures its screen rectangle once the window has been shown
/// long enough for DWM's appear animation to settle, under a watchdog.
fn run_capture(spec: WindowSpec) -> CaptureRun {
    let image = Rc::new(RefCell::new(None));
    let timed_out = Rc::new(Cell::new(false));
    let image_for_app = Rc::clone(&image);
    let timed_out_for_timer = Rc::clone(&timed_out);

    let result = win32ui::run_app(spec, move |ui| {
        let capture = ui.set_timer(600).ok();
        let watchdog = ui.set_timer(5000).ok();
        ui.on_timer(move |id| {
            if Some(id) == capture {
                Some(Msg::Capture)
            } else if Some(id) == watchdog {
                timed_out_for_timer.set(true);
                win32ui::quit(1);
                None
            } else {
                None
            }
        });
        CaptureApp {
            image: image_for_app,
        }
    });

    let image = match result {
        Ok(()) => image.borrow_mut().take().and_then(|captured| captured.ok()),
        Err(_) => None,
    };
    CaptureRun {
        timed_out: timed_out.get(),
        image,
    }
}

#[test]
fn extended_mica_content_is_not_black() {
    let run = run_capture(
        WindowSpec::new("extended.frame.mica")
            .theme(Theme::light())
            .backdrop(Backdrop::Mica)
            .title_bar(TitleBar::Extended),
    );

    assert!(
        !run.timed_out,
        "the watchdog fired before the capture timer"
    );
    let Some(image) = run.image else {
        return;
    };

    // The client below the strip is the solid light background, never the black
    // DWM "glass" colour that leaks when the frame is not extended.
    let background = Theme::light().background;
    let mid = image.pixel(image.width / 2, image.height / 2);
    assert_eq!(
        mid,
        Some([background.r, background.g, background.b, 0xFF]),
        "the content area must stay on the theme background"
    );

    // DWM draws the caption buttons in the top-right of the extended strip, so
    // that region is neither the flat theme background (no buttons) nor flat
    // black (no material, or the black "glass" leak).
    let top_right = Rect::new(
        image.width.saturating_sub(200) as i32,
        0,
        image.width as i32,
        64,
    );
    assert!(
        region_is_not_flat_black_or_background(&image, top_right, background),
        "the caption buttons/material must be drawn in the top-right strip"
    );
}

/// Whether `region` holds drawn content — it has a pixel that is not the flat
/// theme background (so something is drawn there) and a pixel that is not pure
/// black (so it is not a black "glass" leak).
fn region_is_not_flat_black_or_background(
    image: &RgbaImage,
    region: Rect,
    background: Color,
) -> bool {
    let left = region.left.clamp(0, image.width as i32) as u32;
    let right = region.right.clamp(0, image.width as i32) as u32;
    let top = region.top.clamp(0, image.height as i32) as u32;
    let bottom = region.bottom.clamp(0, image.height as i32) as u32;
    let mut has_non_background = false;
    let mut has_non_black = false;
    for y in (top..bottom).step_by(4) {
        for x in (left..right).step_by(4) {
            if let Some([r, g, b, _]) = image.pixel(x, y) {
                has_non_background |= [r, g, b] != [background.r, background.g, background.b];
                has_non_black |= [r, g, b] != [0, 0, 0];
                if has_non_background && has_non_black {
                    return true;
                }
            }
        }
    }
    false
}

/// The default spec (no backdrop, standard title bar) must be untouched: the
/// screen capture still reports the theme background in the content area.
#[test]
fn default_spec_content_is_the_theme_background() {
    let run = run_capture(WindowSpec::new("extended.frame.default").theme(Theme::light()));

    assert!(
        !run.timed_out,
        "the watchdog fired before the capture timer"
    );
    let Some(image) = run.image else {
        return;
    };
    let background = Theme::light().background;
    assert_eq!(
        image.pixel(image.width / 2, image.height / 2),
        Some([background.r, background.g, background.b, 0xFF]),
        "the default spec must keep the theme background"
    );
}

/// `Ui::title_bar_height` reports a non-zero reserved strip for an extended
/// title bar (so the demo layout starts below the caption buttons and menu).
#[test]
fn title_bar_height_reports_the_reserved_strip() {
    struct App {
        recorded: Rc<Cell<bool>>,
    }

    impl win32ui::App for App {
        type Msg = ();
        fn update(&mut self, _msg: (), ui: &mut Ui<()>) {
            self.recorded.set(ui.title_bar_height().value() > 0.0);
            ui.quit();
        }
    }

    let recorded = Rc::new(Cell::new(false));
    let recorded_for_make = Rc::clone(&recorded);
    let timed_out = Rc::new(Cell::new(false));
    let timed_out_for_timer = Rc::clone(&timed_out);

    let result = win32ui::run_app(
        WindowSpec::new("extended.frame.height")
            .title_bar(TitleBar::Extended)
            .theme(Theme::light()),
        move |ui| {
            let watchdog = ui.set_timer(5000).ok();
            ui.on_timer(move |id| {
                if Some(id) == watchdog {
                    timed_out_for_timer.set(true);
                    win32ui::quit(1);
                }
                None
            });
            ui.emit(());
            App {
                recorded: recorded_for_make,
            }
        },
    );

    assert!(result.is_ok(), "run_app failed");
    assert!(!timed_out.get(), "the watchdog fired before the app quit");
    assert!(
        recorded.get(),
        "an extended title bar must report a non-zero strip height"
    );
}
