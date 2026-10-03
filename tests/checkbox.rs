//! The owner-drawn check box: the theme accent fills the checked glyph, and
//! the widget's own check state drives toggles.
//!
//! Window-creating tests use the shared watchdog helper so failures fail
//! instead of hanging.

#![cfg(windows)]

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::run_app_with_watchdog;
use win32ui::prelude::*;

/// A colour no default theme token uses, so its pixels can only be the glyph.
const ACCENT: Color = Color::hex(0xE6_7E_22);

#[derive(Clone, Debug, PartialEq, Eq)]
enum Msg {
    Start,
    Toggled(bool),
}

/// What the test observed, read after the loop exits.
#[derive(Default)]
struct Seen {
    accent_checked: usize,
    accent_unchecked: usize,
    accent_after_click: usize,
    toggles: Vec<bool>,
}

struct CheckApp {
    check: CheckBox<Msg>,
    seen: Rc<RefCell<Seen>>,
}

fn accent_pixels(ui: &Ui<Msg>) -> usize {
    let shot = ui.capture().expect("capture");
    shot.pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[0] == ACCENT.r && p[1] == ACCENT.g && p[2] == ACCENT.b)
        .count()
}

impl App for CheckApp {
    type Msg = Msg;

    fn update(&mut self, msg: Msg, ui: &mut Ui<Msg>) {
        match msg {
            Msg::Start => {
                let mut seen = self.seen.borrow_mut();
                self.check.set_checked(true);
                seen.accent_checked = accent_pixels(ui);
                self.check.set_checked(false);
                seen.accent_unchecked = accent_pixels(ui);
                // Owner-drawn buttons keep no check state: the widget's own
                // state must flip on the click.
                self.check.click();
                seen.accent_after_click = accent_pixels(ui);
            }
            Msg::Toggled(on) => {
                let mut seen = self.seen.borrow_mut();
                seen.toggles.push(on);
                if seen.toggles.len() == 1 {
                    self.check.click();
                } else {
                    ui.quit();
                }
            }
        }
    }
}

/// A checked box paints the theme accent; an unchecked one does not, and each
/// click reports the flipped state.
#[test]
fn checked_box_is_filled_with_the_theme_accent() {
    let seen = Rc::new(RefCell::new(Seen::default()));
    let seen_for_make = Rc::clone(&seen);
    let Some(run) = run_app_with_watchdog("win32ui.checkbox.accent", move |ui| {
        let mut theme = Theme::dark();
        theme.accent = ACCENT;
        ui.set_theme(theme);
        let check = CheckBox::new(ui, "Zebra striping")
            .expect("check box")
            .on_toggle(|on| Some(Msg::Toggled(on)));
        check.set_bounds(Rect::new(10, 10, 200, 40));
        ui.emit(Msg::Start);
        CheckApp {
            check,
            seen: seen_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    let seen = seen.borrow();
    assert!(
        seen.accent_checked >= 25,
        "a checked box fills its glyph with the accent: {} pixels",
        seen.accent_checked
    );
    assert_eq!(seen.accent_unchecked, 0, "an unchecked box shows no accent");
    assert!(
        seen.accent_after_click >= 25,
        "a click checks the box and repaints it"
    );
    assert_eq!(
        seen.toggles,
        vec![true, false],
        "each click flips the state"
    );
}
