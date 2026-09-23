//! Paint-path test: a real window paints through the cached back buffer using
//! the new canvas primitives, then is destroyed (releasing the buffer).

#![cfg(windows)]

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::run_with_watchdog;
use win32ui::gdi::Paint;
use win32ui::prelude::*;

struct PaintHandler {
    painted: Rc<Cell<bool>>,
    dirty: Rc<Cell<bool>>,
}

impl WindowHandler for PaintHandler {
    fn message(&self, window: &Window, message: Message) -> Option<LResult> {
        if let Message::Paint = message {
            if let Some(paint) = Paint::begin(window.hwnd()) {
                let canvas = paint.canvas();
                let dirty = canvas.paint_rect();
                self.dirty.set(!dirty.is_empty());
                canvas.fill_rect(paint.client_rect(), Color::rgb(24, 24, 24));
                canvas.line(
                    Point::new(0, 0),
                    Point::new(50, 50),
                    Color::rgb(200, 0, 0),
                    2,
                );
                canvas.outline(dirty, Color::rgb(0, 120, 200));
                self.painted.set(true);
            }
            window.destroy();
            win32ui::quit(0);
            return Some(0);
        }
        None
    }
}

#[test]
fn paint_path_draws_and_releases() {
    let painted = Rc::new(Cell::new(false));
    let dirty = Rc::new(Cell::new(false));
    let Some(run) = run_with_watchdog("win32ui.gdi", || PaintHandler {
        painted: Rc::clone(&painted),
        dirty: Rc::clone(&dirty),
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the paint");
    assert!(painted.get(), "WM_PAINT never reached the handler");
    assert!(dirty.get(), "the dirty rectangle was empty");
    assert!(!run.window.is_alive(), "the window was not destroyed");
}
