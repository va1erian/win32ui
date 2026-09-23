//! The progress bar's visual verification: build bars in each state, capture
//! the window and assert the theme's exact fill colours are on screen.
//!
//! This is the automated stand-in for the light/dark screenshots: because the
//! bar is owner-drawn from semantic tokens, a themed native control would not
//! produce these pixel values.

#![cfg(windows)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use win32ui::prelude::*;

enum Msg {
    Capture,
}

struct BarApp {
    /// Held so the bars stay alive for the run; dropped after the capture.
    _bars: Vec<ProgressBar>,
    image: Rc<RefCell<Option<Result<RgbaImage>>>>,
}

impl App for BarApp {
    type Msg = Msg;

    fn update(&mut self, _msg: Msg, ui: &mut Ui<Msg>) {
        *self.image.borrow_mut() = Some(ui.capture());
        ui.quit();
    }
}

/// The number of pixels exactly equal to `color`.
fn count(image: &RgbaImage, color: Color) -> usize {
    let expected = [color.r, color.g, color.b, 0xFF];
    (0..image.height)
        .flat_map(|y| (0..image.width).map(move |x| (x, y)))
        .filter(|&(x, y)| image.pixel(x, y) == Some(expected))
        .count()
}

/// Runs an app that builds three bars (normal, paused, error) in `theme` and
/// captures the window once it has painted. Returns `None` if windows cannot be
/// created in this session.
fn capture_states(theme: Theme, name: &str) -> Option<(RgbaImage, bool)> {
    win32ui::init();

    let timed_out = Rc::new(Cell::new(false));
    let image = Rc::new(RefCell::new(None));
    let timed_out_for_make = Rc::clone(&timed_out);
    let image_for_make = Rc::clone(&image);

    let result = win32ui::run_app(
        WindowSpec::new(name)
            .size(dip(380.0), dip(150.0))
            .theme(theme),
        move |ui| {
            let dpi = ui.dpi();
            let left = dip(10.0).to_px(dpi).value();
            let right = dip(370.0).to_px(dpi).value();
            let height = dip(24.0).to_px(dpi).value();
            let gap = dip(10.0).to_px(dpi).value();

            let mut bars = Vec::new();
            for (index, state) in [
                ProgressState::Normal,
                ProgressState::Paused,
                ProgressState::Error,
            ]
            .into_iter()
            .enumerate()
            {
                if let Ok(bar) = ProgressBar::new(ui) {
                    let bar = bar.range(0..=100).value(60).state(state);
                    let top = left + index as i32 * (height + gap);
                    bar.set_bounds(Rect::new(left, top, right, top + height));
                    bars.push(bar);
                }
            }

            let capture = ui.set_timer(200).ok();
            let watchdog = ui.set_timer(5000).ok();
            ui.on_timer(move |id| {
                if Some(id) == watchdog {
                    timed_out_for_make.set(true);
                    win32ui::quit(1);
                }
                if Some(id) == capture {
                    Some(Msg::Capture)
                } else {
                    None
                }
            });

            BarApp {
                _bars: bars,
                image: image_for_make,
            }
        },
    );

    result.ok()?;
    let captured = image.borrow_mut().take()?;
    let image = captured.ok()?;
    Some((image, timed_out.get()))
}

#[test]
fn progress_bar_paints_its_states_in_light() {
    let Some((image, timed_out)) = capture_states(Theme::light(), "win32ui.progressbar.light")
    else {
        return;
    };
    assert!(!timed_out, "the watchdog fired before the capture");
    let theme = Theme::light();
    assert!(
        count(&image, theme.accent) > 200,
        "the normal fill (accent) was not painted"
    );
    assert!(
        count(&image, theme.warning) > 200,
        "the paused fill (warning) was not painted"
    );
    assert!(
        count(&image, theme.danger) > 200,
        "the error fill (danger) was not painted"
    );
}

#[test]
fn progress_bar_paints_its_states_in_dark() {
    let Some((image, timed_out)) = capture_states(Theme::dark(), "win32ui.progressbar.dark") else {
        return;
    };
    assert!(!timed_out, "the watchdog fired before the capture");
    let theme = Theme::dark();
    assert!(
        count(&image, theme.accent) > 200,
        "the dark normal fill (accent) was not painted"
    );
    assert!(
        count(&image, theme.warning) > 200,
        "the dark paused fill (warning) was not painted"
    );
    assert!(
        count(&image, theme.danger) > 200,
        "the dark error fill (danger) was not painted"
    );
}
