#![forbid(unsafe_code)]

//! Per-window state for the extended title bar: whether the standard caption is
//! removed, the caption buttons' inset, and which child widgets accept clicks
//! in the caption strip instead of starting a window drag.
//!
//! The raw `WM_NCCALCSIZE` / `WM_NCHITTEST` handling lives in [`crate::sys::nc`];
//! this is the safe store it reads and the widget layer writes.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use crate::geometry::{Point, Rect};
use crate::hwnd::Hwnd;

#[derive(Default)]
struct Entry {
    extended: bool,
    /// The caption buttons' bounds in client coordinates (empty when unknown).
    caption_inset: Rect,
    /// The height of the extended strip (the caption incl. its top frame), in
    /// device pixels. Zero until the frame is first extended.
    strip_height: i32,
    /// The strip's self-drawn menu row height, in device pixels, when the strip
    /// menu is active; zero otherwise.
    menu_row: i32,
    /// The strip menu's item rectangles in client coordinates.
    menu_items: Vec<Rect>,
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

/// Records the extended strip's height (device pixels) for `window`.
pub(crate) fn set_strip_height(window: Hwnd, height: i32) {
    WINDOWS.with(|map| {
        map.borrow_mut()
            .entry(window.raw())
            .or_default()
            .strip_height = height;
    });
}

/// The extended strip's height (device pixels) for `window`, or 0 when unknown.
pub(crate) fn strip_height(window: Hwnd) -> i32 {
    WINDOWS.with(|map| {
        map.borrow()
            .get(&window.raw())
            .map_or(0, |entry| entry.strip_height)
    })
}

/// Records the strip menu's row height and item rectangles (client
/// coordinates) for `window`. `height` is the extra row the menu adds below the
/// caption (zero when the menu is inline); the items are its clickable
/// rectangles.
pub(crate) fn set_menu_strip(window: Hwnd, height: i32, items: Vec<Rect>) {
    WINDOWS.with(|map| {
        let mut map = map.borrow_mut();
        let entry = map.entry(window.raw()).or_default();
        entry.menu_row = height;
        entry.menu_items = items;
    });
}

/// The strip menu's row height (device pixels) for `window`, or 0 when the
/// strip menu is not active.
pub(crate) fn menu_row(window: Hwnd) -> i32 {
    WINDOWS.with(|map| {
        map.borrow()
            .get(&window.raw())
            .map_or(0, |entry| entry.menu_row)
    })
}

/// Whether `point` (client coordinates) lies on a strip menu item. The stored
/// rectangles are already in client coordinates.
pub(crate) fn over_menu_item(window: Hwnd, point: Point) -> bool {
    WINDOWS.with(|map| {
        map.borrow()
            .get(&window.raw())
            .is_some_and(|entry| entry.menu_items.iter().any(|rect| rect.contains(point)))
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
