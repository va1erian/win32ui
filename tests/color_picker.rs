//! `ColorPicker`: an owner-drawn swatch that opens `ChooseColor`.
//!
//! The dialog itself is interactive, so this exercises the widget-layer shape:
//! it is created from `ui`, laid out, and its swatch colour can be read and set
//! from code (`on_change` fires from the dialog, not from `set_color`).

#![cfg(windows)]

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::run_app_with_watchdog;
use win32ui::column;
use win32ui::prelude::*;

struct PickerApp {
    picker: ColorPicker<()>,
    checks: Rc<Cell<u8>>,
}

impl App for PickerApp {
    type Msg = ();

    fn update(&mut self, _msg: (), ui: &mut Ui<()>) {
        let mut checks = 0u8;

        // The initial colour is the one handed to the constructor.
        if self.picker.color() == Color::hex(0x33_55_AA) {
            checks |= 1;
        }

        // `set_color` replaces it without raising the event.
        self.picker.set_color(Color::hex(0x12_34_56));
        if self.picker.color() == Color::hex(0x12_34_56) {
            checks |= 2;
        }

        // It is a real, laid-out widget.
        if self.picker.bounds().width() > 0 && self.picker.bounds().height() > 0 {
            checks |= 4;
        }

        self.checks.set(checks);
        ui.quit();
    }
}

#[test]
fn color_picker_holds_and_sets_its_colour() {
    let checks = Rc::new(Cell::new(0u8));
    let checks_for_make = Rc::clone(&checks);

    let Some(run) = run_app_with_watchdog("win32ui.colorpicker", move |ui| {
        let picker = ColorPicker::new(ui, Color::hex(0x33_55_AA))
            .expect("picker")
            .on_change(|_color| None::<()>);
        ui.set_layout(column![picker.height(dip(24.0))]);
        ui.emit(());
        PickerApp {
            picker,
            checks: checks_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    assert_eq!(
        checks.get(),
        0b111,
        "the colour picker did not keep or set its colour, or was not laid out"
    );
}
