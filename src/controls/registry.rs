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
use crate::message::Message;

/// Which kind of control owns a handle, used to disambiguate notifications
/// whose codes are shared (e.g. `NM_DBLCLK`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ControlKind {
    ListView,
    TreeView,
    Tooltip,
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

thread_local! {
    /// Per-`HWND` widget-layer event mappers: each maps the control's decoded
    /// `Message` to the app's `Msg`, pushed onto the app's queue by the mapper
    /// itself (which captures a `Sender`).
    static APP_EVENTS: RefCell<HashMap<usize, AppEventMapper>> = RefCell::new(HashMap::new());
}

/// A type-erased widget event mapper: consumes the decoded message and reports
/// whether it was handled.
type AppEventMapper = Rc<dyn Fn(&Message) -> bool>;

/// Associates a widget-layer event mapper with `hwnd`. The mapper returns
/// whether it consumed the message.
pub(crate) fn register_app_events(hwnd: Hwnd, mapper: AppEventMapper) {
    APP_EVENTS.with(|events| {
        events.borrow_mut().insert(hwnd.raw(), mapper);
    });
}

/// Removes any widget-layer event mapper for `hwnd`.
pub(crate) fn unregister_app_events(hwnd: Hwnd) {
    APP_EVENTS.with(|events| {
        events.borrow_mut().remove(&hwnd.raw());
    });
}

/// Offers a decoded message to the widget-layer mapper registered for `hwnd`,
/// returning whether it was consumed.
pub(crate) fn dispatch_app_event(hwnd: Hwnd, message: &Message) -> bool {
    let mapper = APP_EVENTS.with(|events| events.borrow().get(&hwnd.raw()).cloned());
    match mapper {
        Some(mapper) => mapper(message),
        None => false,
    }
}
