//! Split layout node: the divider reserves space and the panes tile the parent.
//!
//! The pure arithmetic lives in `src/app/layout/tests.rs`; this exercises the
//! whole path through a real window, so a broken binding (the divider child
//! window) would show up as the wrong pane geometry.

#![cfg(windows)]

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::run_app_with_watchdog;
use win32ui::column;
use win32ui::prelude::*;

struct SplitApp {
    left: Label,
    right: Label,
    geometry_ok: Rc<Cell<bool>>,
}

impl App for SplitApp {
    type Msg = ();

    fn update(&mut self, _msg: (), ui: &mut Ui<()>) {
        let dpi = ui.dpi();
        let divider = dip(5.0).to_px(dpi).value();
        let expected = dip(150.0).to_px(dpi).value();
        let left = self.left.bounds();
        let right = self.right.bounds();
        let client = ui.client_rect();
        self.geometry_ok.set(
            left.width() == expected
                && right.left == left.right + divider
                && right.right == client.width(),
        );
        ui.quit();
    }
}

#[test]
fn split_layout_positions_both_panes_around_the_divider() {
    let geometry_ok = Rc::new(Cell::new(false));
    let ok_for_make = Rc::clone(&geometry_ok);

    let Some(run) = run_app_with_watchdog("win32ui.split", move |ui| {
        let left = Label::new(ui, Rect::default(), "left").expect("left");
        let right = Label::new(ui, Rect::default(), "right").expect("right");
        ui.set_layout(column![
            split_row![left, right]
                .position(dip(150.0))
                .min(dip(40.0), dip(40.0))
        ]);
        ui.emit(());
        SplitApp {
            left,
            right,
            geometry_ok: ok_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    assert!(
        geometry_ok.get(),
        "the split did not tile the parent around the divider"
    );
}
