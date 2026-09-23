//! Reentrancy: a message that arrives while a handler is already on the stack
//! must still reach the handler, and a handler destroyed from inside one of its
//! own messages must stay alive until the outer dispatch returns.

#![cfg(windows)]

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{NullHandler, run_with_watchdog};
use win32ui::prelude::*;

/// The same reentrancy guarantee without depending on the common controls:
/// a handler synchronously sends a message to its own window and must receive
/// it back before the send returns.
#[test]
fn reentrant_sent_message_reaches_handler() {
    const WM_PROBE: u32 = 0x0400; // `WM_USER`

    struct ReentrantHandler {
        under_test: Rc<Cell<Option<TimerId>>>,
        probed: Rc<Cell<bool>>,
    }

    impl WindowHandler for ReentrantHandler {
        fn message(&self, window: &Window, message: Message) -> Option<LResult> {
            match message {
                Message::Create => {
                    self.under_test.set(window.set_timer(50).ok());
                    Some(0)
                }
                Message::Timer { id } if Some(id) == self.under_test.get() => {
                    window.send_message(WM_PROBE, 0, 0);
                    window.destroy();
                    win32ui::quit(0);
                    Some(0)
                }
                Message::Other { code, .. } if code == WM_PROBE => {
                    self.probed.set(true);
                    Some(0)
                }
                _ => None,
            }
        }
    }

    let under_test = Rc::new(Cell::new(None));
    let probed = Rc::new(Cell::new(false));
    let Some(run) = run_with_watchdog("win32ui.reentrant.send", || ReentrantHandler {
        under_test: Rc::clone(&under_test),
        probed: Rc::clone(&probed),
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired");
    assert!(under_test.get().is_some(), "the timer was not started");
    assert!(
        probed.get(),
        "a message sent from a handler did not reach it reentrantly"
    );
}

/// Destroying a window from inside its own handler tears the handler down
/// reentrantly (`WM_DESTROY`/`WM_NCDESTROY` while an outer dispatch still
/// holds a shared reference to it). The deferred free must keep it alive until
/// the outer dispatch returns; the write after `destroy()` would be
/// use-after-free otherwise.
#[test]
fn destroy_from_handler_is_safe() {
    struct DestroyHandler {
        under_test: Rc<Cell<Option<TimerId>>>,
        destroyed: Rc<Cell<bool>>,
    }

    impl WindowHandler for DestroyHandler {
        fn message(&self, window: &Window, message: Message) -> Option<LResult> {
            match message {
                Message::Create => {
                    self.under_test.set(window.set_timer(50).ok());
                }
                Message::Timer { id } if Some(id) == self.under_test.get() => {
                    window.destroy();
                    self.destroyed.set(true);
                    win32ui::quit(0);
                }
                _ => {}
            }
            None
        }
    }

    let under_test = Rc::new(Cell::new(None));
    let destroyed = Rc::new(Cell::new(false));
    let Some(run) = run_with_watchdog("win32ui.destroy", || DestroyHandler {
        under_test: Rc::clone(&under_test),
        destroyed: Rc::clone(&destroyed),
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired");
    assert!(destroyed.get(), "the timer never ran");
    assert!(
        !run.window.is_alive(),
        "the window survived its own destroy()"
    );
}

/// A window whose handler owns a child `Window` destroys itself from inside its
/// own handler. Dropping the handler then destroys the child, which re-enters
/// `window_proc`; the free of the parent handler must not run while the
/// dispatch bookkeeping is borrowed, or the reentry would panic inside the
/// `extern "system"` boundary and abort the process.
///
/// The child is parented to a second, independent window: a child of the window
/// being destroyed is torn down by `DestroyWindow` before the handler is
/// dropped, so it would never exercise the reentry.
#[test]
fn destroy_owning_child_from_handler_is_safe() {
    struct OwningHandler {
        // Declared before `owner` so the child drops (and reenters) first.
        child: RefCell<Option<Window>>,
        owner: RefCell<Option<Window>>,
        under_test: Rc<Cell<Option<TimerId>>>,
        created: Rc<Cell<bool>>,
        destroyed: Rc<Cell<bool>>,
    }

    impl WindowHandler for OwningHandler {
        fn message(&self, window: &Window, message: Message) -> Option<LResult> {
            match message {
                Message::Create => {
                    let background = Theme::light().background;
                    let owner = WindowClass::register("win32ui.destroy.owner", background)
                        .ok()
                        .and_then(|class| {
                            Window::create(
                                class,
                                None,
                                WindowStyle::overlapped(),
                                WindowExStyle::new(),
                                Rect::new(0, 0, 200, 200),
                                "owner",
                                NullHandler,
                            )
                            .ok()
                        });
                    if let Some(owner) = &owner {
                        *self.child.borrow_mut() =
                            WindowClass::register("win32ui.destroy.child", background)
                                .ok()
                                .and_then(|class| {
                                    Window::create(
                                        class,
                                        Some(owner.hwnd()),
                                        WindowStyle::new().child().visible(),
                                        WindowExStyle::new(),
                                        Rect::new(0, 0, 100, 100),
                                        "child",
                                        NullHandler,
                                    )
                                    .ok()
                                });
                    }
                    self.created.set(self.child.borrow().is_some());
                    *self.owner.borrow_mut() = owner;
                    self.under_test.set(window.set_timer(50).ok());
                    Some(0)
                }
                Message::Timer { id } if Some(id) == self.under_test.get() => {
                    window.destroy();
                    self.destroyed.set(true);
                    win32ui::quit(0);
                    Some(0)
                }
                _ => None,
            }
        }
    }

    let under_test = Rc::new(Cell::new(None));
    let created = Rc::new(Cell::new(false));
    let destroyed = Rc::new(Cell::new(false));
    let Some(run) = run_with_watchdog("win32ui.destroy.owning", || OwningHandler {
        child: RefCell::new(None),
        owner: RefCell::new(None),
        under_test: Rc::clone(&under_test),
        created: Rc::clone(&created),
        destroyed: Rc::clone(&destroyed),
    }) else {
        return;
    };

    assert!(
        !run.timed_out,
        "the watchdog fired: destroying an owned child hung or aborted"
    );
    if !created.get() {
        return;
    }
    assert!(destroyed.get(), "the timer never ran");
    assert!(
        !run.window.is_alive(),
        "the window survived its own destroy()"
    );
}
