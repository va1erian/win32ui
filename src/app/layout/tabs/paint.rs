#![forbid(unsafe_code)]

//! Tab painting: each tab through `WM_DRAWITEM`, and the strip chrome pass.

use std::rc::Weak;

use crate::app::Ui;
use crate::app::core::Core;
use crate::geometry::Rect;
use crate::sys;
use crate::theme::Theme;

use super::TabsShared;

/// Paints one tab from the window's current theme. `state` is the
/// `WM_DRAWITEM` state when the control asked for the tab, or `None` when the
/// chrome pass repaints a tab after the native frame.
pub(super) fn paint_tab<M: 'static>(
    shared: &TabsShared,
    core: &Weak<Core<M>>,
    item: usize,
    state: Option<u32>,
    dc: isize,
    area: Rect,
) {
    let theme = theme_of(core);
    let titles = shared.titles.borrow();
    let Some(title) = titles.get(item) else {
        return;
    };
    let bound = shared.bound.borrow();
    let Some(bound) = bound.as_ref() else {
        return;
    };
    let mut visual = match state {
        Some(state) => sys::tabs::decode_state(state),
        None => sys::tabs::TabVisual {
            selected: shared.selected.get() == item,
            focused: false,
            hot: false,
            disabled: false,
        },
    };
    visual.hot = shared.hot.get() == Some(item);
    if state.is_none() {
        visual.focused = visual.selected && sys::tabs::has_focus(shared.hwnd.get());
    }
    sys::tabs::draw_tab(dc, area, title, bound.font.raw(), visual, &paint_of(&theme));
}

/// Repaints the parts of the native tab control that ignore dark mode: the
/// strip background and the control's frame around the display area. The tabs
/// are redrawn from tokens too, so no native chrome survives.
pub(super) fn paint_chrome<M: 'static>(
    shared: &TabsShared,
    core: &Weak<Core<M>>,
    dc: isize,
    bounds: Rect,
    theme: &Theme,
) {
    let hwnd = shared.hwnd.get();
    let count = shared.count.get();
    if !hwnd.is_alive() || count == 0 {
        return;
    }
    let display = sys::tabs::page_rect(hwnd, bounds);
    let background = theme.background;

    sys::tabs::fill(
        dc,
        Rect::new(bounds.left, bounds.top, bounds.right, display.top),
        background,
    );
    for index in 0..count {
        if let Some(rect) = sys::tabs::tab_rect(hwnd, index) {
            paint_tab(shared, core, index, None, dc, rect);
        }
    }
    // The display-area frame's left, right and bottom edges.
    sys::tabs::fill(
        dc,
        Rect::new(bounds.left, display.top, display.left, bounds.bottom),
        background,
    );
    sys::tabs::fill(
        dc,
        Rect::new(display.right, display.top, bounds.right, bounds.bottom),
        background,
    );
    sys::tabs::fill(
        dc,
        Rect::new(display.left, display.bottom, display.right, bounds.bottom),
        background,
    );
}

/// The window's theme, or the light one once the window is gone.
pub(super) fn theme_of<M: 'static>(core: &Weak<Core<M>>) -> Theme {
    core.upgrade()
        .map(|core| Ui::new(core).theme())
        .unwrap_or_else(Theme::light)
}

/// Derives the tab palette from semantic tokens.
fn paint_of(theme: &Theme) -> sys::tabs::TabPaint {
    sys::tabs::TabPaint {
        background: theme.background,
        raised: theme.raised,
        hover: theme.hover,
        text: theme.text,
        text_secondary: theme.text_secondary,
        text_disabled: theme.text_disabled,
        accent: theme.accent,
    }
}
