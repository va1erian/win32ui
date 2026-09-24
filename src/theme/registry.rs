#![forbid(unsafe_code)]

//! Per-window themes and the themed-children registry.
//!
//! The widget layer keeps its theme on [`Ui`](crate::Ui) and registers every
//! widget created through it here, keyed by the top-level window. The
//! platform layer uses the same map through [`Window::set_theme`](crate::Window).
//! [`Window::set_theme`](crate::Window) re-themes every registered child and
//! repaints once; dropping a widget unregisters it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::color::Color;
use crate::hwnd::Hwnd;

use super::tokens::Theme;

/// A type-erased re-theme callback stored per child.
pub(crate) type ApplyTheme = Rc<dyn Fn(&Theme)>;

#[derive(Default)]
struct WindowEntry {
    theme: Option<Theme>,
    /// Whether DWM is drawing a backdrop material behind this window's client
    /// area, so the class background must stay transparent black.
    backdrop_active: bool,
    children: Vec<(usize, ApplyTheme)>,
    /// Set by `Window::follow_system_theme`/`Ui::follow_system_theme`: applies
    /// a freshly read [`Theme::system`] when a theme-change message arrives.
    follow_system: Option<ApplyTheme>,
}

thread_local! {
    static WINDOWS: RefCell<HashMap<usize, WindowEntry>> = RefCell::new(HashMap::new());
}

/// Stores `theme` as the window's theme.
pub(crate) fn set_window_theme(window: Hwnd, theme: Theme) {
    WINDOWS.with(|map| {
        map.borrow_mut().entry(window.raw()).or_default().theme = Some(theme);
    });
}

/// The window's theme, or [`Theme::light`] when none was stored.
pub(crate) fn window_theme(window: Hwnd) -> Theme {
    WINDOWS.with(|map| {
        map.borrow()
            .get(&window.raw())
            .and_then(|entry| entry.theme)
            .unwrap_or_else(Theme::light)
    })
}

/// Records whether `window` has an active backdrop material.
pub(crate) fn set_backdrop_active(window: Hwnd, active: bool) {
    WINDOWS.with(|map| {
        map.borrow_mut()
            .entry(window.raw())
            .or_default()
            .backdrop_active = active;
    });
}

/// Whether `window` has an active backdrop material.
pub(crate) fn backdrop_active(window: Hwnd) -> bool {
    WINDOWS.with(|map| {
        map.borrow()
            .get(&window.raw())
            .is_some_and(|entry| entry.backdrop_active)
    })
}

/// The class background a window should paint: always the theme's background.
/// With an extended frame only the caption strip is glass; it is cleared to
/// black by the non-client erase path, never by painting the whole client
/// black (which would leave black bands wherever no control paints).
pub(crate) fn window_background(_window: Hwnd, theme: Theme) -> Color {
    theme.background
}

/// Registers `child` (created under `window`) with its re-theme callback.
/// Re-registering the same child replaces its callback.
pub(crate) fn register_child(window: Hwnd, child: Hwnd, apply: ApplyTheme) {
    WINDOWS.with(|map| {
        let mut map = map.borrow_mut();
        let entry = map.entry(window.raw()).or_default();
        if let Some(slot) = entry
            .children
            .iter_mut()
            .find(|(hwnd, _)| *hwnd == child.raw())
        {
            *slot = (child.raw(), apply);
        } else {
            entry.children.push((child.raw(), apply));
        }
    });
}

/// Removes any registration for `child`, returning whether one existed.
pub(crate) fn unregister_child(child: Hwnd) -> bool {
    WINDOWS.with(|map| {
        let mut map = map.borrow_mut();
        let mut removed = false;
        let mut empty = Vec::new();
        for (window, entry) in map.iter_mut() {
            let before = entry.children.len();
            entry.children.retain(|(hwnd, _)| *hwnd != child.raw());
            if entry.children.len() != before {
                removed = true;
            }
            if entry.children.is_empty() && entry.theme.is_none() && !entry.backdrop_active {
                empty.push(*window);
            }
        }
        for window in empty {
            map.remove(&window);
        }
        removed
    })
}

/// Whether `child` is currently registered.
#[cfg(test)]
pub(crate) fn is_registered(child: Hwnd) -> bool {
    WINDOWS.with(|map| {
        map.borrow()
            .values()
            .any(|entry| entry.children.iter().any(|(hwnd, _)| *hwnd == child.raw()))
    })
}

/// The number of themed children registered under `window`.
#[cfg(test)]
pub(crate) fn child_count(window: Hwnd) -> usize {
    WINDOWS.with(|map| {
        map.borrow()
            .get(&window.raw())
            .map(|entry| entry.children.len())
            .unwrap_or(0)
    })
}

/// Re-themes every child registered under `window`.
pub(crate) fn retheme_children(window: Hwnd, theme: &Theme) {
    let callbacks: Vec<ApplyTheme> = WINDOWS.with(|map| {
        map.borrow()
            .get(&window.raw())
            .map(|entry| {
                entry
                    .children
                    .iter()
                    .map(|(_, apply)| apply.clone())
                    .collect()
            })
            .unwrap_or_default()
    });
    for apply in callbacks {
        apply(theme);
    }
}

/// Sets (or clears, with `None`) the callback that applies a freshly read
/// [`Theme::system`](super::Theme::system) when `window` gets a theme-change
/// message.
pub(crate) fn set_follow_system(window: Hwnd, apply: Option<ApplyTheme>) {
    WINDOWS.with(|map| {
        let mut map = map.borrow_mut();
        match apply {
            Some(apply) => map.entry(window.raw()).or_default().follow_system = Some(apply),
            None => {
                if let Some(entry) = map.get_mut(&window.raw()) {
                    entry.follow_system = None;
                }
            }
        }
    });
}

/// The callback registered by [`set_follow_system`] for `window`, if any.
pub(crate) fn follow_system_apply(window: Hwnd) -> Option<ApplyTheme> {
    WINDOWS.with(|map| {
        map.borrow()
            .get(&window.raw())
            .and_then(|entry| entry.follow_system.clone())
    })
}

/// Drops a destroyed window's theme and children.
pub(crate) fn forget_window(window: Hwnd) {
    WINDOWS.with(|map| {
        map.borrow_mut().remove(&window.raw());
    });
}

#[cfg(test)]
mod tests {
    use super::{child_count, is_registered, register_child, retheme_children};
    use super::{set_window_theme, unregister_child, window_theme};
    use crate::hwnd::Hwnd;
    use crate::theme::Theme;
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn window_theme_defaults_to_light() {
        let window = Hwnd::from_raw(0x3001);
        assert_eq!(window_theme(window), Theme::light());
        super::forget_window(window);
    }

    #[test]
    fn set_and_read_window_theme() {
        let window = Hwnd::from_raw(0x3002);
        set_window_theme(window, Theme::dark());
        assert_eq!(window_theme(window), Theme::dark());
        super::forget_window(window);
    }

    #[test]
    fn destroyed_child_is_removed() {
        let window = Hwnd::from_raw(0x3003);
        let child = Hwnd::from_raw(0x3004);
        let applied = Rc::new(Cell::new(false));
        let applied_for_callback = applied.clone();
        register_child(
            window,
            child,
            Rc::new(move |_| applied_for_callback.set(true)),
        );
        assert!(is_registered(child));
        assert_eq!(child_count(window), 1);

        retheme_children(window, &Theme::dark());
        assert!(applied.get());

        assert!(unregister_child(child));
        assert!(!is_registered(child));
        assert_eq!(child_count(window), 0);
        super::forget_window(window);
    }
}
