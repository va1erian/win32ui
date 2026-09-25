//! Borderless fullscreen and idle cursor hiding.
//!
//! The tests create a real top-level window, enter fullscreen on the monitor it
//! is on, and check the geometry, the delivered events and the restored state.
//! A watchdog makes a stuck message loop fail instead of hang.

#![cfg(windows)]

mod common;

use std::cell::Cell;
use std::rc::Rc;

use win32ui::prelude::*;

/// A handler that quits on the first paint, so the helper returns promptly.
struct QuitOnPaint;

impl WindowHandler for QuitOnPaint {
    fn message(&self, _window: &Window, message: Message) -> Option<LResult> {
        if matches!(message, Message::Paint) {
            win32ui::quit(0);
            return Some(0);
        }
        None
    }
}

/// Entering fullscreen covers the monitor's full rectangle and leaving restores
/// the original bounds.
#[test]
fn fullscreen_covers_the_monitor_and_restores() {
    let Some(run) = common::run_with_watchdog("win32ui.fullscreen.cover", || QuitOnPaint) else {
        return;
    };
    assert!(!run.timed_out, "the watchdog fired before the app quit");

    let monitor = run.window.monitor().expect("the window's monitor");
    let before = run.window.window_rect();

    run.window
        .enter_fullscreen(&monitor)
        .expect("enter fullscreen");
    assert!(run.window.is_fullscreen(), "not marked fullscreen");
    assert_eq!(
        run.window.window_rect(),
        monitor.rect,
        "the fullscreen window does not cover the monitor"
    );

    run.window.leave_fullscreen().expect("leave fullscreen");
    assert!(!run.window.is_fullscreen(), "still marked fullscreen");
    assert_eq!(
        run.window.window_rect(),
        before,
        "the window bounds were not restored"
    );
}

/// A handler that enters fullscreen, posts Escape/double-click/deactivate, and
/// records which of them arrived before quitting.
struct EventProbe {
    entered: Cell<bool>,
    posted: Cell<bool>,
    escaped: Rc<Cell<bool>>,
    double_clicked: Rc<Cell<bool>>,
    deactivated: Rc<Cell<bool>>,
}

impl EventProbe {
    fn finish_if_all(&self, window: &Window) {
        if self.escaped.get() && self.double_clicked.get() && self.deactivated.get() {
            window.destroy();
            win32ui::quit(0);
        }
    }
}

impl WindowHandler for EventProbe {
    fn message(&self, window: &Window, message: Message) -> Option<LResult> {
        match message {
            Message::Paint => {
                if !self.entered.replace(true)
                    && let Some(monitor) = window.monitor()
                {
                    let _ = window.enter_fullscreen(&monitor);
                }
                if !self.posted.replace(true) {
                    let _ = window.post_message(WM_KEYDOWN, VK_ESCAPE as usize, 0);
                    let _ = window.post_message(WM_LBUTTONDBLCLK, 0, 0);
                    let _ = window.post_message(WM_ACTIVATE, 0, 0);
                }
                // Fall through so `DefWindowProcW` validates the paint; claiming
                // it would leave the window invalid and starve the timers.
                None
            }
            Message::KeyDown {
                key: Key::ESCAPE, ..
            } => {
                self.escaped.set(true);
                self.finish_if_all(window);
                Some(0)
            }
            Message::MouseDoubleClick { .. } => {
                self.double_clicked.set(true);
                self.finish_if_all(window);
                Some(0)
            }
            Message::Activate { active: false, .. } => {
                self.deactivated.set(true);
                self.finish_if_all(window);
                Some(0)
            }
            _ => None,
        }
    }
}

/// A fullscreen window still delivers Escape, double-click and focus-loss to
/// the app.
#[test]
fn fullscreen_delivers_escape_double_click_and_focus_loss() {
    let escaped = Rc::new(Cell::new(false));
    let double_clicked = Rc::new(Cell::new(false));
    let deactivated = Rc::new(Cell::new(false));
    let escaped_for_handler = Rc::clone(&escaped);
    let double_for_handler = Rc::clone(&double_clicked);
    let deactivated_for_handler = Rc::clone(&deactivated);

    let Some(run) = common::run_with_watchdog("win32ui.fullscreen.events", move || EventProbe {
        entered: Cell::new(false),
        posted: Cell::new(false),
        escaped: escaped_for_handler,
        double_clicked: double_for_handler,
        deactivated: deactivated_for_handler,
    }) else {
        return;
    };

    assert!(
        !run.timed_out,
        "the watchdog fired before every event arrived"
    );
    assert!(escaped.get(), "Escape was not delivered while fullscreen");
    assert!(
        double_clicked.get(),
        "a double-click was not delivered while fullscreen"
    );
    assert!(
        deactivated.get(),
        "the focus-loss event was not delivered while fullscreen"
    );
}

