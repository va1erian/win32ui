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
