#![forbid(unsafe_code)]

//! Per-window state for the extended title bar: whether the standard caption is
//! removed, the caption buttons' inset, and which child widgets accept clicks
//! in the caption strip instead of starting a window drag.
//!
//! The raw `WM_NCCALCSIZE` / `WM_NCHITTEST` handling lives in [`crate::sys::nc`];
//! this is the safe store it reads and the widget layer writes.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use crate::geometry::Rect;
use crate::hwnd::Hwnd;

#[derive(Default)]
struct Entry {
    extended: bool,
    /// The caption buttons' bounds in client coordinates (empty when unknown).
    caption_inset: Rect,
}

thread_local! {
    static WINDOWS: RefCell<HashMap<usize, Entry>> = RefCell::new(HashMap::new());
    /// Child windows that opted into caption-strip clicks.
    static INTERACTIVE: RefCell<HashSet<usize>> = RefCell::new(HashSet::new());
}

/// Records whether `window` uses the extended title bar.
pub(crate) fn set_extended(window: Hwnd, extended: bool) {
    WINDOWS.with(|map| {
        map.borrow_mut().entry(window.raw()).or_default().extended = extended;
    });
}

/// Whether `window` uses the extended title bar.
pub(crate) fn is_extended(window: Hwnd) -> bool {
    WINDOWS.with(|map| map.borrow().get(&window.raw()).is_some_and(|e| e.extended))
}

/// Records the caption buttons' bounds (client coordinates) for `window`.
pub(crate) fn set_caption_inset(window: Hwnd, inset: Rect) {
    WINDOWS.with(|map| {
        map.borrow_mut()
            .entry(window.raw())
            .or_default()
            .caption_inset = inset;
    });
}

/// The caption buttons' bounds (client coordinates) for `window`.
pub(crate) fn caption_inset(window: Hwnd) -> Rect {
    WINDOWS.with(|map| {
        map.borrow()
            .get(&window.raw())
            .map_or(Rect::default(), |entry| entry.caption_inset)
    })
}

/// Records whether `child` accepts clicks in the caption strip.
pub(crate) fn set_caption_interactive(child: Hwnd, interactive: bool) {
    INTERACTIVE.with(|set| {
        let mut set = set.borrow_mut();
        if interactive {
            set.insert(child.raw());
        } else {
            set.remove(&child.raw());
        }
    });
}

/// Whether `child` accepts clicks in the caption strip.
pub(crate) fn is_caption_interactive(child: Hwnd) -> bool {
    INTERACTIVE.with(|set| set.borrow().contains(&child.raw()))
}

/// Drops all extended-title-bar state for a destroyed window.
pub(crate) fn forget_window(window: Hwnd) {
    WINDOWS.with(|map| {
        map.borrow_mut().remove(&window.raw());
    });
    INTERACTIVE.with(|set| {
        set.borrow_mut().remove(&window.raw());
    });
}
