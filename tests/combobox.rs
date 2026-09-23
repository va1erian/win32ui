//! ComboBox behaviour that needs a real window: the label/value mapping, the
//! UTF-16 item round trip and selection preservation across `set_items`.

#![cfg(windows)]

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::run_app_with_watchdog;
use win32ui::prelude::*;

enum Msg {
    Start,
}

struct MappedApp {
    _combo: Option<ComboBox<u32, Msg>>,
}

impl App for MappedApp {
    type Msg = Msg;

    fn update(&mut self, msg: Msg, ui: &mut Ui<Msg>) {
        let Msg::Start = msg;
        ui.quit();
    }
}

#[test]
fn items_and_values_round_trip() {
    let passed = Rc::new(Cell::new(false));
    let created = Rc::new(Cell::new(false));

    let passed_for_make = Rc::clone(&passed);
    let created_for_make = Rc::clone(&created);
    let Some(run) = run_app_with_watchdog("win32ui.combo", move |ui| {
        let mut combo = ComboBox::new(ui, [("Alpha", 10u32), ("Beta", 20), ("Gamma", 30)]).ok();

        if let Some(combo) = &mut combo {
            let mut ok = true;

            // Index <-> value mapping.
            ok &= combo.len() == 3;
            ok &= !combo.is_empty();
            ok &= combo.selected().is_none();
            combo.set_selected(&20);
            ok &= combo.selected() == Some(&20);
            ok &= combo.selected_index() == Some(1);
            ok &= combo.label(2) == Some("Gamma");
            ok &= combo.label(9).is_none();

            // Selecting a value that is not present clears the selection.
            combo.set_selected(&99);
            ok &= combo.selected().is_none();

            // UTF-16 labels survive the native round trip.
            combo.set_items([("日本語", 1u32), ("🎵 emoji", 2), ("e\u{301}", 3)]);
            ok &= combo.item_text(0) == "日本語";
            ok &= combo.item_text(1) == "🎵 emoji";
            ok &= combo.item_text(2) == "e\u{301}";

            // A selected value survives set_items when it is still present,
            // and is cleared when it is gone.
            combo.set_selected(&2);
            combo.set_items([("New", 5u32), ("Kept", 2), ("Other", 6)]);
            ok &= combo.selected() == Some(&2);
            combo.set_items([("Only", 5u32)]);
            ok &= combo.selected().is_none();

            // A live theme switch leaves the mapping intact.
            ui.set_theme(Theme::dark());
            combo.set_selected(&5);
            ok &= combo.selected() == Some(&5);

            passed_for_make.set(ok);
        }

        if combo.is_none() {
            ui.quit();
        } else {
            created_for_make.set(true);
            ui.emit(Msg::Start);
        }
        MappedApp { _combo: combo }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    if !created.get() {
        return;
    }
    assert!(passed.get(), "the combo box item/value mapping was wrong");
}
