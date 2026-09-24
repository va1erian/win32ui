//! `ScrollView`: a native vertical scrollbar over content taller than the
//! viewport. `scroll_to` clamps to the content extent, and the content is
//! re-parented into the viewport.

#![cfg(windows)]

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::run_app_with_watchdog;
use win32ui::Renderer;
use win32ui::Size;
use win32ui::column;
use win32ui::d2d::{D2dCanvas, RectF};
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

const CONTENT_FILL: Color = Color::rgb(0xC0, 0x20, 0x20);
const SIBLING_FILL: Color = Color::rgb(0x20, 0x20, 0xC0);
const SIBLING_HEIGHT_DIP: f32 = 80.0;

/// A Direct2D widget that fills its viewport with a solid colour — used as the
/// `ScrollView`'s content, and separately as a sibling shown next to it.
struct SolidWidget {
    color: Color,
    /// A tall preferred height, so the content genuinely exceeds the
    /// viewport (as `GridView`'s always-full-document content does).
    tall: bool,
}

impl CustomWidget for SolidWidget {
    type Event = ();

    fn paint(&self, _canvas: &Canvas, _bounds: Rect, _theme: &Theme) {}

    fn renderer(&self) -> Renderer {
        Renderer::Direct2D
    }

    fn paint_d2d(&self, canvas: &mut D2dCanvas<'_>, bounds: RectF, _theme: &Theme) {
        canvas.fill_rect(bounds, self.color);
    }

    fn preferred_size(&self, dpi: u32) -> Option<Size> {
        self.tall.then(|| {
            Size::new(
                dip(200.0).to_px(dpi).value(),
                dip(600.0).to_px(dpi).value(),
            )
        })
    }
}

enum RelayoutMsg {
    ShowSibling,
    Capture,
}

struct RelayoutApp {
    view: ScrollView,
    content: Custom<SolidWidget, ()>,
    sibling: Custom<SolidWidget, ()>,
    captured: Rc<RefCell<Option<RgbaImage>>>,
}

impl App for RelayoutApp {
    type Msg = RelayoutMsg;

    fn update(&mut self, msg: RelayoutMsg, ui: &mut Ui<RelayoutMsg>) {
        match msg {
            // Mirrors the Albums view: a hidden sibling becomes visible and
            // `Ui::relayout` shrinks the `ScrollView`'s *height* only — its
            // width, scroll offset and content height are unchanged.
            RelayoutMsg::ShowSibling => {
                self.sibling.set_visible(true);
                ui.relayout();
            }
            // Give the invalidated region a chance to actually repaint
            // (`InvalidateRect` only schedules a `WM_PAINT`) before capturing.
            RelayoutMsg::Capture => {
                *self.captured.borrow_mut() = ui.capture().ok();
                ui.quit();
            }
        }
    }
}

/// Showing a sibling next to a `ScrollView` shrinks the view's height only —
/// the content's own bounds (position/size in its always-full-document
/// coordinate space) do not depend on viewport height, so a naive
/// bounds-unchanged check must not skip repainting the area the view gave up.
/// Regression test for the relayout repaint bug: a previously-hidden sibling
/// shown next to a `GridView`-style `ScrollView` used to keep showing the
/// scroll content's stale pixels instead of the sibling's own.
#[test]
fn scroll_view_repaints_area_a_shrinking_sibling_gives_up() {
    use std::cell::RefCell;
    use win32ui::RgbaImage;

    let captured = Rc::new(RefCell::new(None));
    let captured_for_make = Rc::clone(&captured);

    let Some(run) = run_app_with_watchdog("win32ui.scrollview.relayout", move |ui| {
        let view = ScrollView::new(ui).expect("scroll view");
        let content = Custom::new(
            ui,
            SolidWidget {
                color: CONTENT_FILL,
                tall: true,
            },
        )
        .expect("content");
        view.set_content(&content);

        let sibling = Custom::new(
            ui,
            SolidWidget {
                color: SIBLING_FILL,
                tall: false,
            },
        )
        .expect("sibling");
        sibling.set_visible(false);

        ui.set_layout(column![view.fill(1), sibling.height(dip(SIBLING_HEIGHT_DIP))]);

        let tick = ui.set_timer(50).ok();
        let watchdog = ui.set_timer(5000).ok();
        let step = Rc::new(Cell::new(0u8));
        ui.on_timer(move |id| {
            if id == watchdog {
                win32ui::quit(1);
                return None;
            }
            if Some(id) != tick {
                return None;
            }
            let current = step.get();
            step.set(current + 1);
            match current {
                0 => Some(RelayoutMsg::ShowSibling),
                1 => Some(RelayoutMsg::Capture),
                _ => None,
            }
        });

        RelayoutApp {
            view,
            content,
            sibling,
            captured: captured_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the capture");
    let image = captured
        .borrow_mut()
        .take()
        .expect("update never captured or Ui::capture failed");

    // A point well inside the band the sibling now occupies (the bottom of
    // the window, where the view used to extend) must show the sibling's own
    // fill, not the scroll content's stale pixels left over from when the
    // view was taller.
    let x = image.width / 2;
    let y = image.height - 10;
    let pixel = image.pixel(x, y).expect("sample point is in bounds");
    assert_eq!(
        pixel,
        [SIBLING_FILL.r, SIBLING_FILL.g, SIBLING_FILL.b, 0xFF],
        "the area the ScrollView gave up to its sibling still shows the \
         scroll content's stale pixels instead of the sibling's own fill"
    );
}
