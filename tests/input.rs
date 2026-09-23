//! End-to-end checks that the input messages survive the raw-message dispatch
//! path. Pure decoding is unit-tested in `src/sys/message/input.rs`; these
//! tests only cover what needs a real window (modifier reading, coordinate
//! conversion, `TrackMouseEvent`). They enter the loop through `common`, so a
//! watchdog fails them instead of hanging.

#![cfg(windows)]

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::run_with_watchdog;
use win32ui::prelude::*;

// Raw codes from `winuser.h`; the tests cannot reach the `windows` crate.
const WM_KEYDOWN: u32 = 0x0100;
const VK_F5: usize = 0x74;

#[test]
fn key_down_round_trips_through_dispatch() {
    struct Handler {
        seen: Rc<Cell<Option<Key>>>,
    }

    impl WindowHandler for Handler {
        fn message(&self, window: &Window, message: Message) -> Option<LResult> {
            match message {
                Message::Create => {
                    // Queue the key for the loop to dispatch (posting during
                    // `WM_CREATE` is fine; sending is not, the window is not
                    // finished being created).
                    let _ = window.post_message(WM_KEYDOWN, VK_F5, 0);
                }
                Message::KeyDown {
                    key, system: false, ..
                } => {
                    self.seen.set(Some(key));
                    window.destroy();
                    win32ui::quit(0);
                }
                _ => {}
            }
            None
        }
    }

    let seen = Rc::new(Cell::new(None));
    let Some(run) = run_with_watchdog("win32ui.input.key", || Handler {
        seen: Rc::clone(&seen),
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the handler quit");
    assert_eq!(seen.get(), Some(Key::F5));
}

#[test]
fn track_mouse_leave_arms() {
    struct Handler {
        armed: Rc<Cell<bool>>,
        probe: Rc<Cell<Option<TimerId>>>,
    }

    impl WindowHandler for Handler {
        fn message(&self, window: &Window, message: Message) -> Option<LResult> {
            match message {
                Message::Create => {
                    self.armed.set(window.track_mouse_leave().is_ok());
                    self.probe.set(window.set_timer(10).ok());
                }
                Message::Timer { id } if Some(id) == self.probe.get() => {
                    window.destroy();
                    win32ui::quit(0);
                }
                _ => {}
            }
            None
        }
    }

    let armed = Rc::new(Cell::new(false));
    let probe = Rc::new(Cell::new(None));
    let Some(run) = run_with_watchdog("win32ui.input.leave", || Handler {
        armed: Rc::clone(&armed),
        probe: Rc::clone(&probe),
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the handler quit");
    assert!(armed.get(), "TrackMouseEvent failed");
}
