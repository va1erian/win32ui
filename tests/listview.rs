//! ListView behaviour that needs a real window: the UTF-16 owner-data path and
//! the background colour round trip.

#![cfg(windows)]

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::run_app_with_watchdog;
use win32ui::prelude::*;

/// Assorted text that has historically tripped up the ANSI/UTF-16 boundary:
/// CJK, emoji beyond the BMP, combining marks, RTL, flags and a long run.
const WEIRD: &[&str] = &[
    "日本語のアルバム",
    "🎵 Émoji 🎶 𝄞",
    "Ω≈ç√∫˜µ≤≥÷",
    "العربية − Ελληνικά − עברית",
    "e\u{301}\u{327} combining",
    "𝔘𝔫𝔦𝔠𝔬𝔡𝔢 𝕗𝕒𝕟𝕔𝕪",
    "🇫🇷🇯🇵 flags",
    "NUL-free\u{200b}zero-width",
];

struct UnicodeSource {
    long: String,
}

impl ListSource for UnicodeSource {
    fn item_count(&self) -> usize {
        WEIRD.len() + 1
    }

    fn text(&self, item: usize, column: usize) -> String {
        if item == WEIRD.len() {
            return if column == 0 {
                self.long.clone()
            } else {
                String::new()
            };
        }
        if column == 0 {
            WEIRD[item].to_string()
        } else {
            format!("{} / {column}", WEIRD[item])
        }
    }
}

struct Empty;

impl ListSource for Empty {
    fn item_count(&self) -> usize {
        0
    }

    fn text(&self, _item: usize, _column: usize) -> String {
        String::new()
    }
}

enum Msg {
    Start,
}

struct UnicodeApp;

impl App for UnicodeApp {
    type Msg = Msg;

    fn update(&mut self, msg: Msg, ui: &mut Ui<Msg>) {
        let Msg::Start = msg;
        ui.quit();
    }
}

#[test]
fn unicode_cell_text_round_trips() {
    let round_tripped = Rc::new(Cell::new(false));
    let created = Rc::new(Cell::new(false));

    let round_tripped_for_make = Rc::clone(&round_tripped);
    let created_for_make = Rc::clone(&created);
    let Some(run) = run_app_with_watchdog("win32ui.unicode", move |ui| {
        let long = "長".repeat(400);
        let list = ListView::new(
            ui,
            Rect::new(0, 0, 700, 500),
            &[
                Column::new("Title", dip(300.0)),
                Column::new("Artist", dip(200.0)),
            ],
            Box::new(UnicodeSource { long: long.clone() }),
            ListViewTheme::from_theme(&Theme::dark()),
        )
        .ok();
        if let Some(list) = &list {
            // Read back before the loop starts, matching the original unit
            // test's exercise of the UTF-16 owner-data path.
            let mut ok = true;
            for (index, expected) in WEIRD.iter().enumerate() {
                if &list.cell_text(index, 0) != expected {
                    ok = false;
                }
            }
            if list.cell_text(0, 1) != format!("{} / 1", WEIRD[0]) {
                ok = false;
            }
            if list.cell_text(WEIRD.len(), 0) != long {
                ok = false;
            }
            round_tripped_for_make.set(ok);
        }
        if list.is_none() {
            ui.quit();
        } else {
            created_for_make.set(true);
            ui.emit(Msg::Start);
        }
        UnicodeApp
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    if !created.get() {
        return;
    }
    assert!(round_tripped.get(), "UTF-16 cell text did not round-trip");
}

struct BackgroundApp {
    list: Option<ListView<Msg>>,
    matches: Rc<Cell<bool>>,
}

impl App for BackgroundApp {
    type Msg = Msg;

    fn update(&mut self, msg: Msg, ui: &mut Ui<Msg>) {
        let Msg::Start = msg;
        let Some(list) = &self.list else {
            return;
        };
        self.matches
            .set(list.background_color() == Theme::dark().background);
        ui.quit();
    }
}

#[test]
fn background_colour_is_applied() {
    let matches = Rc::new(Cell::new(false));
    let created = Rc::new(Cell::new(false));

    let matches_for_make = Rc::clone(&matches);
    let created_for_make = Rc::clone(&created);
    let Some(run) = run_app_with_watchdog("win32ui.listbg", move |ui| {
        let list = ListView::new(
            ui,
            Rect::new(0, 0, 200, 200),
            &[Column::new("A", dip(80.0))],
            Box::new(Empty),
            ListViewTheme::from_theme(&Theme::dark()),
        )
        .ok();
        if list.is_none() {
            ui.quit();
        } else {
            created_for_make.set(true);
            ui.emit(Msg::Start);
        }
        BackgroundApp {
            list,
            matches: matches_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    if !created.get() {
        return;
    }
    assert!(
        matches.get(),
        "the list view did not apply the theme background"
    );
}
