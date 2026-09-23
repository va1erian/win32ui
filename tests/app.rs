//! Widget-layer tests: message ordering, the no-re-entrancy invariant, and
//! RAII destruction of widgets.

#![cfg(windows)]

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{TestRows, run_app_with_watchdog};
use win32ui::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Msg {
    Start,
    A,
    B,
    C,
}

struct OrderApp {
    log: Rc<RefCell<Vec<Msg>>>,
}

impl App for OrderApp {
    type Msg = Msg;

    fn update(&mut self, msg: Msg, ui: &mut Ui<Msg>) {
        self.log.borrow_mut().push(msg.clone());
        match msg {
            Msg::Start => {
                ui.emit(Msg::A);
                ui.emit(Msg::B);
                ui.emit(Msg::C);
            }
            Msg::C => ui.quit(),
            _ => {}
        }
    }
}

/// Messages enqueued in one `update` must be delivered in order after it
/// returns.
#[test]
fn messages_arrive_in_order() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let log_for_make = Rc::clone(&log);
    let Some(run) = run_app_with_watchdog("win32ui.app.order", move |ui| {
        ui.emit(Msg::Start);
        OrderApp { log: log_for_make }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    assert_eq!(
        *log.borrow(),
        vec![Msg::Start, Msg::A, Msg::B, Msg::C],
        "messages were reordered or dropped"
    );
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum SelMsg {
    Start,
    Select,
    Selected(usize),
}

struct SelectApp {
    list: Option<ListView<SelMsg>>,
    log: Rc<RefCell<Vec<SelMsg>>>,
    in_update: bool,
    reentered: Rc<Cell<bool>>,
}

impl App for SelectApp {
    type Msg = SelMsg;

    fn update(&mut self, msg: SelMsg, ui: &mut Ui<SelMsg>) {
        // `update` is `&mut self`, so a nested call could not even borrow; this
        // flag documents and verifies that the drain never re-enters.
        if self.in_update {
            self.reentered.set(true);
        }
        self.in_update = true;
        self.log.borrow_mut().push(msg.clone());
        match msg {
            SelMsg::Start => ui.emit(SelMsg::Select),
            SelMsg::Select => {
                if let Some(list) = &self.list {
                    // Synchronously fires LVN_ITEMCHANGED, which the widget maps
                    // to SelMsg::Selected — but only *after* this update returns.
                    list.select(0);
                }
            }
            SelMsg::Selected(_) => ui.quit(),
        }
        self.in_update = false;
    }
}

/// A `Msg` whose `update` calls `list.select(..)` produces a selection `Msg`
/// that arrives after that `update` returns, never nested.
#[test]
fn select_during_update_is_not_nested() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let reentered = Rc::new(Cell::new(false));
    let list_created = Rc::new(Cell::new(false));

    let log_for_make = Rc::clone(&log);
    let reentered_for_make = Rc::clone(&reentered);
    let list_created_for_make = Rc::clone(&list_created);
    let Some(run) = run_app_with_watchdog("win32ui.app.select", move |ui| {
        let list = ListView::new(
            ui,
            Rect::new(0, 0, 200, 200),
            &[Column::new("A", dip(80.0))],
            Box::new(TestRows),
        )
        .ok()
        .map(|list| list.on_select(|item| Some(SelMsg::Selected(item))));
        list_created_for_make.set(list.is_some());
        ui.emit(SelMsg::Start);
        SelectApp {
            list,
            log: log_for_make,
            in_update: false,
            reentered: reentered_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    if !list_created.get() {
        return;
    }
    assert!(!reentered.get(), "update was re-entered");
    assert_eq!(
        *log.borrow(),
        vec![SelMsg::Start, SelMsg::Select, SelMsg::Selected(0)],
        "the selection message did not arrive after Select returned"
    );
}

struct DropApp {
    list: Option<ListView<DropMsg>>,
    list_hwnd: Option<Hwnd>,
    list_alive_after_drop: Rc<Cell<bool>>,
}

enum DropMsg {
    Start,
    DropList,
    Check,
}

impl App for DropApp {
    type Msg = DropMsg;

    fn update(&mut self, msg: DropMsg, ui: &mut Ui<DropMsg>) {
        match msg {
            DropMsg::Start => ui.emit(DropMsg::DropList),
            DropMsg::DropList => {
                self.list = None;
                ui.emit(DropMsg::Check);
            }
            DropMsg::Check => {
                if let Some(hwnd) = self.list_hwnd {
                    self.list_alive_after_drop.set(hwnd.is_alive());
                }
                ui.quit();
            }
        }
    }
}

/// Dropping a widget destroys its `HWND` (checked while the parent window is
/// still alive, so it cannot be the parent's teardown doing it).
#[test]
fn dropping_a_widget_destroys_it() {
    let list_alive_after_drop = Rc::new(Cell::new(false));
    let created = Rc::new(Cell::new(false));

    let alive_for_make = Rc::clone(&list_alive_after_drop);
    let created_for_make = Rc::clone(&created);
    let Some(run) = run_app_with_watchdog("win32ui.app.drop", move |ui| {
        let list = ListView::new(
            ui,
            Rect::new(0, 0, 200, 200),
            &[Column::new("A", dip(80.0))],
            Box::new(TestRows),
        )
        .ok();
        let list_hwnd = list.as_ref().map(|list| list.hwnd());
        created_for_make.set(list.is_some());
        ui.emit(DropMsg::Start);
        DropApp {
            list,
            list_hwnd,
            list_alive_after_drop: alive_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    if !created.get() {
        return;
    }
    assert!(
        !list_alive_after_drop.get(),
        "the list view's HWND survived the widget being dropped"
    );
}
