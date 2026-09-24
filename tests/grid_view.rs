//! `GridView` behaviour that needs a real window: the model/selection round
//! trip and tile-size clamping. The virtualization-range and keyboard
//! wrap-around arithmetic are pure functions, unit-tested alongside them in
//! `src/controls/grid_view/layout.rs`.

#![cfg(windows)]

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::run_app_with_watchdog;
use win32ui::prelude::*;

struct Tile(u32);

enum Msg {
    Start,
}

struct GridApp;

impl App for GridApp {
    type Msg = Msg;

    fn update(&mut self, msg: Msg, ui: &mut Ui<Msg>) {
        let Msg::Start = msg;
        ui.quit();
    }
}

#[test]
fn selection_round_trips_and_clears_out_of_range() {
    let checks = Rc::new(Cell::new((None::<Option<usize>>, None::<Option<usize>>)));
    let checks_for_make = Rc::clone(&checks);

    let Some(run) = run_app_with_watchdog("win32ui.grid_view.selection", move |ui| {
        let grid = GridView::<Tile, Msg>::new(ui).expect("grid").content(
            |tile: &Tile, _canvas, _rect, _state| {
                let _ = tile.0;
            },
        );
        grid.set_model(vec![Tile(0), Tile(1), Tile(2)]);

        grid.set_selected(Some(1));
        let selected = grid.selected();

        grid.set_model(vec![Tile(0)]); // shrinks past the old selection
        let after_shrink = grid.selected();

        checks_for_make.set((Some(selected), Some(after_shrink)));
        ui.emit(Msg::Start);
        GridApp
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    let (selected, after_shrink) = checks.get();
    assert_eq!(selected, Some(Some(1)));
    assert_eq!(after_shrink, Some(None));
}

#[test]
fn tile_size_clamps_to_the_given_range() {
    let observed = Rc::new(Cell::new(None::<f32>));
    let observed_for_make = Rc::clone(&observed);

    let Some(run) = run_app_with_watchdog("win32ui.grid_view.tile_size", move |ui| {
        let grid = GridView::<Tile, Msg>::new(ui)
            .expect("grid")
            .tile_size(dip(100.0)..dip(200.0));

        grid.set_tile_size(dip(1000.0)); // above the range
        observed_for_make.set(Some(grid.current_tile_size().value()));

        ui.emit(Msg::Start);
        GridApp
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    assert_eq!(observed.get(), Some(200.0));
}
