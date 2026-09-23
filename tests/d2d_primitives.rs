//! Direct2D primitives, verified through `Window::capture`: gradient midpoint
//! colours, semi-transparent fills, rounded clips, and bitmap round-trips.

#![cfg(windows)]

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use win32ui::d2d::{
    D2dCanvas, D2dSurface, GradientStop, Interpolation, LinearGradient, PointF, RectF, Rgba,
    RoundedRect,
};
use win32ui::prelude::*;

const BACKGROUND: Color = Color::rgb(0xFF, 0xFF, 0xFF);

/// The paint hook: draws with the canvas and its (DIP) bounds.
type Draw = dyn for<'a, 'b> Fn(&'b mut D2dCanvas<'a>, RectF);

struct PaintHandler {
    surface: RefCell<Option<D2dSurface>>,
    draw: Rc<Draw>,
    capture_timer: Cell<Option<TimerId>>,
    image: Rc<RefCell<Option<Result<RgbaImage>>>>,
}

impl PaintHandler {
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
        canvas.clear(BACKGROUND);
        let bounds = canvas.bounds();
        (self.draw)(&mut canvas, bounds);
        let _ = canvas.end_draw();
    }
}

impl WindowHandler for PaintHandler {
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

/// Runs one D2D paint+capture cycle with `draw`, returning the captured image.
fn run(
    name: &str,
    draw: impl for<'a, 'b> Fn(&'b mut D2dCanvas<'a>, RectF) + 'static,
) -> Option<RgbaImage> {
    let image = Rc::new(RefCell::new(None));
    let handler_image = Rc::clone(&image);
    let draw: Rc<Draw> = Rc::new(draw);
    let run = common::run_with_watchdog(name, move || PaintHandler {
        surface: RefCell::new(None),
        draw: Rc::clone(&draw),
        capture_timer: Cell::new(None),
        image: handler_image,
    })?;
    assert!(!run.timed_out, "the watchdog fired before the capture");
    let image = image
        .borrow_mut()
        .take()
        .expect("the capture timer never fired")
        .expect("Window::capture failed");
    Some(image)
}

#[test]
fn linear_gradient_midpoint_is_a_blend() {
    let Some(image) = run("win32ui.d2d.gradient", |canvas, bounds| {
        let gradient = LinearGradient::new(
            PointF::new(bounds.left, bounds.top),
            PointF::new(bounds.right, bounds.top),
            vec![
                GradientStop::new(0.0, Rgba::rgb(0xFF, 0x00, 0x00)),
                GradientStop::new(1.0, Rgba::rgb(0x00, 0x00, 0xFF)),
            ],
        );
        canvas.fill_rect_linear(bounds, &gradient);
    }) else {
        return;
    };

    let [r, g, b, _] = image
        .pixel(image.width / 2, image.height / 2)
        .expect("in bounds");
    assert!(
        (100..=156).contains(&r) && (100..=156).contains(&b),
        "midpoint should be a red/blue blend, got ({r}, {g}, {b})"
    );
    assert!(g < 10, "no green in a red/blue gradient, got {g}");
}

#[test]
fn semi_transparent_fill_blends_over_background() {
    let Some(image) = run("win32ui.d2d.alpha", |canvas, bounds| {
        canvas.fill_rect_rgba(bounds, Rgba::with_alpha(0x00, 0x00, 0x00, 0x80));
    }) else {
        return;
    };

    let [r, g, b, _] = image
        .pixel(image.width / 2, image.height / 2)
        .expect("in bounds");
    assert_eq!(r, g, "a grey blend");
    assert_eq!(g, b, "a grey blend");
    assert!(
        (96..=160).contains(&r),
        "50% black over white should be a mid grey, got {r}"
    );
}

#[test]
fn rounded_clip_hides_content_outside_the_curve() {
    let Some(image) = run("win32ui.d2d.clip", |canvas, bounds| {
        let pill = RoundedRect::uniform(
            RectF::new(
                bounds.left + 60.0,
                bounds.top + 60.0,
                bounds.right - 60.0,
                bounds.bottom - 60.0,
            ),
            24.0,
        );
        if canvas.push_clip_rounded(pill).is_ok() {
            canvas.fill_rect(bounds, Color::rgb(0xFF, 0x00, 0x00));
            let _ = canvas.pop_clip();
        }
    }) else {
        return;
    };

    let inside = image
        .pixel(image.width / 2, image.height / 2)
        .expect("in bounds");
    let outside = image.pixel(20, image.height / 2).expect("in bounds");
    assert_eq!(
        inside[..3],
        [0xFF, 0x00, 0x00],
        "the clip centre should be filled"
    );
    assert_eq!(
        outside[..3],
        [BACKGROUND.r, BACKGROUND.g, BACKGROUND.b],
        "left of the pill is outside the curve and stays background"
    );
}

#[test]
fn image_pixels_round_trip() {
    let image = RgbaImage {
        width: 1,
        height: 1,
        pixels: vec![0xE0, 0x10, 0xF0, 0xFF],
    };
    let Some(captured) = run("win32ui.d2d.image", move |canvas, bounds| {
        let id = canvas.image(&image);
        canvas.draw_image(id, bounds, None, 1.0, Interpolation::Nearest);
    }) else {
        return;
    };

    let [r, g, b, _] = captured
        .pixel(captured.width / 2, captured.height / 2)
        .expect("in bounds");
    assert_eq!(
        [r, g, b],
        [0xE0, 0x10, 0xF0],
        "the drawn pixel must round-trip through the bitmap upload"
    );
}

#[test]
fn tiled_image_repeats() {
    let image = RgbaImage {
        width: 2,
        height: 1,
        pixels: vec![0xFF, 0x00, 0x00, 0xFF, 0x00, 0x00, 0xFF, 0xFF],
    };
    let Some(captured) = run("win32ui.d2d.tiled", move |canvas, bounds| {
        let id = canvas.image(&image);
        canvas.fill_image_tiled(id, bounds, 1.0, Interpolation::Nearest);
    }) else {
        return;
    };

    // Scan the horizontal row through the window's centre (well inside the
    // client area, away from the title bar and borders): a tiled 2×1 image
    // must alternate its two colours along that row.
    let y = captured.height / 2;
    let mut red = false;
    let mut blue = false;
    for x in 16..captured.width - 16 {
        let [r, g, b, _] = captured.pixel(x, y).expect("in bounds");
        let is_red = r > 0xC0 && g < 0x40 && b < 0x40;
        let is_blue = r < 0x40 && g < 0x40 && b > 0xC0;
        assert!(
            is_red || is_blue,
            "the tiled row should be only red or blue, got ({r}, {g}, {b}) at x={x}"
        );
        red |= is_red;
        blue |= is_blue;
    }
    assert!(red && blue, "the tiled image must repeat both colours");
}
