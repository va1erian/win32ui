//! Headless smoke test: creates real windows and controls and checks the safe
//! wrapper's bookkeeping. If the CI session cannot create windows at all, the
//! test skips rather than fails.

#![cfg(windows)]

use win32ui::prelude::*;

struct NullHandler;

impl WindowHandler for NullHandler {
    fn message(&mut self, _window: &Window, _message: Message) -> Option<LResult> {
        None
    }
}

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

struct TestRows;

impl ListSource for TestRows {
    fn item_count(&self) -> usize {
        5
    }

    fn text(&self, item: usize, column: usize) -> String {
        format!("{item}/{column}")
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
        &[Column::new("Title", 160), Column::right("Time", 60)],
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
/// A second, slow "watchdog" timer makes the test fail rather than hang if the
/// 50 ms timer never fires; the two ids differing also shows that starting a
/// second timer no longer replaces the first.
#[test]
fn timer_id_round_trips() {
    use std::cell::Cell;
    use std::rc::Rc;

    win32ui::init();

    struct TimerHandler {
        under_test: Rc<Cell<Option<TimerId>>>,
        watchdog: Rc<Cell<Option<TimerId>>>,
        fired: Rc<Cell<bool>>,
    }

    impl WindowHandler for TimerHandler {
        fn message(&mut self, window: &Window, message: Message) -> Option<LResult> {
            match message {
                Message::Create => {
                    self.under_test.set(window.set_timer(50).ok());
                    self.watchdog.set(window.set_timer(3000).ok());
                }
                Message::Timer { id } if Some(id) == self.under_test.get() => {
                    self.fired.set(true);
                    window.destroy();
                    win32ui::quit(0);
                }
                Message::Timer { id } if Some(id) == self.watchdog.get() => {
                    window.destroy();
                    win32ui::quit(0);
                }
                _ => {}
            }
            None
        }
    }

    let theme = Theme::light();
    let Ok(class) = WindowClass::register("win32ui.timer", theme.background) else {
        return;
    };
    let under_test = Rc::new(Cell::new(None));
    let watchdog = Rc::new(Cell::new(None));
    let fired = Rc::new(Cell::new(false));
    let Ok(window) = Window::create(
        class,
        None,
        WindowStyle::overlapped(),
        WindowExStyle::new(),
        Rect::new(0, 0, 320, 200),
        "timer",
        TimerHandler {
            under_test: Rc::clone(&under_test),
            watchdog: Rc::clone(&watchdog),
            fired: Rc::clone(&fired),
        },
    ) else {
        return;
    };

    window.show();
    win32ui::run();
    assert!(
        under_test.get().is_some() && watchdog.get().is_some(),
        "the timers were not started"
    );
    assert_ne!(
        under_test.get(),
        watchdog.get(),
        "two timers on one window got the same id"
    );
    assert!(fired.get(), "WM_TIMER never reported the SetTimer id");
}
