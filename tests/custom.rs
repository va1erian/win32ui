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
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::{GetDC, GetPixel, ReleaseDC};
use windows::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_KEYDOWN, WM_PAINT};

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

/// A widget that claims `Down`/`PageDown` in
/// [`CustomWidget::key`] — as a list moving its focused row would — and leaves
/// every other key to the scroll host.
struct NavWidget {
    claimed: Rc<Cell<u32>>,
}

impl CustomWidget for NavWidget {
    type Event = ();

    fn paint(&self, _canvas: &Canvas, _bounds: Rect, _theme: &Theme) {}

    fn key(&self, key: Key, _modifiers: Modifiers, _cx: &mut WidgetCx<()>) -> KeyResult {
        match key {
            Key::DOWN | Key::PAGE_DOWN => {
                self.claimed.set(self.claimed.get() + 1);
                KeyResult::Handled
            }
            _ => KeyResult::Ignored,
        }
    }
}

/// A widget that never claims a navigation key: the scroll host keeps them.
struct IgnoreWidget;

impl CustomWidget for IgnoreWidget {
    type Event = ();

    fn paint(&self, _canvas: &Canvas, _bounds: Rect, _theme: &Theme) {}
}

/// Delivers `WM_PAINT` to the widget's child window synchronously, so a test
/// can measure exactly one paint after invalidating.
fn send_paint(hwnd: Hwnd) {
    let window = HWND(hwnd.raw() as *mut core::ffi::c_void);
    // SAFETY: a synchronous `WM_PAINT` to the widget's own live window.
    unsafe {
        SendMessageW(window, WM_PAINT, Some(WPARAM(0)), Some(LPARAM(0)));
    }
}

/// A GDI widget that records the repaint rectangle (`rcPaint`) of every paint.
struct PaintRecorder {
    rects: Rc<RefCell<Vec<Rect>>>,
}

impl CustomWidget for PaintRecorder {
    type Event = ();

    fn paint(&self, canvas: &Canvas, _bounds: Rect, _theme: &Theme) {
        self.rects.borrow_mut().push(canvas.paint_rect());
    }
}

enum PaintMsg {
    Start,
}

struct PaintApp {
    widget: Custom<PaintRecorder, PaintMsg>,
    rects: Rc<RefCell<Vec<Rect>>>,
    full: Rc<Cell<Rect>>,
}

impl App for PaintApp {
    type Msg = PaintMsg;

    fn update(&mut self, _msg: PaintMsg, ui: &mut Ui<PaintMsg>) {
        let hwnd = self.widget.hwnd();
        // Clear the region left by creation with one full paint first.
        self.widget.invalidate();
        send_paint(hwnd);
        self.rects.borrow_mut().clear();

        let a = Rect::new(10, 10, 60, 40);
        let b = Rect::new(80, 30, 140, 90);
        self.widget.invalidate_rect(a);
        self.widget.invalidate_rect(b);
        send_paint(hwnd);

        self.widget.invalidate();
        send_paint(hwnd);
        let bounds = self.widget.bounds();
        self.full
            .set(Rect::new(0, 0, bounds.width(), bounds.height()));
        ui.quit();
    }
}

