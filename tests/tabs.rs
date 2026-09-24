//! Tabs layout node: a native tab control pages two layout subtrees.
//!
//! The pure tree-to-rects arithmetic lives in `src/app/layout/tests.rs`; this
//! exercises the whole path through a real window, so a broken binding (the
//! native `SysTabControl32`) shows up as the wrong page geometry or visibility.

#![cfg(windows)]

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::run_app_with_watchdog;
use win32ui::prelude::*;
use win32ui::{column, tabs};

struct TabsApp {
    general: Label,
    accounts: Label,
    geometry_ok: Rc<Cell<bool>>,
}

/// A bounds instance whose selection and visibility are driven at runtime.
struct RuntimeTabsApp {
    tabs: Tabs,
    general: Label,
    accounts: Label,
    checks: Rc<Cell<u8>>,
}

impl App for RuntimeTabsApp {
    type Msg = ();

    fn update(&mut self, _msg: (), ui: &mut Ui<()>) {
        let mut checks = 0u8;

        // Runtime selection repages to the second page.
        self.tabs.set_selected(1);
        if self.tabs.selected() == 1 && self.accounts.is_visible() && !self.general.is_visible() {
            checks |= 1;
        }

        // Hiding the strip takes the whole node out of layout and hides pages.
        self.tabs.set_visible(false);
        if !self.tabs.is_visible() && !self.general.is_visible() && !self.accounts.is_visible() {
            checks |= 2;
        }

        // Showing it again repages the still-selected second page.
        self.tabs.set_visible(true);
        if self.tabs.is_visible() && self.accounts.is_visible() && !self.general.is_visible() {
            checks |= 4;
        }

        self.checks.set(checks);
        ui.quit();
    }
}

impl App for TabsApp {
    type Msg = ();

    fn update(&mut self, _msg: (), ui: &mut Ui<()>) {
        let client = ui.client_rect();
        let general = self.general.bounds();
        // Only the selected page is shown; the hidden one takes no space and
        // cannot receive focus.
        let selected = self.general.is_visible() && !self.accounts.is_visible();
        // The page sits below the tab strip, inside the client area.
        let placed = general.top > 0
            && general.left >= 0
            && general.right <= client.width()
            && general.bottom <= client.height();
        self.geometry_ok.set(selected && placed);
        ui.quit();
    }
}

#[test]
fn tabs_page_the_selected_layout_subtree() {
    let geometry_ok = Rc::new(Cell::new(false));
    let ok_for_make = Rc::clone(&geometry_ok);

    let Some(run) = run_app_with_watchdog("win32ui.tabs", move |ui| {
        let general = Label::new(ui, Rect::default(), "General page").expect("general");
        let accounts = Label::new(ui, Rect::default(), "Accounts page").expect("accounts");
        let tabs = tabs![
            ("General", column![general.fill(1)]),
            ("Accounts", column![accounts.fill(1)]),
        ]
        .on_change(|_index| None::<()>);
        ui.set_layout(column![tabs]);
        ui.emit(());
        TabsApp {
            general,
            accounts,
            geometry_ok: ok_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    assert!(
        geometry_ok.get(),
        "the tabs node did not page its layout subtrees"
    );
}

#[test]
fn tabs_selection_and_visibility_are_runtime_driven() {
    let checks = Rc::new(Cell::new(0u8));
    let checks_for_make = Rc::clone(&checks);

    let Some(run) = run_app_with_watchdog("win32ui.tabs.runtime", move |ui| {
        let general = Label::new(ui, Rect::default(), "General page").expect("general");
        let accounts = Label::new(ui, Rect::default(), "Accounts page").expect("accounts");
        let tabs = tabs![
            ("General", column![general.fill(1)]),
            ("Accounts", column![accounts.fill(1)]),
        ]
        .on_change(|_index| None::<()>);
        ui.set_layout(column![tabs]);
        ui.emit(());
        RuntimeTabsApp {
            tabs,
            general,
            accounts,
            checks: checks_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    assert_eq!(
        checks.get(),
        0b111,
        "runtime selection or visibility did not repage the tab node"
    );
}
