//! `ScrollView`: a native vertical scrollbar over content taller than the
//! viewport. `scroll_to` clamps to the content extent, and the content is
//! re-parented into the viewport.

#![cfg(windows)]

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::run_app_with_watchdog;
use win32ui::Size;
use win32ui::column;
use win32ui::gdi::Canvas;
use win32ui::prelude::*;

/// Tall, display-only content: `preferred_size` makes it several viewports high.
struct TallContent;

impl CustomWidget for TallContent {
    type Event = ();

    fn paint(&self, canvas: &Canvas, bounds: Rect, theme: &Theme) {
        canvas.fill_rect(bounds, theme.background);
    }

    fn preferred_size(&self, dpi: u32) -> Option<Size> {
        Some(Size::new(
            dip(200.0).to_px(dpi).value(),
            dip(1200.0).to_px(dpi).value(),
        ))
    }
}

struct ScrollApp {
    view: ScrollView,
    content: Custom<TallContent, ()>,
    checks: Rc<Cell<u8>>,
}

impl App for ScrollApp {
    type Msg = ();

    fn update(&mut self, _msg: (), ui: &mut Ui<()>) {
        let mut checks = 0u8;

        // Scrolling far past the end clamps to the content extent.
        self.view.scroll_to(Px(100_000));
        let bottom = self.view.scroll_offset().value();
        if bottom > 0 && bottom <= self.view.content_height().value() {
            checks |= 1;
        }

        // Scrolling above the top clamps to zero.
        self.view.scroll_to(Px(-50));
        if self.view.scroll_offset().value() == 0 {
            checks |= 2;
        }

        // Scrolling by a known amount lands exactly there.
        self.view.scroll_to(Px(30));
        if self.view.scroll_offset().value() == 30 {
            checks |= 4;
        }

        self.checks.set(checks);
        // Keep the content alive through the check; it is owned here.
        let _ = &self.content;
        ui.quit();
    }
}

#[test]
fn scroll_view_clamps_and_tracks_the_offset() {
    let checks = Rc::new(Cell::new(0u8));
    let checks_for_make = Rc::clone(&checks);

    let Some(run) = run_app_with_watchdog("win32ui.scrollview", move |ui| {
        let view = ScrollView::new(ui).expect("scroll view");
        let content = Custom::new(ui, TallContent).expect("content");
        view.set_content(&content);
        ui.set_layout(column![view.fill(1)]);
        ui.emit(());
        ScrollApp {
            view,
            content,
            checks: checks_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    assert_eq!(
        checks.get(),
        0b111,
        "scroll_to did not clamp to the content extent or track a known offset"
    );
}
