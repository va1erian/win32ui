//! Offscreen rendering of Direct2D custom widgets: `Custom::render_image`
//! draws `paint_d2d` into a software buffer independent of the window and the
//! desktop, and `Ui::capture` fills the visible Direct2D widgets in with it.

#![cfg(windows)]

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::run_app_with_watchdog;
use win32ui::d2d::{D2dCanvas, RectF};
use win32ui::gdi::Canvas;
use win32ui::prelude::*;
use win32ui::{Renderer, Size};

const LEFT: Color = Color::rgb(0xC0, 0x10, 0x20);
const RIGHT: Color = Color::rgb(0x10, 0x30, 0xD0);
const CLIPPED: Color = Color::rgb(0x00, 0xB0, 0x40);
const WIDTH_DIP: f32 = 160.0;
const HEIGHT_DIP: f32 = 60.0;

/// Fills its left half with `LEFT` and its right half with `RIGHT`, then
/// floods the whole viewport with `CLIPPED` through a clip of the top-left
/// quarter, so the clip stack is exercised too.
struct Halves;

impl CustomWidget for Halves {
    type Event = ();

    fn paint(&self, _canvas: &Canvas, _bounds: Rect, _theme: &Theme) {}

    fn renderer(&self) -> Renderer {
        Renderer::Direct2D
    }

    fn paint_d2d(&self, canvas: &mut D2dCanvas<'_>, bounds: RectF, _theme: &Theme) {
        let middle = bounds.width() / 2.0;
        canvas.fill_rect(RectF::new(0.0, 0.0, middle, bounds.bottom), LEFT);
        canvas.fill_rect(RectF::new(middle, 0.0, bounds.right, bounds.bottom), RIGHT);
        canvas.push_clip(RectF::new(0.0, 0.0, middle / 2.0, bounds.bottom / 2.0));
        canvas.fill_rect(bounds, CLIPPED);
        let _ = canvas.pop_clip();
    }

    fn preferred_size(&self, dpi: u32) -> Option<Size> {
        Some(Size::new(
            dip(WIDTH_DIP).to_px(dpi).value(),
            dip(HEIGHT_DIP).to_px(dpi).value(),
        ))
    }
}

/// A GDI widget, which has no Direct2D paint to render offscreen.
struct GdiOnly;

impl CustomWidget for GdiOnly {
    type Event = ();

    fn paint(&self, _canvas: &Canvas, _bounds: Rect, _theme: &Theme) {}
}

/// What the app observed, read after the loop ends.
#[derive(Default)]
struct Seen {
    rendered: Option<Result<RgbaImage>>,
    rendered_hidden: Option<Result<RgbaImage>>,
    gdi: Option<Result<RgbaImage>>,
    captured: Option<Result<RgbaImage>>,
    size: Size,
    origin: Point,
}

struct OffscreenApp {
    halves: Custom<Halves, ()>,
    gdi: Custom<GdiOnly, ()>,
    seen: Rc<RefCell<Seen>>,
}

impl App for OffscreenApp {
    type Msg = ();

    fn update(&mut self, _msg: (), ui: &mut Ui<()>) {
        let mut seen = self.seen.borrow_mut();
        seen.size = self.halves.bounds().size();
        let widget = self.halves.window_rect();
        let window = ui.window_rect();
        seen.origin = Point::new(widget.left - window.left, widget.top - window.top);
        seen.rendered = Some(self.halves.render_image());
        seen.captured = Some(ui.capture());
        seen.gdi = Some(self.gdi.render_image());
        self.halves.set_visible(false);
        seen.rendered_hidden = Some(self.halves.render_image());
        ui.quit();
    }
}

fn rgba(color: Color) -> [u8; 4] {
    [color.r, color.g, color.b, 0xFF]
}

/// Checks the widget's three regions in `image`, whose widget origin is at
/// `origin`, for a widget of `size` device pixels.
fn assert_halves(image: &RgbaImage, origin: Point, size: Size, what: &str) {
    let at = |x: i32, y: i32| {
        image
            .pixel((origin.x + x) as u32, (origin.y + y) as u32)
            .expect("in bounds")
    };
    let (w, h) = (size.width, size.height);
    assert_eq!(at(w / 8, h / 4), rgba(CLIPPED), "{what}: inside the clip");
    assert_eq!(
        at(w * 3 / 8, h / 4),
        rgba(LEFT),
        "{what}: left half, past the clip"
    );
    assert_eq!(
        at(w / 8, h * 3 / 4),
        rgba(LEFT),
        "{what}: left half, below the clip"
    );
    assert_eq!(at(w * 3 / 4, h / 2), rgba(RIGHT), "{what}: right half");
}

/// `render_image` returns the widget's `paint_d2d` at its client size, pixel
/// exact, whether or not the window can be read (or the widget is visible);
/// `Ui::capture` includes the same pixels at the widget's position; and a GDI
/// widget reports an error rather than a blank image.
#[test]
fn direct2d_widget_renders_offscreen() {
    let seen = Rc::new(RefCell::new(Seen::default()));
    let seen_for_make = Rc::clone(&seen);
    let Some(run) = run_app_with_watchdog("win32ui.custom.offscreen", move |ui| {
        let halves = Custom::new(ui, Halves).expect("d2d widget");
        let gdi = Custom::new(ui, GdiOnly).expect("gdi widget");
        halves.set_bounds(Rect::new(
            10,
            20,
            10 + halves.bounds().width(),
            20 + halves.bounds().height(),
        ));
        ui.emit(());
        OffscreenApp {
            halves,
            gdi,
            seen: seen_for_make,
        }
    }) else {
        return;
    };
    assert!(!run.timed_out, "the watchdog fired before the app quit");

    let mut seen = seen.borrow_mut();
    let size = seen.size;
    let origin = seen.origin;
    let rendered = seen
        .rendered
        .take()
        .expect("update never ran")
        .expect("render_image failed");
    assert_eq!(
        rendered.size(),
        size,
        "the image is the widget's client size"
    );
    assert_halves(&rendered, Point::new(0, 0), size, "render_image");

    let hidden = seen
        .rendered_hidden
        .take()
        .expect("update never ran")
        .expect("render_image of a hidden widget failed");
    assert_halves(&hidden, Point::new(0, 0), size, "render_image while hidden");

    let captured = seen
        .captured
        .take()
        .expect("update never ran")
        .expect("Ui::capture failed");
    assert_halves(&captured, origin, size, "Ui::capture");

    assert!(
        seen.gdi.take().expect("update never ran").is_err(),
        "a GDI widget has no Direct2D paint to render"
    );
}
