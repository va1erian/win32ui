//! Layout-tree tests against real widgets: the tree positions the children and
//! `Ui::relayout` re-runs it when something changes.
//!
//! Pure tree-to-rects arithmetic is covered by the unit tests in
//! `src/app/layout.rs`; this exercises the window-owned path.

#![cfg(windows)]

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::run_app_with_watchdog;
use win32ui::prelude::*;
use win32ui::{column, row};

enum Msg {
    Start,
    Hide,
    Check,
}

struct LayoutApp {
    left: Option<Label>,
    right: Option<Label>,
    initial: Rc<RefCell<Vec<(Rect, Rect)>>>,
    after_hide: Rc<RefCell<Vec<(Rect, Rect)>>>,
}

impl App for LayoutApp {
    type Msg = Msg;

    fn update(&mut self, msg: Msg, ui: &mut Ui<Msg>) {
        let (Some(left), Some(right)) = (&self.left, &self.right) else {
            ui.quit();
            return;
        };
        match msg {
            Msg::Start => {
                self.initial
                    .borrow_mut()
                    .push((left.bounds(), right.bounds()));
                ui.emit(Msg::Hide);
            }
            Msg::Hide => {
                left.set_visible(false);
                ui.relayout();
                self.after_hide
                    .borrow_mut()
                    .push((left.bounds(), right.bounds()));
                ui.emit(Msg::Check);
            }
            Msg::Check => ui.quit(),
        }
    }
}

/// A row of a fixed-width and a filling widget: the tree must place them side
/// by side, and hiding the fixed one must give its space back.
#[test]
fn layout_positions_widgets_and_relayouts() {
    let client = Rc::new(Cell::new(Rect::default()));
    let initial = Rc::new(RefCell::new(Vec::new()));
    let after_hide = Rc::new(RefCell::new(Vec::new()));
    let created = Rc::new(Cell::new(false));

    let client_for_make = Rc::clone(&client);
    let initial_for_make = Rc::clone(&initial);
    let after_hide_for_make = Rc::clone(&after_hide);
    let created_for_make = Rc::clone(&created);

    let Some(run) = run_app_with_watchdog("win32ui.layout", move |ui| {
        let dpi = ui.dpi();
        client_for_make.set(ui.client_rect());
        let left = Label::new(ui, Rect::default(), "left").ok();
        let right = Label::new(ui, Rect::default(), "right").ok();

        if let (Some(left), Some(right)) = (&left, &right) {
            let fixed = dip(80.0).to_px(dpi).value();
            ui.set_layout(column![row![left.width(dip(80.0)), right.fill(1)].fill(1)]);
            assert_eq!(
                left.bounds().width(),
                fixed,
                "the fixed width was not honoured"
            );
            initial_for_make
                .borrow_mut()
                .push((left.bounds(), right.bounds()));
            created_for_make.set(true);
            ui.emit(Msg::Start);
        } else {
            ui.quit();
        }

        LayoutApp {
            left,
            right,
            initial: initial_for_make,
            after_hide: after_hide_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    if !created.get() {
        return;
    }

    let client = client.get();
    let initial = initial.borrow();
    let (left, right) = initial.last().expect("initial bounds");
    assert_eq!(left.left, 0);
    assert_eq!(left.top, 0);
    assert_eq!(left.right, right.left, "the row's items must be adjacent");
    assert_eq!(right.right, client.right, "the row must fill the width");
    drop(initial);

    let after_hide = after_hide.borrow();
    let (_, right) = after_hide.last().expect("bounds after hiding");
    assert_eq!(
        right.left, 0,
        "hiding the fixed widget did not reclaim its space"
    );
    assert_eq!(
        right.right, client.right,
        "the filling widget should now span the row"
    );
}