/// Two `invalidate_rect` calls coalesce into one paint whose `rcPaint` is their
/// bounding rectangle, and a full `invalidate` still repaints everything.
#[test]
fn invalidate_rect_coalesces_and_full_invalidate_repaints_everything() {
    let rects = Rc::new(RefCell::new(Vec::new()));
    let full = Rc::new(Cell::new(Rect::default()));
    let rects_for_make = Rc::clone(&rects);
    let full_for_make = Rc::clone(&full);

    let Some(run) = run_app_with_watchdog("win32ui.custom.rect", move |ui| {
        let widget = Custom::new(
            ui,
            PaintRecorder {
                rects: Rc::clone(&rects_for_make),
            },
        )
        .expect("paint recorder");
        ui.set_layout(column![widget.fill(1)]);
        ui.emit(PaintMsg::Start);
        PaintApp {
            widget,
            rects: rects_for_make,
            full: full_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    let rects = rects.borrow();
    assert_eq!(
        rects.len(),
        2,
        "expected the coalesced paint then the full one, got {rects:?}"
    );
    assert_eq!(
        rects[0],
        Rect::new(10, 10, 140, 90),
        "two invalidate_rect calls must coalesce into their bounding rectangle"
    );
    assert_eq!(
        rects[1],
        full.get(),
        "a full invalidate must repaint the whole client"
    );
}

/// A Direct2D widget that fills its viewport with a mutable colour.
struct PhaseWidget {
    color: Rc<Cell<Color>>,
}

impl CustomWidget for PhaseWidget {
    type Event = ();

    fn paint(&self, _canvas: &Canvas, _bounds: Rect, _theme: &Theme) {}

    fn renderer(&self) -> Renderer {
        Renderer::Direct2D
    }

    fn paint_d2d(&self, canvas: &mut D2dCanvas<'_>, bounds: RectF, _theme: &Theme) {
        canvas.fill_rect(bounds, self.color.get());
    }
}

const PHASE_A: Color = Color::rgb(0x20, 0x80, 0x40);
const PHASE_B: Color = Color::rgb(0xC0, 0x30, 0x30);

enum PhaseMsg {
    Start,
}

#[derive(Default)]
struct Pixels {
    inside: Cell<Option<u32>>,
    outside: Cell<Option<u32>>,
}

struct PhaseApp {
    widget: Custom<PhaseWidget, PhaseMsg>,
    color: Rc<Cell<Color>>,
    pixels: Rc<Pixels>,
}

impl App for PhaseApp {
    type Msg = PhaseMsg;

    fn update(&mut self, _msg: PhaseMsg, ui: &mut Ui<PhaseMsg>) {
        let hwnd = self.widget.hwnd();
        self.color.set(PHASE_A);
        self.widget.invalidate();
        send_paint(hwnd);

        // Change the colour but invalidate only a small rectangle: only those
        // pixels must change, the rest must keep the first frame's colour.
        self.color.set(PHASE_B);
        let dirty = Rect::new(40, 40, 140, 100);
        self.widget.invalidate_rect(dirty);
        send_paint(hwnd);

        // Read two pixels straight from the widget's DC: one inside the dirty
        // rectangle, one well outside it. `GetDC` does not repaint, unlike a
        // `PrintWindow` capture, so this sees the frame exactly as painted.
        let bounds = self.widget.bounds();
        let window = HWND(hwnd.raw() as *mut core::ffi::c_void);
        // SAFETY: `window` is the live child window; its DC is released below.
        unsafe {
            let dc = GetDC(Some(window));
            let inside = GetPixel(dc, 90, 70).0;
            let outside = GetPixel(dc, bounds.width() - 20, bounds.height() - 20).0;
            let _ = ReleaseDC(Some(window), dc);
            self.pixels.inside.set(Some(inside));
            self.pixels.outside.set(Some(outside));
        }
        ui.quit();
    }
}

/// In the Direct2D path a rect-scoped invalidation clips the frame: only the
/// dirty rectangle is repainted, and the rest of the surface keeps its pixels.
#[test]
fn direct2d_frame_clips_to_the_dirty_rect() {
    let color = Rc::new(Cell::new(PHASE_A));
    let pixels = Rc::new(Pixels::default());
    let color_for_make = Rc::clone(&color);
    let pixels_for_make = Rc::clone(&pixels);

    let Some(run) = run_app_with_watchdog("win32ui.custom.d2d.rect", move |ui| {
        let widget = Custom::new(
            ui,
            PhaseWidget {
                color: Rc::clone(&color_for_make),
            },
        )
        .expect("phase widget");
        ui.set_layout(column![widget.fill(1)]);
        ui.emit(PhaseMsg::Start);
        PhaseApp {
            widget,
            color: color_for_make,
            pixels: pixels_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    assert_eq!(
        pixels.inside.get(),
        Some(PHASE_B.to_colorref()),
        "the dirty rectangle must be repainted with the new colour"
    );
    assert_eq!(
        pixels.outside.get(),
        Some(PHASE_A.to_colorref()),
        "pixels outside the dirty rectangle must keep their previous colour"
    );
}

/// Sends `key` down to the widget's child window.
fn send_key_down(hwnd: Hwnd, key: Key) {
    let window = HWND(hwnd.raw() as *mut core::ffi::c_void);
    // SAFETY: a synchronous message to the widget's own live window.
    unsafe {
        SendMessageW(
            window,
            WM_KEYDOWN,
            Some(WPARAM(usize::from(key.code()))),
            Some(LPARAM(0)),
        );
    }
}

#[derive(Default)]
struct KeyChecks {
    nav_down: Cell<f32>,
    nav_page: Cell<f32>,
    nav_end: Cell<f32>,
    plain_down: Cell<f32>,
    claimed: Cell<u32>,
}

struct KeyApp {
    nav: Custom<NavWidget, ()>,
    plain: Custom<IgnoreWidget, ()>,
    claimed: Rc<Cell<u32>>,
    checks: Rc<KeyChecks>,
}

impl App for KeyApp {
    type Msg = ();

    fn update(&mut self, _msg: (), ui: &mut Ui<()>) {
        let nav = self.nav.hwnd();
        self.nav.scroll_to(dip(0.0));
        send_key_down(nav, Key::DOWN);
        self.checks.nav_down.set(self.nav.scroll_offset().value());
        send_key_down(nav, Key::PAGE_DOWN);
        self.checks.nav_page.set(self.nav.scroll_offset().value());
        send_key_down(nav, Key::END);
        self.checks.nav_end.set(self.nav.scroll_offset().value());
        self.checks.claimed.set(self.claimed.get());

        let plain = self.plain.hwnd();
        self.plain.scroll_to(dip(0.0));
        send_key_down(plain, Key::DOWN);
        self.checks
            .plain_down
            .set(self.plain.scroll_offset().value());

        ui.quit();
    }
}

/// A widget that claims `Down`/`PageDown` in `CustomWidget::key` stops the
/// scroll host from consuming them; a widget that ignores them still scrolls.
#[test]
fn custom_widget_can_claim_scroll_navigation_keys() {
    let claimed = Rc::new(Cell::new(0u32));
    let claimed_for_make = Rc::clone(&claimed);
    let checks = Rc::new(KeyChecks::default());
    let checks_for_make = Rc::clone(&checks);

    let Some(run) = run_app_with_watchdog("win32ui.custom.keys", move |ui| {
        let viewport = dip(300.0).to_px(ui.dpi()).value();
        let nav = Custom::new(
            ui,
            NavWidget {
                claimed: claimed_for_make,
            },
        )
        .expect("nav widget")
        .with_vscroll();
        nav.set_bounds(Rect::new(0, 0, viewport, viewport));
        nav.set_content_height(dip(5000.0));
        let plain = Custom::new(ui, IgnoreWidget)
            .expect("plain widget")
            .with_vscroll();
        plain.set_bounds(Rect::new(0, 0, viewport, viewport));
        plain.set_content_height(dip(5000.0));
        ui.emit(());
        KeyApp {
            nav,
            plain,
            claimed,
            checks: checks_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    assert_eq!(
        checks.claimed.get(),
        2,
        "the widget did not claim Down and PageDown"
    );
    assert_eq!(checks.nav_down.get(), 0.0, "a claimed Down must not scroll");
    assert_eq!(
        checks.nav_page.get(),
        0.0,
        "a claimed PageDown must not scroll"
    );
    assert!(
        (checks.nav_end.get() - dip(4700.0).value()).abs() < 1.0,
        "an ignored End should still scroll to the bottom (got {})",
        checks.nav_end.get()
    );
    assert!(
        (checks.plain_down.get() - dip(48.0).value()).abs() < 1.0,
        "a widget that ignores Down should scroll one line (got {})",
        checks.plain_down.get()
    );
}
