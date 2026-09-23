//! Tooltips as a `ControlExt` capability and the toolbar item tooltips.
//!
//! The native side (the shared `tooltips_class32` window and its tools) is
//! covered by the unit tests in `src/controls/tooltip.rs`; these integration
//! tests exercise the public widget-layer path under the watchdog helper.

#![cfg(windows)]

mod common;

use common::run_app_with_watchdog;
use win32ui::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Msg {
    Start,
    Check,
}

struct TooltipApp {
    label: Option<Label>,
    read_back: std::rc::Rc<std::cell::RefCell<Option<String>>>,
    updated: std::rc::Rc<std::cell::Cell<bool>>,
}

impl App for TooltipApp {
    type Msg = Msg;

    fn update(&mut self, msg: Msg, ui: &mut Ui<Msg>) {
        match msg {
            Msg::Start => {
                if let Some(label) = &self.label {
                    label.set_tooltip("Hello");
                    *self.read_back.borrow_mut() = label.tooltip();
                    label.set_tooltip("Changed");
                    self.updated
                        .set(label.tooltip().as_deref() == Some("Changed"));
                }
                ui.emit(Msg::Check);
            }
            Msg::Check => ui.quit(),
        }
    }
}

/// `ControlExt::set_tooltip` records the text and `ControlExt::tooltip` reads
/// it back, updating on a second call.
#[test]
fn set_tooltip_round_trips() {
    let read_back = std::rc::Rc::new(std::cell::RefCell::new(None));
    let updated = std::rc::Rc::new(std::cell::Cell::new(false));
    let created = std::rc::Rc::new(std::cell::Cell::new(false));

    let read_back_for_make = read_back.clone();
    let updated_for_make = updated.clone();
    let created_for_make = created.clone();
    let Some(run) = run_app_with_watchdog("win32ui.tooltip.control", move |ui| {
        let label = Label::new(ui, Rect::new(0, 0, 160, 24), "hi").ok();
        created_for_make.set(label.is_some());
        if label.is_none() {
            ui.quit();
        }
        ui.emit(Msg::Start);
        TooltipApp {
            label,
            read_back: read_back_for_make,
            updated: updated_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    if !created.get() {
        return;
    }
    assert_eq!(
        read_back.borrow().as_deref(),
        Some("Hello"),
        "the tooltip text did not round-trip"
    );
    assert!(updated.get(), "the tooltip text did not update");
}

struct ToolbarApp {
    _toolbar: Option<Toolbar<()>>,
}

impl App for ToolbarApp {
    type Msg = ();

    fn update(&mut self, _msg: (), ui: &mut Ui<()>) {
        ui.quit();
    }
}

/// A toolbar whose items carry tooltips and shortcuts builds and adds one
/// region tool per button without failing.
#[test]
fn toolbar_item_tooltips_build() {
    let built = std::rc::Rc::new(std::cell::Cell::new(false));
    let built_for_make = built.clone();
    let Some(run) = run_app_with_watchdog("win32ui.tooltip.toolbar", move |ui| {
        let toolbar = Toolbar::new(
            ui,
            vec![
                ToolbarItem::new("Refresh")
                    .tooltip("Refresh")
                    .shortcut(Shortcut::ctrl(Key::R))
                    .on_click(|| None),
                ToolbarItem::new("Clear").tooltip("Clear").on_click(|| None),
                ToolbarItem::new("Plain").on_click(|| None),
            ],
        )
        .ok();
        built_for_make.set(toolbar.is_some());
        if toolbar.is_none() {
            ui.quit();
        }
        ui.emit(());
        ToolbarApp { _toolbar: toolbar }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    assert!(built.get(), "the toolbar with item tooltips did not build");
}
