//! Direct2D rendering, verified through `Window::capture`: a rounded rectangle
//! drawn with `D2dCanvas` must have anti-aliased edges, which GDI cannot
//! produce (it only ever writes the fill and background colours).

#![cfg(windows)]

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use win32ui::d2d::{D2dSurface, PointF, RectF};
use win32ui::gdi::Paint;
use win32ui::prelude::*;

const FILL: Color = Color::rgb(0xFF, 0x00, 0x00);
const BACKGROUND: Color = Color::rgb(0xFF, 0xFF, 0xFF);

struct D2dHandler {
    surface: RefCell<Option<D2dSurface>>,
    capture_timer: Cell<Option<TimerId>>,
    reentrant_frame_rejected: Rc<Cell<bool>>,
    image: Rc<RefCell<Option<Result<RgbaImage>>>>,
}

impl D2dHandler {
    fn paint(&self, window: &Window) {
        let mut surface = self.surface.borrow_mut();
        if surface.is_none() {
            *surface = D2dSurface::new(window.hwnd()).ok();
        }
        let Some(surface) = surface.as_ref() else {
            return;
        };
        let Ok(mut canvas) = surface.begin_draw() else {
            return;
        };
        self.reentrant_frame_rejected
            .set(surface.begin_draw().is_err());
        canvas.clear(BACKGROUND);
        let pill = RectF::new(20.0, 20.0, 220.0, 60.0);
        canvas.fill_rounded_rect(pill, pill.pill_radius(), FILL);
        canvas.fill_ellipse(PointF::new(260.0, 40.0), 16.0, 16.0, FILL);
        let _ = canvas.end_draw();
    }
}

impl WindowHandler for D2dHandler {
    fn message(&self, window: &Window, message: Message) -> Option<LResult> {
        if D2dSurface::is_erase_background(&message) {
            return Some(1);
        }
        match message {
            Message::Create => {
                self.capture_timer.set(window.set_timer(300).ok());
            }
            Message::Paint => {
                self.paint(window);
                return Some(0);
            }
            Message::Size { width, height } => {
                if let Some(surface) = self.surface.borrow().as_ref() {
                    surface.resize(width, height);
                }
            }
            Message::Timer { id } if Some(id) == self.capture_timer.get() => {
                *self.image.borrow_mut() = Some(window.capture());
                window.destroy();
                win32ui::quit(0);
            }
            _ => {}
        }
        None
    }
}

/// Whether `pixel` is a mix of the red fill and the white background: full red,
/// with green and blue equal and strictly between the two extremes.
fn is_blend(pixel: [u8; 4]) -> bool {
    pixel[0] == 0xFF && pixel[1] == pixel[2] && pixel[1] > 0 && pixel[1] < 0xFF
}

#[test]
fn rounded_shapes_have_antialiased_edges() {
    let reentrant_frame_rejected = Rc::new(Cell::new(false));
    let image = Rc::new(RefCell::new(None));
    let handler_flag = Rc::clone(&reentrant_frame_rejected);
    let handler_image = Rc::clone(&image);

    let Some(run) = common::run_with_watchdog("win32ui.d2d.antialiasing", move || D2dHandler {
        surface: RefCell::new(None),
        capture_timer: Cell::new(None),
        reentrant_frame_rejected: handler_flag,
        image: handler_image,
    }) else {
        return;
    };
    assert!(!run.timed_out, "the watchdog fired before the capture");
    let image = image
        .borrow_mut()
        .take()
        .expect("the capture timer never fired")
        .expect("Window::capture failed");

    let mut solid = 0;
    let mut levels = std::collections::BTreeSet::new();
    for y in 0..image.height {
        for x in 0..image.width {
            let pixel = image.pixel(x, y).expect("in bounds");
            if pixel == [FILL.r, FILL.g, FILL.b, 0xFF] {
                solid += 1;
            } else if is_blend(pixel) {
                levels.insert(pixel[1]);
            }
        }
    }
    assert!(solid > 5000, "the shapes were not filled ({solid} pixels)");
    assert!(
        levels.len() >= 6,
        "no anti-aliased edge pixels: only {} blend levels",
        levels.len()
    );
    assert!(
        reentrant_frame_rejected.get(),
        "a second begin_draw during a frame must be rejected"
    );
}

/// A `Canvas` over a paint DC (the list view's header, the toolbar, the menus)
/// draws its triangle through Direct2D, so the sort arrow's diagonal edges are
/// anti-aliased instead of GDI's stair-stepped `Polygon`.
struct TriangleHandler {
    capture_timer: Cell<Option<TimerId>>,
    image: Rc<RefCell<Option<Result<RgbaImage>>>>,
}

impl WindowHandler for TriangleHandler {
    fn message(&self, window: &Window, message: Message) -> Option<LResult> {
        match message {
            Message::Create => {
                self.capture_timer.set(window.set_timer(300).ok());
            }
            Message::Paint => {
                if let Some(paint) = Paint::begin(window.hwnd()) {
                    let client = paint.client_rect();
                    let canvas = paint.canvas();
                    canvas.fill_rect(client, BACKGROUND);
                    canvas.triangle(Rect::new(20, 20, 220, 220), FILL, true);
                }
                return Some(0);
            }
            Message::Timer { id } if Some(id) == self.capture_timer.get() => {
                *self.image.borrow_mut() = Some(window.capture());
                window.destroy();
                win32ui::quit(0);
            }
            _ => {}
        }
        None
    }
}

#[test]
fn canvas_triangle_has_antialiased_edges() {
    let image = Rc::new(RefCell::new(None));
    let handler_image = Rc::clone(&image);

    let Some(run) = common::run_with_watchdog("win32ui.d2d.triangle", move || TriangleHandler {
        capture_timer: Cell::new(None),
        image: handler_image,
    }) else {
        return;
    };
    assert!(!run.timed_out, "the watchdog fired before the capture");
    let image = image
        .borrow_mut()
        .take()
        .expect("the capture timer never fired")
        .expect("Window::capture failed");

    // GDI's `Polygon` writes only the fill and the background; Direct2D's
    // anti-aliased edges leave pixels that are a blend of the two.
    let mut intermediate = 0;
    for y in 0..image.height {
        for x in 0..image.width {
            let pixel = image.pixel(x, y).expect("in bounds");
            if pixel != [FILL.r, FILL.g, FILL.b, 0xFF]
                && pixel != [BACKGROUND.r, BACKGROUND.g, BACKGROUND.b, 0xFF]
            {
                intermediate += 1;
            }
        }
    }
    assert!(
        intermediate > 100,
        "the triangle had too few anti-aliased edge pixels ({intermediate})"
    );
}
