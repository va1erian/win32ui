//! Custom widgets: the event-mapping invariant, `preferred_size`, and the
//! Direct2D paint path.
//!
//! The event test mirrors `select_during_update_is_not_nested` in `tests/app.rs`:
//! an event raised from within `update` (here a `SetFocus` delivered by
//! `ControlExt::focus`) must map through `on_event` into the app's queue and be
//! delivered after that `update` returns — never re-entered.

#![cfg(windows)]

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::run_app_with_watchdog;
use win32ui::Renderer;
use win32ui::Size;
use win32ui::column;
use win32ui::d2d::{D2dCanvas, RectF};
use win32ui::gdi::Canvas;
use win32ui::prelude::*;

/// A widget that raises its event the moment it gains focus.
struct FocusWidget;

impl CustomWidget for FocusWidget {
    type Event = ();

    fn paint(&self, _canvas: &Canvas, _bounds: Rect, _theme: &Theme) {}

    fn input(&self, input: Input, cx: &mut WidgetCx<()>) {
        if let Input::SetFocus = input {
            cx.emit(());
        }
    }

    fn preferred_size(&self, dpi: u32) -> Option<Size> {
        Some(Size::new(
            dip(120.0).to_px(dpi).value(),
            dip(28.0).to_px(dpi).value(),
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum FocusMsg {
    Start,
    Focused,
}

struct FocusApp {
    custom: Option<Custom<FocusWidget, FocusMsg>>,
    log: Rc<RefCell<Vec<FocusMsg>>>,
    in_update: bool,
    reentered: Rc<Cell<bool>>,
}

impl App for FocusApp {
    type Msg = FocusMsg;

    fn update(&mut self, msg: FocusMsg, ui: &mut Ui<FocusMsg>) {
        if self.in_update {
            self.reentered.set(true);
        }
        self.in_update = true;
        self.log.borrow_mut().push(msg.clone());
        match msg {
            FocusMsg::Start => {
                // Synchronously delivers `WM_SETFOCUS`, which the widget maps to
                // `FocusMsg::Focused` — delivered only after this returns.
                if let Some(custom) = &self.custom {
                    custom.focus();
                }
            }
            FocusMsg::Focused => ui.quit(),
        }
        self.in_update = false;
    }
}

/// A widget event maps through `on_event` into the app's queue and arrives after
/// the `update` that triggered it returns.
#[test]
fn custom_widget_event_maps_to_msg_without_reentry() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let reentered = Rc::new(Cell::new(false));
    let created = Rc::new(Cell::new(false));

    let log_for_make = Rc::clone(&log);
    let reentered_for_make = Rc::clone(&reentered);
    let created_for_make = Rc::clone(&created);
    let Some(run) = run_app_with_watchdog("win32ui.custom.focus", move |ui| {
        let custom = Custom::new(ui, FocusWidget)
            .ok()
            .map(|custom| custom.on_event(|()| Some(FocusMsg::Focused)));
        created_for_make.set(custom.is_some());
        if custom.is_none() {
            ui.quit();
        }
        ui.emit(FocusMsg::Start);
        FocusApp {
            custom,
            log: log_for_make,
            in_update: false,
            reentered: reentered_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    if !created.get() {
        return;
    }
    assert!(!reentered.get(), "update was re-entered");
    assert_eq!(
        *log.borrow(),
        vec![FocusMsg::Start, FocusMsg::Focused],
        "the widget event did not map through the queue"
    );
}

/// `preferred_size` sets the widget's initial bounds, which a layout can then
/// use as its natural size.
#[test]
fn custom_widget_preferred_size_sets_initial_bounds() {
    struct BoundsApp {
        _custom: Option<Custom<FocusWidget, ()>>,
    }

    impl App for BoundsApp {
        type Msg = ();

        fn update(&mut self, _msg: (), ui: &mut Ui<()>) {
            ui.quit();
        }
    }

    let size_ok = Rc::new(Cell::new(false));
    let size_for_make = Rc::clone(&size_ok);
    let Some(run) = run_app_with_watchdog("win32ui.custom.bounds", move |ui| {
        let custom = Custom::new(ui, FocusWidget).ok();
        if let Some(custom) = &custom {
            let expected = dip(120.0).to_px(ui.dpi()).value();
            size_for_make.set(custom.bounds().width() == expected);
        }
        ui.emit(());
        BoundsApp { _custom: custom }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    assert!(
        size_ok.get(),
        "the widget's preferred width did not become its initial bounds"
    );
}

const D2D_FILL: Color = Color::rgb(0x40, 0x20, 0x80);

/// A widget that opts into Direct2D and fills its viewport with a solid colour.
struct D2dWidget;

impl CustomWidget for D2dWidget {
    type Event = ();

    fn paint(&self, _canvas: &Canvas, _bounds: Rect, _theme: &Theme) {}

    fn renderer(&self) -> Renderer {
        Renderer::Direct2D
    }

    fn paint_d2d(&self, canvas: &mut D2dCanvas<'_>, bounds: RectF, theme: &Theme) {
        canvas.clear(theme.background);
        canvas.fill_rounded_rect(bounds, bounds.pill_radius(), D2D_FILL);
    }
}

struct D2dApp {
    _widget: Custom<D2dWidget, ()>,
    image: Rc<RefCell<Option<RgbaImage>>>,
}

impl App for D2dApp {
    type Msg = ();

    fn update(&mut self, _msg: (), ui: &mut Ui<()>) {
        *self.image.borrow_mut() = ui.capture().ok();
        ui.quit();
    }
}

/// A widget that opts into [`Renderer::Direct2D`] paints its fill: a pixel in
/// the middle of the captured window is the widget's colour.
#[test]
fn direct2d_widget_paints_its_fill() {
    let image = Rc::new(RefCell::new(None));
    let image_for_make = Rc::clone(&image);
    let Some(run) = run_app_with_watchdog("win32ui.custom.d2d", move |ui| {
        let widget = Custom::new(ui, D2dWidget).expect("d2d widget");
        ui.set_layout(column![widget.fill(1)]);
        ui.emit(());
        D2dApp {
            _widget: widget,
            image: image_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the capture");
    let image = image
        .borrow_mut()
        .take()
        .expect("update never captured or Window::capture failed");
    let x = image.width / 2;
    let y = image.height / 2;
    let pixel = image.pixel(x, y).expect("centre is in bounds");
    assert_eq!(
        pixel,
        [D2D_FILL.r, D2D_FILL.g, D2D_FILL.b, 0xFF],
        "the Direct2D widget did not paint its fill"
    );
}

const SCROLL_BAND: Color = Color::rgb(0x00, 0xC0, 0x00);
const DOCUMENT_DIP: f32 = 5000.0;
const BAND_DIP: f32 = 12.0;

/// A scrollable Direct2D widget whose document has a marker band along its
/// top edge, so a stale translation is visible as the band leaving the viewport.
struct ScrollD2dWidget;

impl CustomWidget for ScrollD2dWidget {
    type Event = ();

    fn paint(&self, _canvas: &Canvas, _bounds: Rect, _theme: &Theme) {}

    fn renderer(&self) -> Renderer {
        Renderer::Direct2D
    }

    fn paint_d2d(&self, canvas: &mut D2dCanvas<'_>, bounds: RectF, theme: &Theme) {
        canvas.clear(theme.background);
        let document = RectF::new(0.0, 0.0, bounds.width(), DOCUMENT_DIP);
        canvas.fill_rect(document, D2D_FILL);
        canvas.fill_rect(RectF::new(0.0, 0.0, bounds.width(), BAND_DIP), SCROLL_BAND);
    }
}

#[derive(Clone, Copy)]
enum ScrollMsg {
    Tick,
}

struct ScrollD2dApp {
    widget: Custom<ScrollD2dWidget, ScrollMsg>,
    scrolled: Rc<RefCell<Option<RgbaImage>>>,
    top: Rc<RefCell<Option<RgbaImage>>>,
    step: Cell<u8>,
}

impl App for ScrollD2dApp {
    type Msg = ScrollMsg;

    fn update(&mut self, _msg: ScrollMsg, ui: &mut Ui<ScrollMsg>) {
        match self.step.get() {
            0 => {
                self.widget.scroll_to(dip(100.0));
                self.widget.invalidate();
                self.step.set(1);
            }
            1 => {
                *self.scrolled.borrow_mut() = ui.capture().ok();
                self.widget.scroll_to(dip(0.0));
                self.widget.invalidate();
                self.step.set(2);
            }
            _ => {
                *self.top.borrow_mut() = ui.capture().ok();
                ui.quit();
            }
        }
    }
}

/// How many pixels of `image` are exactly `color`.
fn count_color(image: &RgbaImage, color: Color) -> usize {
    image
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[0] == color.r && p[1] == color.g && p[2] == color.b)
        .count()
}

/// The scroll host reuses one Direct2D render target across frames, so it must
/// reset the canvas translation every paint. Painting at offset 100 and then
/// back at offset 0 must show the document's top band again; a stale
/// translation leaves the viewport blank at the top.
#[test]
fn direct2d_scroll_host_resets_translation_at_offset_zero() {
    let scrolled = Rc::new(RefCell::new(None));
    let top = Rc::new(RefCell::new(None));
    let timed_out = Rc::new(Cell::new(false));

    let scrolled_for_make = Rc::clone(&scrolled);
    let top_for_make = Rc::clone(&top);
    let timed_out_for_make = Rc::clone(&timed_out);
    let Some(_run) = run_app_with_watchdog("win32ui.custom.d2d.scroll", move |ui| {
        let widget = Custom::new(ui, ScrollD2dWidget)
            .expect("d2d scroll widget")
            .with_vscroll();
        widget.set_content_height(dip(DOCUMENT_DIP));
        ui.set_layout(column![widget.fill(1)]);

        let tick = ui.set_timer(200).ok();
        let watchdog = ui.set_timer(5000).ok();
        ui.on_timer(move |id| {
            if Some(id) == watchdog {
                timed_out_for_make.set(true);
                win32ui::quit(1);
            }
            (Some(id) == tick).then_some(ScrollMsg::Tick)
        });
        ui.emit(ScrollMsg::Tick);

        ScrollD2dApp {
            widget,
            scrolled: scrolled_for_make,
            top: top_for_make,
            step: Cell::new(0),
        }
    }) else {
        return;
    };

    assert!(!timed_out.get(), "the watchdog fired before the capture");
    let scrolled = scrolled
        .borrow_mut()
        .take()
        .expect("the offset-100 frame was never captured");
    let top = top
        .borrow_mut()
        .take()
        .expect("the offset-0 frame was never captured");

    assert_eq!(
        count_color(&scrolled, SCROLL_BAND),
        0,
        "the document's top band should be scrolled out of view at offset 100"
    );
    assert!(
        count_color(&top, SCROLL_BAND) > 100,
        "after returning to offset 0 the top band must be visible again; \
         a stale Direct2D translation left the viewport blank"
    );
}
