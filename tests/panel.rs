//! `Panel` + `ScrollView`: a scrollable form of standard controls (#119).
//!
//! The point of the pair is that the controls parent to the *panel*, so moving
//! the panel to scroll it moves them too. This checks that a child's position on
//! screen follows the scroll offset, that the offset clamps to the content
//! extent, and that the panel is resized to that extent.

#![cfg(windows)]

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::run_app_with_watchdog;
use win32ui::column;
use win32ui::prelude::*;

struct FormApp {
    view: ScrollView,
    panel: Panel,
    field: Edit<()>,
    content_px: i32,
    checks: Rc<Cell<u8>>,
}

impl App for FormApp {
    type Msg = ();

    fn update(&mut self, _msg: (), _ui: &mut Ui<()>) {
        let mut checks = 0u8;

        // The panel is laid out to the scroll content extent, so the viewport
        // has a real range to scroll.
        if self.panel.client_rect().height() == self.content_px {
            checks |= 1;
        }

        // A child of the panel moves with the scroll offset: it is parented to
        // the panel, not to the top-level window.
        self.view.scroll_to(Px(0));
        let before = common::screen_rect(self.field.hwnd());
        self.view.scroll_to(Px(100));
        let after = common::screen_rect(self.field.hwnd());
        if let (Some(before), Some(after)) = (before, after)
            && after.top == before.top - 100
            && after.left == before.left
        {
            checks |= 2;
        }

        // The offset clamps to the content extent.
        if self.view.scroll_offset().value() == 100 {
            checks |= 4;
        }

        self.checks.set(checks);
        _ui.quit();
    }
}

#[test]
fn panel_scrolls_standard_controls() {
    let checks = Rc::new(Cell::new(0u8));
    let checks_for_make = Rc::clone(&checks);

    let Some(run) = run_app_with_watchdog("win32ui.panel", move |ui| {
        let content_px = dip(600.0).to_px(ui.dpi()).value();
        let view = ScrollView::new(ui).expect("scroll view");
        let panel = Panel::new(ui).expect("panel");
        let mut form = panel.ui(ui);
        let caption = Label::new(&mut form, Rect::default(), "Name").expect("caption");
        let field = Edit::single_line(&mut form).expect("field");
        panel.set_layout(
            column![caption.height(dip(20.0)), field.height(dip(26.0))].spacing(dip(8.0)),
        );
        view.set_content(&panel);
        view.set_content_height(Px(content_px));
        ui.set_layout(column![view.fill(1)]);
        ui.emit(());
        FormApp {
            view,
            panel,
            field,
            content_px,
            checks: checks_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    assert_eq!(
        checks.get(),
        0b111,
        "the panel was not laid out to the content extent, or its child did not follow the scroll"
    );
}
