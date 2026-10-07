//! Coalescable timers: they tick as ordinary `WM_TIMER`s on both the raw
//! `Window` and the widget-layer `Ui`, and reject an out-of-range tolerance.
//! A watchdog makes a stuck message loop fail instead of hang.

#![cfg(windows)]

mod common;

use std::cell::Cell;
use std::rc::Rc;

use win32ui::prelude::*;

/// A handler that starts a coalescable timer and quits on its first tick.
struct CoalescedProbe {
    timer: Rc<Cell<Option<TimerId>>>,
    ticked: Rc<Cell<bool>>,
    rejected: Rc<Cell<bool>>,
}

impl WindowHandler for CoalescedProbe {
    fn message(&self, window: &Window, message: Message) -> Option<LResult> {
        match message {
            Message::Create => {
                // Above `TIMERV_COALESCING_MAX` and not `TIMERV_NO_COALESCING`.
                self.rejected
                    .set(window.set_coalescable_timer(20, 0x7FFF_FFF6).is_err());
                self.timer.set(window.set_coalescable_timer(20, 10).ok());
                None
            }
            Message::Timer { id } if Some(id) == self.timer.get() => {
                self.ticked.set(true);
                window.kill_timer(id);
                window.destroy();
                win32ui::quit(0);
                Some(0)
            }
            _ => None,
        }
    }
}

/// `Window::set_coalescable_timer` delivers `Message::Timer` with its own id.
#[test]
fn window_coalescable_timer_ticks() {
    let timer = Rc::new(Cell::new(None));
    let ticked = Rc::new(Cell::new(false));
    let rejected = Rc::new(Cell::new(false));
    let probe = CoalescedProbe {
        timer: Rc::clone(&timer),
        ticked: Rc::clone(&ticked),
        rejected: Rc::clone(&rejected),
    };
    let Some(run) = common::run_with_watchdog("win32ui.timer.window", move || probe) else {
        return;
    };
    assert!(!run.timed_out, "the watchdog fired before the timer ticked");
    assert!(
        timer.get().is_some(),
        "the coalescable timer was not started"
    );
    assert_ne!(
        timer.get(),
        run.watchdog,
        "the timer reused the watchdog id"
    );
    assert!(ticked.get(), "the coalescable timer never ticked");
    assert!(rejected.get(), "an out-of-range tolerance was accepted");
}

struct TickApp {
    ticked: Rc<Cell<bool>>,
}

impl App for TickApp {
    type Msg = bool;

    fn update(&mut self, ours: bool, ui: &mut Ui<bool>) {
        self.ticked.set(ours);
        ui.quit();
    }
}

/// `Ui::set_coalescable_timer` ticks reach `Ui::on_timer`.
#[test]
fn ui_coalescable_timer_reaches_on_timer() {
    let ticked = Rc::new(Cell::new(false));
    let app_ticked = Rc::clone(&ticked);
    let Some(run) = common::run_app_with_watchdog("win32ui.timer.ui", move |ui| {
        let ours = ui.set_coalescable_timer(20, 10).ok();
        // This replaces the watchdog's mapping, so any other tick (the
        // watchdog's) ends the run as a failure instead.
        ui.on_timer(move |id| Some(Some(id) == ours));
        TickApp { ticked: app_ticked }
    }) else {
        return;
    };
    assert!(!run.timed_out, "the watchdog fired before the timer ticked");
    assert!(
        ticked.get(),
        "the coalescable timer's tick did not arrive first"
    );
}
