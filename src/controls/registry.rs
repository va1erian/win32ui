#![forbid(unsafe_code)]

//! A thread-local map from child `HWND` to the Rust object that handles its
//! self-contained notifications.
//!
//! Common controls send `WM_NOTIFY` to their *parent*, so without this the
//! application's [`WindowHandler`](crate::WindowHandler) would have to know
//! about owner-data and custom-draw internals. Instead, the window procedure
//! offers each notification to the registered control first; only events a
//! control does not swallow reach the application.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::hwnd::Hwnd;

/// Which kind of control owns a handle, used to disambiguate notifications
/// whose codes are shared (e.g. `NM_DBLCLK`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ControlKind {
    ListView,
    TreeView,
}

/// Implemented by control state that wants first refusal on its own
/// notifications.
pub(crate) trait ControlEvents {
    /// The control's kind.
    fn kind(&self) -> ControlKind;

    /// Handles a raw notification, returning `Some(code)` if it was consumed.
    fn on_notification(
        &mut self,
        hwnd: Hwnd,
        code: u32,
        wparam: usize,
        lparam: isize,
    ) -> Option<isize>;
}

thread_local! {
    static REGISTRY: RefCell<HashMap<usize, Rc<RefCell<dyn ControlEvents>>>> =
        RefCell::new(HashMap::new());
}

/// Associates `events` with `hwnd`.
pub(crate) fn register(hwnd: Hwnd, events: Rc<RefCell<dyn ControlEvents>>) {
    REGISTRY.with(|registry| {
        registry.borrow_mut().insert(hwnd.raw(), events);
    });
}

/// Removes any registration for `hwnd`.
pub(crate) fn unregister(hwnd: Hwnd) {
    REGISTRY.with(|registry| {
        registry.borrow_mut().remove(&hwnd.raw());
    });
}

/// The kind of control `hwnd` maps to, if registered.
pub(crate) fn kind(hwnd: Hwnd) -> Option<ControlKind> {
    REGISTRY.with(|registry| {
        registry
            .borrow()
            .get(&hwnd.raw())
            .map(|events| events.borrow().kind())
    })
}

/// Offers a notification to the control registered for `hwnd`.
pub(crate) fn dispatch(hwnd: Hwnd, code: u32, wparam: usize, lparam: isize) -> Option<isize> {
    let events = REGISTRY.with(|registry| registry.borrow().get(&hwnd.raw()).cloned())?;
    // Re-borrow only briefly; a control may itself send messages.
    let mut events = events.try_borrow_mut().ok()?;
    events.on_notification(hwnd, code, wparam, lparam)
}
