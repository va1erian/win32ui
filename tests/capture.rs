//! Capturing a window's pixels: create a window with a known background, let
//! it paint, then check a pixel of the captured image. A watchdog timer makes
//! the test fail rather than hang if the capture timer never fires.

#![cfg(windows)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use win32ui::prelude::*;

const BACKGROUND: Color = Color::rgb(0x12, 0x34, 0x56);

struct CaptureHandler {
    capture_timer: Rc<Cell<Option<TimerId>>>,
    watchdog_timer: Rc<Cell<Option<TimerId>>>,
    captured: Rc<RefCell<Option<Result<RgbaImage>>>>,
}

impl WindowHandler for CaptureHandler {
    fn message(&self, window: &Window, message: Message) -> Option<LResult> {
        match message {
            Message::Create => {
                // The 150 ms capture timer runs after the first paint; the 3 s
                // watchdog guarantees the loop terminates even if it does not.
                self.capture_timer.set(window.set_timer(150).ok());
                self.watchdog_timer.set(window.set_timer(3000).ok());
            }
            Message::Timer { id } if Some(id) == self.capture_timer.get() => {
                *self.captured.borrow_mut() = Some(window.capture());
                window.destroy();
                win32ui::quit(0);
            }
            Message::Timer { id } if Some(id) == self.watchdog_timer.get() => {
                window.destroy();
                win32ui::quit(0);
            }
            _ => {}
        }
        None
    }
}

#[test]
fn capture_reads_back_the_window_background() {
    win32ui::init();

    let Ok(class) = WindowClass::register("win32ui.capture", BACKGROUND) else {
        return;
    };
    let capture_timer = Rc::new(Cell::new(None));
    let watchdog_timer = Rc::new(Cell::new(None));
    let captured = Rc::new(RefCell::new(None));
    let Ok(window) = Window::create(
        class,
        None,
        WindowStyle::new().popup().visible(),
        WindowExStyle::new(),
        Rect::new(0, 0, 64, 64),
        "capture",
        CaptureHandler {
            capture_timer: Rc::clone(&capture_timer),
            watchdog_timer: Rc::clone(&watchdog_timer),
            captured: Rc::clone(&captured),
        },
    ) else {
        return;
    };

    window.show();
    win32ui::run();

    assert!(
        capture_timer.get().is_some(),
        "the capture timer was not started"
    );
    let captured = captured.borrow();
    let image = captured
        .as_ref()
        .expect("the capture timer never fired (watchdog won)")
        .as_ref()
        .expect("PrintWindow failed");
    assert_eq!(
        (image.width, image.height),
        (64, 64),
        "the captured image has the wrong size"
    );
    assert_eq!(
        image.pixel(32, 32),
        Some([BACKGROUND.r, BACKGROUND.g, BACKGROUND.b, 0xFF]),
        "the captured pixel does not match the window background"
    );
}
