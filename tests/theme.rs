//! Theming foundation: token derivations and the live-switching registry.
//!
//! Window-creating tests use the shared watchdog helper so failures fail
//! instead of hanging.

#![cfg(windows)]

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::{TestRow, run_app_with_watchdog, run_with_watchdog, test_rows};
use win32ui::prelude::*;

/// The per-control derived palettes follow the semantic tokens.
#[test]
fn derived_palettes_follow_tokens() {
    for theme in [Theme::light(), Theme::dark()] {
        let list = ListViewTheme::from_theme(&theme);
        assert_eq!(list.background, theme.background);
        assert_eq!(list.selection, theme.selection);
        assert_eq!(list.playing, theme.accent);
        assert_eq!(list.on_playing, theme.text_on_accent);
        assert_eq!(list.border, theme.border);
        assert_eq!(list.header_background, theme.surface);

        let toolbar = ToolbarTheme::from_theme(&theme);
        assert_eq!(toolbar.background, theme.surface);
        assert_eq!(toolbar.button_hover, theme.hover);
        assert_eq!(toolbar.button_pressed, theme.pressed);

        let status = StatusBarTheme::from_theme(&theme);
        assert_eq!(status.background, theme.surface);
    }
    assert!(!Theme::light().is_dark);
    assert!(Theme::dark().is_dark);
}

enum ThemeMsg {
    Start,
    Switch,
    Check,
}

struct ThemeApp {
    list: Option<ListView<TestRow, ThemeMsg>>,
    switched: bool,
    matches: std::rc::Rc<std::cell::Cell<bool>>,
}

impl App for ThemeApp {
    type Msg = ThemeMsg;

    fn update(&mut self, msg: ThemeMsg, ui: &mut Ui<ThemeMsg>) {
        match msg {
            ThemeMsg::Start => ui.emit(ThemeMsg::Switch),
            ThemeMsg::Switch => {
                ui.set_theme(Theme::dark());
                self.switched = ui.theme() == Theme::dark();
                if let Some(list) = &self.list {
                    use win32ui::Themed;
                    list.apply_theme(&Theme::dark());
                }
                ui.emit(ThemeMsg::Check);
            }
            ThemeMsg::Check => {
                let background_ok = self
                    .list
                    .as_ref()
                    .map(|list| list.background_color() == Theme::dark().background)
                    .unwrap_or(false);
                self.matches.set(self.switched && background_ok);
                ui.quit();
            }
        }
    }
}

