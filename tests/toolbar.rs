//! The toolbar's public widget-layer API under the watchdog helper: per-item
//! typed state, separators, spacers, named and caller-supplied icons and the
//! label modes build and re-theme without a window hanging.

#![cfg(windows)]

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::run_app_with_watchdog;
use win32ui::d2d::{PathBuilder, PointF};
use win32ui::prelude::*;

enum Msg {
    Start,
}

struct ToolbarApp {
    _toolbar: Option<Toolbar<Msg>>,
}

impl App for ToolbarApp {
    type Msg = Msg;

    fn update(&mut self, _msg: Msg, ui: &mut Ui<Msg>) {
        if let Some(toolbar) = &self._toolbar {
            // Per-item typed state, addressed by id.
            toolbar.set_enabled(4u32, false);
            toolbar.set_enabled(4u32, true);
            toolbar.set_checked(5u32, true);
            // Live theme switching, light and dark.
            toolbar.apply_theme(&Theme::light());
            toolbar.apply_theme(&Theme::dark());
        }
        ui.quit();
    }
}

/// Builds a triangle path the app supplies as a crisp vector icon.
fn triangle() -> Option<std::rc::Rc<win32ui::d2d::Path>> {
    let mut builder = PathBuilder::new().ok()?;
    builder
        .move_to(PointF::new(8.0, 2.0))
        .line_to(PointF::new(14.0, 14.0))
        .line_to(PointF::new(2.0, 14.0))
        .close();
    builder.build().ok().map(Rc::new)
}

#[test]
fn the_toolbar_builds_with_state_groups_and_icons() {
    let built = Rc::new(Cell::new(false));
    let built_for_make = Rc::clone(&built);
    let Some(path) = triangle() else {
        return;
    };

    let Some(run) = run_app_with_watchdog("win32ui.toolbar.state", move |ui| {
        let toolbar = Toolbar::new(
            ui,
            vec![
                ToolbarItem::new("Reply").with_icon(ToolbarIcon::Reply),
                ToolbarItem::new("Forward").with_icon(ToolbarIcon::Forward),
                ToolbarItem::new("Archive")
                    .with_icon(ToolbarIcon::Archive)
                    .label_mode(LabelMode::IconOnly),
                ToolbarItem::new("Delete").id(4u32).enabled(false),
                ToolbarItem::new("Star")
                    .id(5u32)
                    .with_icon(ToolbarIcon::path(path))
                    .toggle()
                    .on_toggle(|_| None),
                Toolbar::separator(),
                Toolbar::spacer(),
                Toolbar::flexible_spacer(),
                ToolbarItem::new("Compose")
                    .with_icon(ToolbarIcon::Compose)
                    .label_mode(LabelMode::TextUnder),
                ToolbarItem::new("Settings")
                    .with_icon(ToolbarIcon::glyph('\u{E713}'))
                    .label_mode(LabelMode::IconOnly),
            ],
        )
        .ok();
        built_for_make.set(toolbar.is_some());
        if toolbar.is_none() {
            ui.quit();
        }
        ui.emit(Msg::Start);
        ToolbarApp { _toolbar: toolbar }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    assert!(built.get(), "the toolbar did not build");
}