/// A handler that arms idle cursor hiding and, through `WM_SETCURSOR`, checks
/// that the cursor is suppressed only while idle.
struct CursorProbe {
    armed: Cell<bool>,
    phase: Cell<u8>,
    timer: Cell<Option<TimerId>>,
    set_cursor_seen: Rc<Cell<u32>>,
    before_idle: Rc<Cell<u32>>,
    after_move: Rc<Cell<u32>>,
}

impl CursorProbe {
    /// Posts a synthetic `WM_SETCURSOR`; the idle handler suppresses it while
    /// the cursor is hidden, so the window handler does not see it.
    fn post_set_cursor(window: &Window) {
        let _ = window.post_message(WM_SETCURSOR, 0, 1);
    }
}

impl WindowHandler for CursorProbe {
    fn message(&self, window: &Window, message: Message) -> Option<LResult> {
        match message {
            Message::Paint => {
                if !self.armed.replace(true) {
                    let _ = window.hide_cursor_when_idle(40);
                    // Before the idle period a `WM_SETCURSOR` reaches the app.
                    Self::post_set_cursor(window);
                    self.timer.set(window.set_timer(80).ok());
                }
                // Fall through so `DefWindowProcW` validates the paint; claiming
                // it would leave the window invalid and starve the timers.
                None
            }
            Message::SetCursor { .. } => {
                self.set_cursor_seen.set(self.set_cursor_seen.get() + 1);
                None
            }
            Message::Timer { id } if Some(id) == self.timer.get() => {
                self.phase.set(self.phase.get() + 1);
                if self.phase.get() == 1 {
                    // The idle period has passed: this `WM_SETCURSOR` is
                    // suppressed, a move shows the cursor again, and the next
                    // one reaches the app.
                    self.before_idle.set(self.set_cursor_seen.get());
                    Self::post_set_cursor(window);
                    let _ = window.post_message(WM_MOUSEMOVE, 0, 0);
                    Self::post_set_cursor(window);
                    self.timer.set(window.set_timer(60).ok());
                } else {
                    self.after_move.set(self.set_cursor_seen.get());
                    window.destroy();
                    win32ui::quit(0);
                }
                Some(0)
            }
            _ => None,
        }
    }
}

/// While the cursor is idle-hidden the window's `WM_SETCURSOR` is suppressed;
/// a mouse move restores it.
#[test]
fn idle_hiding_suppresses_then_restores_the_cursor() {
    let before_idle = Rc::new(Cell::new(0u32));
    let after_move = Rc::new(Cell::new(0u32));
    let seen = Rc::new(Cell::new(0u32));
    let seen_for_handler = Rc::clone(&seen);
    let before_for_handler = Rc::clone(&before_idle);
    let after_for_handler = Rc::clone(&after_move);

    let Some(run) = common::run_with_watchdog("win32ui.fullscreen.cursor", move || CursorProbe {
        armed: Cell::new(false),
        phase: Cell::new(0),
        timer: Cell::new(None),
        set_cursor_seen: seen_for_handler,
        before_idle: before_for_handler,
        after_move: after_for_handler,
    }) else {
        return;
    };
    assert!(
        !run.timed_out,
        "the watchdog fired before the cursor was checked"
    );

    assert_eq!(
        before_idle.get(),
        1,
        "WM_SETCURSOR was not delivered before the idle period"
    );
    assert_eq!(
        after_move.get(),
        2,
        "WM_SETCURSOR was not suppressed while idle and restored on the move"
    );
}

// Constants from `Winuser.h`/`Winnt.h`, used only to post the messages above.
const WM_ACTIVATE: u32 = 0x0006;
const WM_KEYDOWN: u32 = 0x0100;
const WM_LBUTTONDBLCLK: u32 = 0x0203;
const WM_MOUSEMOVE: u32 = 0x0200;
const WM_SETCURSOR: u32 = 0x0020;
const VK_ESCAPE: u16 = 0x1B;
