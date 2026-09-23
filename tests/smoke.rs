//! Headless smoke test: creates a real widget-layer window and checks the safe
//! wrapper's bookkeeping. If the CI session cannot create windows at all, the
//! test skips rather than fails.

#![cfg(windows)]

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{TestRows, run_app_with_watchdog, run_with_watchdog};
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

enum SmokeMsg {
    Start,
}

struct SmokeApp {
    tree: Option<TreeView<SmokeMsg>>,
    list: Option<ListView<SmokeMsg>>,
    status: Option<StatusBar>,
    toolbar: Option<Toolbar<SmokeMsg>>,
    label: Option<Label>,
    node_count: Rc<Cell<Option<i32>>>,
    selected: Rc<Cell<Option<Option<usize>>>>,
    label_text: Rc<RefCell<Option<String>>>,
    toolbar_height: Rc<Cell<Option<i32>>>,
}

impl App for SmokeApp {
    type Msg = SmokeMsg;

    fn update(&mut self, msg: SmokeMsg, ui: &mut Ui<SmokeMsg>) {
        let SmokeMsg::Start = msg;
        let (Some(tree), Some(list), Some(status), Some(label), Some(toolbar)) = (
            &self.tree,
            &self.list,
            &self.status,
            &self.label,
            &self.toolbar,
        ) else {
            return;
        };

        self.node_count.set(Some(tree.node_count()));
        list.select(2);
        self.selected.set(Some(list.selected()));
        list.set_playing(Some(3));
        list.set_sort_indicator(1, SortDirection::Ascending);
        status.set_text(0, "Ready");
        label.set_text("hello");
        self.label_text.replace(Some(label.text()));
        self.toolbar_height.set(Some(toolbar.height()));
        ui.quit();
    }
}

#[test]
fn window_with_controls_round_trips() {
    let node_count = Rc::new(Cell::new(None));
    let selected = Rc::new(Cell::new(None));
    let label_text = Rc::new(RefCell::new(None));
    let toolbar_height = Rc::new(Cell::new(None));
    let created = Rc::new(Cell::new(false));

    let node_count_for_make = Rc::clone(&node_count);
    let selected_for_make = Rc::clone(&selected);
    let label_text_for_make = Rc::clone(&label_text);
    let toolbar_height_for_make = Rc::clone(&toolbar_height);
    let created_for_make = Rc::clone(&created);

    let Some(run) = run_app_with_watchdog("win32ui.smoke", move |ui| {
        let theme = Theme::light();
        let toolbar = Toolbar::new(
            ui,
            vec![ToolbarItem::new("One")],
            ToolbarTheme::from_theme(&theme),
        )
        .ok();
        let tree = TreeView::new(ui, Rect::new(0, 0, 200, 400), Box::new(TestTree)).ok();
        let list = ListView::new(
            ui,
            Rect::new(200, 0, 640, 400),
            &[
                Column::new("Title", dip(160.0)),
                Column::right("Time", dip(60.0)),
            ],
            Box::new(TestRows),
            ListViewTheme::from_theme(&theme),
        )
        .ok();
        let status = StatusBar::new(ui, StatusBarTheme::from_theme(&theme)).ok();
        let label = Label::new(ui, Rect::default(), "hi").ok();

        if toolbar.is_none()
            || tree.is_none()
            || list.is_none()
            || status.is_none()
            || label.is_none()
        {
            ui.quit();
        } else {
            created_for_make.set(true);
            ui.emit(SmokeMsg::Start);
        }

        SmokeApp {
            tree,
            list,
            status,
            toolbar,
            label,
            node_count: node_count_for_make,
            selected: selected_for_make,
            label_text: label_text_for_make,
            toolbar_height: toolbar_height_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    if !created.get() {
        return;
    }
    assert_eq!(
        node_count.get(),
        Some(2),
        "the tree did not report its roots"
    );
    assert_eq!(
        selected.get(),
        Some(Some(2)),
        "the list did not select row 2"
    );
    assert_eq!(
        label_text.borrow().as_deref(),
        Some("hello"),
        "the label text did not round-trip"
    );
    assert!(
        toolbar_height.get().is_some_and(|height| height > 0),
        "the toolbar reported no height"
    );
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
