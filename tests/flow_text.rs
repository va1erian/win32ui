//! `FlowText` and the rich layout under it: span-to-range mapping, hit testing
//! across a wrapped line, the wrapped height, and the window behaviour (a link
//! click emits its message, plain text does not, and the cursor is a hand over
//! a link). The window tests use a watchdog and park the real pointer, because
//! they inject their own mouse messages.

#![cfg(windows)]

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use common::run_app_with_watchdog;
use win32ui::column;
use win32ui::d2d::{FontSpec, RectF, Span, TextSystem, pixels_to_dips};
use win32ui::prelude::*;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    GCLP_HCURSOR, GetClassLongPtrW, GetSystemMetrics, IDC_ARROW, IDC_HAND, LoadCursorW,
    SM_CXSCREEN, SM_CYSCREEN, SendMessageW, SetCursorPos, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEMOVE,
};
use windows::core::PCWSTR;

/// The font `FlowText` uses by default.
const FAMILY: &str = "Segoe UI, Arial, sans-serif";
const SIZE: f32 = 14.0;

fn system() -> TextSystem {
    TextSystem::new().expect("DirectWrite is available")
}

/// The centre of a rectangle in device-independent pixels.
fn centre(rect: RectF) -> (f32, f32) {
    (
        (rect.left + rect.right) / 2.0,
        (rect.top + rect.bottom) / 2.0,
    )
}

#[test]
fn spans_map_to_their_own_ranges() {
    let font = system().font(&FontSpec::new(FAMILY, 18.0)).expect("font");
    let spans = [Span::new("Hello "), Span::new("brave "), Span::new("world")];
    let layout = font.rich_layout(&spans, f32::INFINITY).expect("layout");

    assert_eq!(layout.span_count(), 3);
    assert_eq!(layout.span_text(0), "Hello ");
    assert_eq!(layout.span_text(2), "world");
    assert_eq!(layout.span_text(9), "");

    for (index, span) in spans.iter().enumerate() {
        let rects = layout.rects_of_span(index);
        assert_eq!(rects.len(), 1, "span {index} is on one line");
        let (x, y) = centre(rects[0]);
        let hit = layout.hit_test_point(x, y);
        assert_eq!(hit.span, index, "{} hit span {}", span.text, hit.span);
        assert!(hit.inside);
    }
}

#[test]
fn hit_testing_crosses_a_wrapped_line() {
    let font = system().font(&FontSpec::new(FAMILY, 16.0)).expect("font");
    let spans = [
        Span::new("alpha beta gamma delta "),
        Span::new("epsilon zeta eta theta "),
        Span::new("iota kappa lambda mu"),
    ];
    let layout = font.rich_layout(&spans, 90.0).expect("layout");
    let last = layout.rects_of_span(2);
    assert!(last.len() >= 2, "the narrow layout wrapped: {last:?}");
    let wrapped = last[last.len() - 1];
    let (x, y) = centre(wrapped);
    let hit = layout.hit_test_point(x, y);
    assert_eq!(hit.span, 2, "a wrapped run hit the wrong span: {hit:?}");
}

#[test]
fn the_height_grows_as_the_width_shrinks() {
    let font = system().font(&FontSpec::new(FAMILY, 16.0)).expect("font");
    let spans = [Span::new(
        "The quick brown fox jumps over the lazy dog and keeps running",
    )];
    let wide = font.rich_layout(&spans, 600.0).expect("wide");
    let narrow = font.rich_layout(&spans, 120.0).expect("narrow");
    assert!(
        narrow.height() > wide.height(),
        "narrow {} vs wide {}",
        narrow.height(),
        wide.height()
    );
}

#[test]
fn mixed_sizes_share_one_baseline() {
    let font = system().font(&FontSpec::new(FAMILY, 16.0)).expect("font");
    let spans = [Span::new("Big").size(28.0), Span::new(" small")];
    let layout = font.rich_layout(&spans, f32::INFINITY).expect("layout");
    let big = layout.rects_of_span(0)[0];
    let small = layout.rects_of_span(1)[0];
    assert!(
        (big.bottom - small.bottom).abs() < 1.0,
        "the runs are not on one baseline: {big:?} {small:?}"
    );
}

/// The on-screen rectangles of the widget's two runs, in device-independent
/// pixels, from the same font and width the widget lays out with.
fn span_rects(flow: &FlowText<Msg>, dpi: u32) -> (RectF, RectF) {
    let width = pixels_to_dips(flow.bounds().width(), dpi);
    let font = system().font(&FontSpec::new(FAMILY, SIZE)).expect("font");
    let spans = [Span::new("plain "), Span::new("link")];
    let layout = font.rich_layout(&spans, width).expect("layout");
    (layout.rects_of_span(0)[0], layout.rects_of_span(1)[0])
}

/// Converts a device-independent point to widget-client device pixels.
fn to_px(point: (f32, f32), dpi: u32) -> (i32, i32) {
    let scale = dpi as f32 / 96.0;
    (
        (point.0 * scale).round() as i32,
        (point.1 * scale).round() as i32,
    )
}

