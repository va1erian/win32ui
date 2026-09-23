//! Unit tests for the shared tooltip manager: the tools are added to and
//! removed from the one `tooltips_class32` window.

use super::{forget_widget, set_control_tooltip, set_region_tooltip, shared_hwnd};
use crate::color::Color;
use crate::geometry::Rect;
use crate::hwnd::Hwnd;
use crate::message::{LResult, Message};
use crate::sys;
use crate::window::{Window, WindowClass, WindowExStyle, WindowHandler, WindowStyle};

struct Null;

impl WindowHandler for Null {
    fn message(&self, _window: &Window, _message: Message) -> Option<LResult> {
        None
    }
}

/// A live top-level window with a live static child, or `None` when the
/// session cannot create windows.
fn fixture() -> Option<(Window, Hwnd)> {
    crate::init();
    let class = WindowClass::register("tooltip.test", Color::rgb(0xFF, 0xFF, 0xFF)).ok()?;
    let window = Window::create(
        class,
        None,
        WindowStyle::overlapped(),
        WindowExStyle::new(),
        Rect::new(0, 0, 240, 120),
        "tooltip test",
        Null,
    )
    .ok()?;
    let style = crate::controls::style::WS_CHILD | crate::controls::style::WS_VISIBLE;
    let child = sys::window::create_control(
        "STATIC",
        style,
        0,
        window.hwnd(),
        1,
        Rect::new(0, 0, 100, 20),
    )
    .ok()?;
    Some((window, sys::hwnd_from(child)))
}

/// A widget tooltip reaches the one shared `tooltips_class32` window, and
/// dropping the widget removes its tool.
#[test]
fn control_tooltip_is_added_and_removed() {
    let Some((window, child)) = fixture() else {
        return;
    };
    set_control_tooltip(child, "Hello");
    let tooltip = shared_hwnd(window.hwnd()).expect("shared tooltip was not created");
    assert_eq!(
        sys::tooltip::tool_text(tooltip, child, child.raw(), true).as_deref(),
        Some("Hello"),
        "the widget tooltip was not added"
    );

    forget_widget(child);
    assert!(
        sys::tooltip::tool_text(tooltip, child, child.raw(), true).is_none(),
        "the widget tooltip was not removed"
    );
    window.destroy();
}

/// A region tooltip is keyed by its slot, and updating it replaces the text in
/// place.
#[test]
fn region_tooltip_updates_in_place() {
    let Some((window, child)) = fixture() else {
        return;
    };
    set_region_tooltip(child, 3, Rect::new(0, 0, 20, 20), "First");
    let tooltip = shared_hwnd(window.hwnd()).expect("shared tooltip was not created");
    assert_eq!(
        sys::tooltip::tool_text(tooltip, child, 3, false).as_deref(),
        Some("First")
    );

    set_region_tooltip(child, 3, Rect::new(0, 0, 40, 20), "Second");
    assert_eq!(
        sys::tooltip::tool_text(tooltip, child, 3, false).as_deref(),
        Some("Second"),
        "the region tooltip text was not updated"
    );
    window.destroy();
}

/// The shown tooltip is at least as wide as its paint font needs for the full
/// text (shortcut suffix included): comctl32 sizes the window from the same
/// font the owner-draw paints with, so the text cannot be clipped. Regression
/// for the tooltip that read "Scan the library (Ct".
#[test]
fn shown_tooltip_fits_its_full_text() {
    let Some((window, child)) = fixture() else {
        return;
    };
    window.set_theme(crate::theme::Theme::dark());
    let text = "Scan the library (Ctrl+S)";
    set_region_tooltip(child, 5, Rect::new(0, 0, 80, 20), text);
    let tooltip = shared_hwnd(window.hwnd()).expect("the shared tooltip was not created");
    if !sys::tooltip::tests::show_tracking(tooltip, child, text) {
        window.destroy();
        return;
    }
    let rect = sys::tooltip::tests::window_rect(tooltip);
    let font = sys::tooltip::tests::paint_font(tooltip);
    let measured = sys::gdi::measure_text(font, text);
    let margin = crate::units::dip(5.0)
        .to_px(sys::dpi::window_dpi(tooltip))
        .value();
    sys::tooltip::tests::hide_tracking(tooltip, child);
    window.destroy();

    assert!(
        rect.width() >= measured.width,
        "the shown tooltip ({}px wide) is narrower than its text ({}px)",
        rect.width(),
        measured.width
    );
    // The owner-draw paints in `window` inset by `text_inset`; the text must fit
    // there, or it is clipped or wrapped (the original bug).
    let painted = rect.width() - super::text_inset(rect.width(), measured.width, margin) * 2;
    assert!(
        painted >= measured.width,
        "the tooltip's text area ({painted}px) is narrower than its text ({}px)",
        measured.width
    );
    assert!(
        rect.height() >= measured.height,
        "the shown tooltip ({}px tall) is shorter than its text ({}px)",
        rect.height(),
        measured.height
    );
}
