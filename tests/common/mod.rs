//! Shared scaffolding for the integration tests: class registration, a
//! watchdog timer and the run loop, so each test only describes its handler.
//!
//! If the CI session cannot create windows at all, [`run_with_watchdog`]
//! returns `None` and the test skips rather than fails.

// Each test binary compiles this module separately, so a helper used by one
// test is "dead" in the others.
#![allow(dead_code)]

use std::cell::Cell;
use std::rc::Rc;

use win32ui::prelude::*;

/// Milliseconds after which the watchdog gives up on a handler.
const WATCHDOG_MS: u32 = 5000;

/// How a watched run ended.
pub struct Run {
    /// Whether the watchdog fired before the handler quit.
    pub timed_out: bool,
    /// The id of the watchdog timer, so a test can prove its own timer got a
    /// different id.
    pub watchdog: Option<TimerId>,
    /// The window, for liveness assertions after the run.
    pub window: Window,
}

/// Creates a window of a freshly registered class, runs its message loop under
/// a watchdog, and reports how it ended.
///
/// `make` builds the handler. The helper starts a watchdog timer on the
/// window's `Create` (before forwarding it to the handler) and quits the loop
/// if it fires, so a handler that never finishes fails the test instead of
/// hanging it. Returns `None` when the session cannot create windows.
pub fn run_with_watchdog<H, F>(name: &str, make: F) -> Option<Run>
where
    H: WindowHandler + 'static,
    F: FnOnce() -> H,
{
    win32ui::init();

    let theme = Theme::light();
    let Ok(class) = WindowClass::register(name, theme.background) else {
        return None;
    };
    let timed_out = Rc::new(Cell::new(false));
    let watchdog = Rc::new(Cell::new(None));
    let handler = WatchdogHandler {
        inner: make(),
        watchdog: Rc::clone(&watchdog),
        timed_out: Rc::clone(&timed_out),
    };
    let Ok(window) = Window::create(
        class,
        None,
        WindowStyle::overlapped(),
        WindowExStyle::new(),
        Rect::new(0, 0, 320, 240),
        name,
        handler,
    ) else {
        return None;
    };

    window.show();
    win32ui::run();
    Some(Run {
        timed_out: timed_out.get(),
        watchdog: watchdog.get(),
        window,
    })
}

/// Wraps a test handler to own the watchdog timer and quit the loop if it
/// fires.
struct WatchdogHandler<H> {
    inner: H,
    watchdog: Rc<Cell<Option<TimerId>>>,
    timed_out: Rc<Cell<bool>>,
}

impl<H: WindowHandler> WindowHandler for WatchdogHandler<H> {
    fn message(&self, window: &Window, message: Message) -> Option<LResult> {
        match message {
            Message::Create => {
                self.watchdog.set(window.set_timer(WATCHDOG_MS).ok());
                self.inner.message(window, message)
            }
            Message::Timer { id } if Some(id) == self.watchdog.get() => {
                self.timed_out.set(true);
                window.destroy();
                win32ui::quit(1);
                Some(0)
            }
            _ => self.inner.message(window, message),
        }
    }
}

/// A handler that ignores every message.
pub struct NullHandler;

impl WindowHandler for NullHandler {
    fn message(&self, _window: &Window, _message: Message) -> Option<LResult> {
        None
    }
}

/// Five list rows, enough to exercise owner-data requests.
pub struct TestRows;

impl ListSource for TestRows {
    fn item_count(&self) -> usize {
        5
    }

    fn text(&self, item: usize, column: usize) -> String {
        format!("{item}/{column}")
    }
}
