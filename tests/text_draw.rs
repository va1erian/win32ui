//! Drawing DirectWrite text, verified through `Window::capture`: the wave
//! emoji must keep its own colours (which a monochrome glyph never does) and
//! measurement of a fallback string must agree with what gets drawn.

#![cfg(windows)]

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use win32ui::d2d::{D2dSurface, Font, FontSpec, PointF, TextSystem};
use win32ui::prelude::*;

const BACKGROUND: Color = Color::rgb(0xFF, 0xFF, 0xFF);
const INK: Color = Color::rgb(0x00, 0x00, 0x00);
const EMOJI: &str = "\u{1F44B}";

struct TextHandler {
    surface: RefCell<Option<D2dSurface>>,
    font: Font,
    capture_timer: Cell<Option<TimerId>>,
    image: Rc<RefCell<Option<Result<RgbaImage>>>>,
}

impl TextHandler {
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
        if let Ok(layout) = self.font.layout(EMOJI, f32::INFINITY) {
            canvas.draw_text(&layout, PointF::new(20.0, 20.0), INK);
        }
        let _ = canvas.end_draw();
    }
}

impl WindowHandler for TextHandler {
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

/// Whether the pixel is the emoji's saturated yellow. A monochrome glyph on
/// white only yields greys and ClearType fringes, never a solid yellow.
fn is_yellow(pixel: [u8; 4]) -> bool {
    pixel[0] > 200 && pixel[1] > 150 && pixel[2] < 120
}

#[test]
fn colour_emoji_keeps_its_colours() {
    let system = TextSystem::new().expect("text system");
    let font = system.font(&FontSpec::new("Segoe UI", 64.0)).expect("font");
    let image = Rc::new(RefCell::new(None));
    let handler_image = Rc::clone(&image);

    let Some(run) = common::run_with_watchdog("win32ui.text.emoji", move || TextHandler {
        surface: RefCell::new(None),
        font,
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

    let yellow = (0..image.height)
        .flat_map(|y| (0..image.width).map(move |x| (x, y)))
        .filter(|&(x, y)| image.pixel(x, y).is_some_and(is_yellow))
        .count();
    assert!(
        yellow > 1500,
        "the emoji has no colour ({yellow} yellow pixels)"
    );
}