fn send(flow: &FlowText<Msg>, message: u32, wparam: usize, (x, y): (i32, i32)) {
    let hwnd = HWND(flow.hwnd().raw() as *mut core::ffi::c_void);
    let lparam = ((y as i16 as u16 as isize) << 16) | (x as i16 as u16 as isize);
    // SAFETY: a synchronous message to the flow widget's own live window.
    unsafe {
        SendMessageW(hwnd, message, Some(WPARAM(wparam)), Some(LPARAM(lparam)));
    }
}

/// Parks the real pointer in the far corner, so it adds no genuine mouse moves.
fn park_real_pointer() {
    // SAFETY: plain integer arguments; a failure (no desktop) is ignored.
    unsafe {
        let _ = SetCursorPos(
            GetSystemMetrics(SM_CXSCREEN) - 1,
            GetSystemMetrics(SM_CYSCREEN) - 1,
        );
    }
}

fn shared_cursor(name: PCWSTR) -> usize {
    // SAFETY: a shared system cursor id, no pointers.
    unsafe { LoadCursorW(None, name) }.expect("system cursor").0 as usize
}

fn class_cursor(flow: &FlowText<Msg>) -> usize {
    let hwnd = HWND(flow.hwnd().raw() as *mut core::ffi::c_void);
    // SAFETY: `hwnd` is live; the class cursor is a plain value.
    unsafe { GetClassLongPtrW(hwnd, GCLP_HCURSOR) }
}

#[derive(Clone, Debug, PartialEq)]
enum Msg {
    Start,
    Artist,
    Late,
}

/// What one run of the harness captured.
struct Outcome {
    log: Vec<Msg>,
    /// `(hand over the link, arrow over plain text)`, for the cursor test.
    cursors: (bool, bool),
}

struct Harness {
    flow: Option<FlowText<Msg>>,
    dpi: u32,
    click: bool,
    log: Rc<RefCell<Vec<Msg>>>,
    cursors: Rc<RefCell<(bool, bool)>>,
}

impl Harness {
    fn update_start(&mut self, ui: &mut Ui<Msg>) {
        let Some(flow) = &self.flow else {
            return;
        };
        let (plain, link) = span_rects(flow, self.dpi);
        let link_px = to_px(centre(link), self.dpi);
        if self.click {
            send(flow, WM_LBUTTONDOWN, 1, link_px);
            send(flow, WM_LBUTTONUP, 0, link_px);
            let plain_px = to_px(centre(plain), self.dpi);
            send(flow, WM_LBUTTONDOWN, 1, plain_px);
            send(flow, WM_LBUTTONUP, 0, plain_px);
            return;
        }
        send(flow, WM_MOUSEMOVE, 0, link_px);
        let hand = class_cursor(flow) == shared_cursor(IDC_HAND);
        let plain_px = to_px(centre(plain), self.dpi);
        send(flow, WM_MOUSEMOVE, 0, plain_px);
        let arrow = class_cursor(flow) == shared_cursor(IDC_ARROW);
        *self.cursors.borrow_mut() = (hand, arrow);
        ui.quit();
    }
}

impl App for Harness {
    type Msg = Msg;

    fn update(&mut self, msg: Msg, ui: &mut Ui<Msg>) {
        self.log.borrow_mut().push(msg.clone());
        match msg {
            Msg::Start => self.update_start(ui),
            Msg::Late => ui.quit(),
            Msg::Artist => {}
        }
    }
}

fn run(name: &str, click: bool) -> Option<Outcome> {
    park_real_pointer();
    let log = Rc::new(RefCell::new(Vec::new()));
    let cursors = Rc::new(RefCell::new((false, false)));
    let log_for_make = Rc::clone(&log);
    let cursors_for_make = Rc::clone(&cursors);
    let run = run_app_with_watchdog(name, move |ui| {
        let dpi = ui.dpi();
        let flow = FlowText::new(ui).ok().map(|flow| {
            flow.run(Run::normal("plain "))
                .run(Run::link("link").on_click(|| Some(Msg::Artist)))
        });
        if let Some(flow) = &flow {
            ui.set_layout(column![flow.fill(1)]);
        }
        let proxy = ui.proxy();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(400));
            let _ = proxy.send(Msg::Late);
        });
        ui.emit(Msg::Start);
        Harness {
            flow,
            dpi,
            click,
            log: log_for_make,
            cursors: cursors_for_make,
        }
    })?;
    assert!(!run.timed_out, "the watchdog fired before the app quit");
    let log = log.borrow().clone();
    let cursors = *cursors.borrow();
    Some(Outcome { log, cursors })
}

#[test]
fn clicking_a_link_emits_its_message_and_plain_text_does_not() {
    let Some(outcome) = run("win32ui.flow_text.click", true) else {
        return;
    };
    assert!(
        outcome.log.contains(&Msg::Artist),
        "the link click did not emit: {:?}",
        outcome.log
    );
    assert_eq!(
        outcome
            .log
            .iter()
            .filter(|msg| **msg == Msg::Artist)
            .count(),
        1,
        "a plain-text click emitted too: {:?}",
        outcome.log
    );
}

#[test]
fn the_cursor_is_a_hand_over_a_link_and_an_arrow_over_text() {
    let Some(outcome) = run("win32ui.flow_text.cursor", false) else {
        return;
    };
    assert!(outcome.cursors.0, "the link did not get the hand cursor");
    assert!(outcome.cursors.1, "plain text did not get the arrow cursor");
}
