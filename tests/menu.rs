//! Menus (#16): a menu bar mapped to `Msg`, owner-drawn dark items and the
//! live theme switch.
//!
//! Window-creating tests use the shared watchdog helper so failures fail
//! instead of hanging.

#![cfg(windows)]

mod common;

use common::run_app_with_watchdog;
use win32ui::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq)]
enum MenuMsg {
    Start,
    Toggled,
    Done,
}

struct MenuApp;

impl App for MenuApp {
    type Msg = MenuMsg;

    fn update(&mut self, msg: MenuMsg, ui: &mut Ui<MenuMsg>) {
        match msg {
            MenuMsg::Start => ui.emit(MenuMsg::Toggled),
            // Switching the theme rebuilds the owner-drawn menu bar live.
            MenuMsg::Toggled => {
                ui.set_theme(Theme::dark());
                ui.emit(MenuMsg::Done);
            }
            MenuMsg::Done => ui.quit(),
        }
    }
}

/// Installing a menu bar (with a submenu, a checked and a disabled item) and
/// re-theming the window both complete and shut down cleanly.
#[test]
fn menu_bar_installs_and_rethemes() {
    let Some(run) = run_app_with_watchdog("win32ui.menu.bar", |ui| {
        let menu = Menu::new()
            .item("&Reset", Shortcut::ctrl(Key::R), || MenuMsg::Done)
            .separator()
            .checked_item("&Check", None, true, || MenuMsg::Done)
            .radio_item("&Radio", None, true, || MenuMsg::Done)
            .disabled_item("&Off", None, || MenuMsg::Done)
            .submenu("&More", Menu::new().item("&Deep", None, || MenuMsg::Done));
        ui.set_menu_bar(menu);
        ui.emit(MenuMsg::Start);
        MenuApp
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
}

/// An empty menu still installs and clears without panicking.
#[test]
fn empty_menu_is_fine() {
    struct Empty;
    impl App for Empty {
        type Msg = ();
        fn update(&mut self, _msg: (), ui: &mut Ui<()>) {
            ui.quit();
        }
    }

    let Some(run) = run_app_with_watchdog("win32ui.menu.empty", |ui| {
        ui.set_menu_bar(Menu::new());
        ui.emit(());
        Empty
    }) else {
        return;
    };
    assert!(!run.timed_out, "the watchdog fired before the app quit");
}