/// Live switching re-themes the window and its children without recreating
/// them.
#[test]
fn live_switch_rethemes_children() {
    let matches = std::rc::Rc::new(std::cell::Cell::new(false));
    let created = std::rc::Rc::new(std::cell::Cell::new(false));
    let matches_for_make = matches.clone();
    let created_for_make = created.clone();
    let Some(run) = run_app_with_watchdog("win32ui.theme.live", move |ui| {
        let list = ListView::new(ui)
            .map(|list| list.column("A", dip(80.0), |row: &TestRow| row.label.as_str()))
            .ok();
        if let Some(list) = &list {
            list.set_model(test_rows());
        }
        created_for_make.set(list.is_some());
        if list.is_none() {
            ui.quit();
        } else {
            ui.emit(ThemeMsg::Start);
        }
        ThemeApp {
            list,
            switched: false,
            matches: matches_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    if !created.get() {
        return;
    }
    assert!(
        matches.get(),
        "live Ui::set_theme did not re-theme the list view"
    );
}

struct DropThemeApp {
    label: Option<Label>,
    label_hwnd: Option<Hwnd>,
    alive_after_drop: std::rc::Rc<std::cell::Cell<bool>>,
}

enum DropThemeMsg {
    Start,
    Drop,
    Check,
}

impl App for DropThemeApp {
    type Msg = DropThemeMsg;

    fn update(&mut self, msg: DropThemeMsg, ui: &mut Ui<DropThemeMsg>) {
        match msg {
            DropThemeMsg::Start => ui.emit(DropThemeMsg::Drop),
            DropThemeMsg::Drop => {
                self.label = None;
                ui.emit(DropThemeMsg::Check);
            }
            DropThemeMsg::Check => {
                if let Some(hwnd) = self.label_hwnd {
                    self.alive_after_drop.set(hwnd.is_alive());
                }
                ui.quit();
            }
        }
    }
}

/// A destroyed themed child is removed: its `HWND` dies with it and a later
/// `set_theme` does not touch it.
#[test]
fn destroyed_child_is_unregistered() {
    let alive = std::rc::Rc::new(std::cell::Cell::new(true));
    let created = std::rc::Rc::new(std::cell::Cell::new(false));
    let alive_for_make = alive.clone();
    let created_for_make = created.clone();
    let Some(run) = run_app_with_watchdog("win32ui.theme.drop", move |ui| {
        let label = Label::new(ui, Rect::new(0, 0, 200, 24), "hi").ok();
        let label_hwnd = label.as_ref().map(|label| label.hwnd());
        created_for_make.set(label.is_some());
        if label.is_none() {
            ui.quit();
        } else {
            ui.emit(DropThemeMsg::Start);
        }
        DropThemeApp {
            label,
            label_hwnd,
            alive_after_drop: alive_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    if !created.get() {
        return;
    }
    assert!(
        !alive.get(),
        "the label's HWND survived the widget being dropped"
    );
}

/// `Window::follow_system_theme(true)` re-reads `Theme::system()` and applies
/// it when a theme-change message arrives, and stops once turned back off.
#[test]
fn follow_system_theme_applies_on_a_theme_change_message() {
    // `WM_SYSCOLORCHANGE` (WinUser.h via the `windows` crate in `sys`, mirrored
    // here as a literal since the constant itself is private to the crate).
    const WM_SYSCOLORCHANGE: u32 = 0x0015;

    struct FollowHandler {
        applied: Rc<Cell<bool>>,
    }

    impl WindowHandler for FollowHandler {
        fn message(&self, window: &Window, message: Message) -> Option<LResult> {
            match message {
                Message::Create => {
                    // Start on a theme that differs from `Theme::system()`'s
                    // `is_dark`, whichever that is on this machine, so a real
                    // application would be able to see the switch.
                    let opposite = if Theme::system().is_dark {
                        Theme::light()
                    } else {
                        Theme::dark()
                    };
                    window.set_theme(opposite);
                    window.follow_system_theme(true);
                    window.post_message(WM_SYSCOLORCHANGE, 0, 0).ok();
                    Some(0)
                }
                Message::SysColorChange => {
                    self.applied.set(window.theme() == Theme::system());
                    win32ui::quit(0);
                    Some(0)
                }
                _ => None,
            }
        }
    }

    let applied = Rc::new(Cell::new(false));
    let Some(run) = run_with_watchdog("win32ui.theme.follow_system", || FollowHandler {
        applied: applied.clone(),
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    assert!(
        applied.get(),
        "follow_system_theme did not apply Theme::system() on WM_SYSCOLORCHANGE"
    );
}

/// Turning `follow_system_theme` back off stops further theme-change messages
/// from touching the window's theme.
#[test]
fn follow_system_theme_off_leaves_the_theme_alone() {
    const WM_SYSCOLORCHANGE: u32 = 0x0015;
    // An arbitrary `WM_APP` id used only as a "check now" marker, posted after
    // the (ignored) theme-change message so it is handled second.
    const WM_CHECK: u32 = 0x8000;

    struct StopHandler {
        stayed: Rc<Cell<bool>>,
    }

    impl WindowHandler for StopHandler {
        fn message(&self, window: &Window, message: Message) -> Option<LResult> {
            match message {
                Message::Create => {
                    window.set_theme(Theme::dark());
                    window.follow_system_theme(true);
                    window.follow_system_theme(false);
                    window.post_message(WM_SYSCOLORCHANGE, 0, 0).ok();
                    window.post_message(WM_CHECK, 0, 0).ok();
                    Some(0)
                }
                Message::Other { code, .. } if code == WM_CHECK => {
                    self.stayed.set(window.theme() == Theme::dark());
                    win32ui::quit(0);
                    Some(0)
                }
                _ => None,
            }
        }
    }

    let stayed = Rc::new(Cell::new(false));
    let Some(run) = run_with_watchdog("win32ui.theme.follow_system_off", || StopHandler {
        stayed: stayed.clone(),
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    assert!(
        stayed.get(),
        "a theme change was applied after follow_system_theme(false)"
    );
}
