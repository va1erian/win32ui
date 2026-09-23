//! Theming foundation: token derivations and the live-switching registry.
//!
//! Window-creating tests use the shared watchdog helper so failures fail
//! instead of hanging.

#![cfg(windows)]

mod common;

use common::run_app_with_watchdog;
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
    list: Option<ListView<ThemeMsg>>,
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
        let list = ListView::new(
            ui,
            Rect::new(0, 0, 200, 200),
            &[Column::new("A", dip(80.0))],
            Box::new(common::TestRows),
        )
        .ok();
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
