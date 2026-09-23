//! Headless smoke test: creates real windows and controls and checks the safe
//! wrapper's bookkeeping. If the CI session cannot create windows at all, the
//! test skips rather than fails.

#![cfg(windows)]

mod common;

use common::{NullHandler, TestRows, run_with_watchdog};
use win32ui::prelude::*;

struct TestTree;

impl TreeSource for TestTree {
    fn children(&self, parent: Option<i64>) -> Vec<TreeEntry> {
        match parent {
            None => vec![
                TreeEntry::branch("Music", 1),
                TreeEntry::leaf("Playlists", 2),
            ],
            Some(1) => vec![TreeEntry::leaf("Rock", 11), TreeEntry::leaf("Jazz", 12)],
            _ => Vec::new(),
        }
    }
}

#[test]
fn window_with_controls_round_trips() {
    win32ui::init();

    let theme = Theme::light();
    let Ok(class) = WindowClass::register("win32ui.smoke", theme.background) else {
        return;
    };
    let Ok(window) = Window::create(
        class,
        None,
        WindowStyle::overlapped(),
        WindowExStyle::new(),
        Rect::new(0, 0, 640, 480),
        "smoke",
        NullHandler,
    ) else {
        return;
    };

    let Ok(tree) = TreeView::new(
        window.hwnd(),
        1,
        Rect::new(0, 0, 200, 400),
        Box::new(TestTree),
        96,
    ) else {
        return;
    };
    assert_eq!(tree.node_count(), 2);

    let Ok(list) = ListView::new(
        window.hwnd(),
        2,
        Rect::new(200, 0, 640, 400),
        &[
            Column::new("Title", dip(160.0)),
            Column::right("Time", dip(60.0)),
        ],
        Box::new(TestRows),
        ListViewTheme::from_theme(&theme),
        96,
    ) else {
        return;
    };
    assert_eq!(list.selected(), None);
    list.select(2);
    assert_eq!(list.selected(), Some(2));
    list.set_playing(Some(3));
    list.set_sort_indicator(1, SortDirection::Ascending);

    drop(list);
    drop(tree);
    window.destroy();
    assert!(!window.is_alive());
}

/// The timer's id must survive the round trip: `WM_TIMER` has to report the
/// id the handler gave `SetTimer`, otherwise a handler can never match it.
///
/// The helper's slower "watchdog" timer makes the test fail rather than hang if
/// the 50 ms timer never fires; the two ids differing also shows that starting
/// a second timer no longer replaces the first.
#[test]
fn timer_id_round_trips() {
    use std::cell::Cell;
    use std::rc::Rc;

    struct TimerHandler {
        under_test: Rc<Cell<Option<TimerId>>>,
        fired: Rc<Cell<bool>>,
    }

    impl WindowHandler for TimerHandler {
        fn message(&self, window: &Window, message: Message) -> Option<LResult> {
            match message {
                Message::Create => {
                    self.under_test.set(window.set_timer(50).ok());
                }
                Message::Timer { id } if Some(id) == self.under_test.get() => {
                    self.fired.set(true);
                    window.destroy();
                    win32ui::quit(0);
                }
                _ => {}
            }
            None
        }
    }

    let under_test = Rc::new(Cell::new(None));
    let fired = Rc::new(Cell::new(false));
    let Some(run) = run_with_watchdog("win32ui.timer", || TimerHandler {
        under_test: Rc::clone(&under_test),
        fired: Rc::clone(&fired),
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the 50 ms timer");
    assert!(under_test.get().is_some(), "the timer was not started");
    assert_ne!(
        under_test.get(),
        run.watchdog,
        "two timers on one window got the same id"
    );
    assert!(fired.get(), "WM_TIMER never reported the SetTimer id");
}
