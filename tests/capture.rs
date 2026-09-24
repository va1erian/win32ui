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

/// The composited (`Windows.Graphics.Capture`) capture must return the
/// window's *own* surface even when another window sits on top of it, without
/// raising either window or moving the pointer.
#[cfg(feature = "wgc")]
mod composited {
    use super::*;

    /// The under window's background.
    const UNDER: Color = Color::rgb(0x10, 0x80, 0x40);
    /// The occluder's background; must never appear in the under capture.
    const OCCLUDER: Color = Color::rgb(0xC0, 0x20, 0x20);

    struct Null;

    impl WindowHandler for Null {
        fn message(&self, _window: &Window, _message: Message) -> Option<LResult> {
            None
        }
    }

    struct OccludedHandler {
        setup_timer: Rc<Cell<Option<TimerId>>>,
        capture_timer: Rc<Cell<Option<TimerId>>>,
        watchdog_timer: Rc<Cell<Option<TimerId>>>,
        under: Rc<RefCell<Option<RgbaImage>>>,
        over: Rc<RefCell<Option<RgbaImage>>>,
        occluder: RefCell<Option<Window>>,
    }

    impl WindowHandler for OccludedHandler {
        fn message(&self, window: &Window, message: Message) -> Option<LResult> {
            match message {
                Message::Create => {
                    // The occluder is created later, so it ends up on top of
                    // the under window rather than being raised over by the
                    // test's `show` call.
                    self.setup_timer.set(window.set_timer(100).ok());
                    self.capture_timer.set(window.set_timer(400).ok());
                    self.watchdog_timer.set(window.set_timer(3000).ok());
                }
                Message::Timer { id } if Some(id) == self.setup_timer.get() => {
                    let bounds = window.window_rect();
                    if let Ok(class) = WindowClass::register("win32ui.capture.occluder", OCCLUDER)
                        && let Ok(occluder) = Window::create(
                            class,
                            None,
                            WindowStyle::new().popup().visible(),
                            WindowExStyle::new(),
                            bounds,
                            "occluder",
                            Null,
                        )
                    {
                        occluder.show();
                        *self.occluder.borrow_mut() = Some(occluder);
                    }
                }
                Message::Timer { id } if Some(id) == self.capture_timer.get() => {
                    *self.under.borrow_mut() = window.capture_composited().ok();
                    if let Some(occluder) = self.occluder.borrow().as_ref() {
                        *self.over.borrow_mut() = occluder.capture_composited().ok();
                        occluder.destroy();
                    }
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
    fn composited_capture_ignores_an_occluder() {
        win32ui::init();

        let Ok(class) = WindowClass::register("win32ui.capture.under", UNDER) else {
            return;
        };
        let under = Rc::new(RefCell::new(None));
        let over = Rc::new(RefCell::new(None));
        let Ok(window) = Window::create(
            class,
            None,
            WindowStyle::new().popup().visible(),
            WindowExStyle::new(),
            Rect::new(0, 0, 80, 80),
            "under",
            OccludedHandler {
                setup_timer: Rc::new(Cell::new(None)),
                capture_timer: Rc::new(Cell::new(None)),
                watchdog_timer: Rc::new(Cell::new(None)),
                under: Rc::clone(&under),
                over: Rc::clone(&over),
                occluder: RefCell::new(None),
            },
        ) else {
            return;
        };

        window.show();
        win32ui::run();

        let under_image = under.borrow();
        let image = under_image
            .as_ref()
            .expect("the composited capture failed or the watchdog won");
        let pixel = image.pixel(40, 40).unwrap();
        assert_eq!(
            &pixel[..3],
            &[UNDER.r, UNDER.g, UNDER.b],
            "the capture shows the occluder instead of the window's own content"
        );
        // The occluder itself renders too, proving the two colours differ on
        // screen and the assertion above is meaningful.
        if let Some(over_image) = over.borrow().as_ref() {
            let occluder_pixel = over_image.pixel(40, 40).unwrap();
            assert_eq!(&occluder_pixel[..3], &[OCCLUDER.r, OCCLUDER.g, OCCLUDER.b]);
        }
    }

    struct MinimizedHandler {
        capture_timer: Rc<Cell<Option<TimerId>>>,
        watchdog_timer: Rc<Cell<Option<TimerId>>>,
        result: Rc<RefCell<Option<Result<RgbaImage>>>>,
    }

    impl WindowHandler for MinimizedHandler {
        fn message(&self, window: &Window, message: Message) -> Option<LResult> {
            match message {
                Message::Create => {
                    window.show_minimized();
                    self.capture_timer.set(window.set_timer(250).ok());
                    self.watchdog_timer.set(window.set_timer(3000).ok());
                }
                Message::Timer { id } if Some(id) == self.capture_timer.get() => {
                    *self.result.borrow_mut() = Some(window.capture_composited());
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
    fn composited_capture_reports_a_minimized_window() {
        win32ui::init();

        let Ok(class) = WindowClass::register("win32ui.capture.minimized", BACKGROUND) else {
            return;
        };
        let result = Rc::new(RefCell::new(None));
        let Ok(window) = Window::create(
            class,
            None,
            WindowStyle::overlapped().visible(),
            WindowExStyle::new(),
            Rect::new(0, 0, 64, 64),
            "minimized",
            MinimizedHandler {
                capture_timer: Rc::new(Cell::new(None)),
                watchdog_timer: Rc::new(Cell::new(None)),
                result: Rc::clone(&result),
            },
        ) else {
            return;
        };

        window.show();
        win32ui::run();

        let result = result.borrow();
        let captured = result
            .as_ref()
            .expect("the composited capture never ran (watchdog won)");
        assert!(
            matches!(captured, Err(Error::Capture(CaptureError::Minimized))),
            "a minimized window must return the typed error, got {captured:?}"
        );
    }
}
