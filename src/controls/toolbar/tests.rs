#![cfg(test)]

//! Unit tests for the toolbar's layout and input transitions, without a window.

use crate::controls::custom::{CustomWidget, Input};
use crate::controls::toolbar_icon::ToolbarIcon;
use crate::geometry::Rect;
use crate::message::{Modifiers, MouseButton};

use super::widget::{ToolbarEvent, ToolbarWidget};
use super::{LabelMode, ToolbarItem, ToolbarItemId};

const DPI: u32 = 96;

fn widget(items: Vec<ToolbarItem<()>>) -> ToolbarWidget<()> {
    ToolbarWidget::new(items)
}

/// The client point at the centre of item `index` at `width` dip.
fn center(widget: &ToolbarWidget<()>, index: usize, width: f32) -> (i32, i32) {
    let rect = widget.layout(width)[index];
    (
        ((rect.left + rect.right) / 2.0) as i32,
        ((rect.top + rect.bottom) / 2.0) as i32,
    )
}

fn mouse(x: i32, y: i32, down: bool) -> Input {
    if down {
        Input::MouseDown {
            x,
            y,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        }
    } else {
        Input::MouseUp {
            x,
            y,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        }
    }
}

/// Presses and releases the centre of item `index`, and returns the event.
fn click(widget: &ToolbarWidget<()>, index: usize, width: f32) -> Option<ToolbarEvent> {
    let (x, y) = center(widget, index, width);
    let bounds = Rect::new(0, 0, width as i32, widget.height_px(DPI));
    widget.interact(mouse(x, y, true), DPI, bounds);
    let (event, _) = widget.interact(mouse(x, y, false), DPI, bounds);
    event
}

#[test]
fn a_disabled_button_emits_nothing() {
    let widget = widget(vec![
        ToolbarItem::new("Delete")
            .with_icon(ToolbarIcon::Delete)
            .enabled(false),
    ]);
    assert_eq!(click(&widget, 0, 200.0), None);
}

#[test]
fn a_toggle_flips_its_checked_state_and_emits_once_per_click() {
    let widget = widget(vec![
        ToolbarItem::new("Star")
            .with_icon(ToolbarIcon::Star)
            .toggle(),
    ]);
    assert_eq!(
        click(&widget, 0, 200.0),
        Some(ToolbarEvent::Toggle(0, true))
    );
    assert_eq!(
        click(&widget, 0, 200.0),
        Some(ToolbarEvent::Toggle(0, false))
    );
}

#[test]
fn a_plain_button_emits_a_click() {
    let widget = widget(vec![ToolbarItem::new("Reply").on_click(|| None)]);
    assert_eq!(click(&widget, 0, 200.0), Some(ToolbarEvent::Click(0)));
}

#[test]
fn a_separator_and_a_spacer_do_not_respond() {
    let widget = widget(vec![
        ToolbarItem::separator(),
        ToolbarItem::spacer(),
        ToolbarItem::new("A"),
    ]);
    let bounds = Rect::new(0, 0, 200, widget.height_px(DPI));
    // Hovering the separator must not produce a hover dirty rectangle.
    let (x, y) = center(&widget, 0, 200.0);
    let (_, dirty) = widget.interact(
        Input::MouseMove {
            x,
            y,
            modifiers: Modifiers::default(),
        },
        DPI,
        bounds,
    );
    assert!(dirty.is_empty(), "a separator repainted on hover");
}

#[test]
fn a_flexible_spacer_pushes_later_items_to_the_right() {
    let widget = widget(vec![
        ToolbarItem::new("A"),
        ToolbarItem::flexible_spacer(),
        ToolbarItem::new("B"),
    ]);
    let rects = widget.layout(500.0);
    assert_eq!(rects[0].left, 0.0);
    assert!(
        (rects[2].right - 500.0).abs() < 0.5,
        "the last item should sit at the right edge, got {}",
        rects[2].right
    );
}

#[test]
fn an_icon_only_button_is_narrower_than_an_icon_text_button() {
    let icon_only = widget(vec![
        ToolbarItem::new("Compose")
            .with_icon(ToolbarIcon::Compose)
            .label_mode(LabelMode::IconOnly),
    ]);
    let icon_text = widget(vec![
        ToolbarItem::new("Compose").with_icon(ToolbarIcon::Compose),
    ]);
    assert!(
        icon_only.preferred_size(DPI).unwrap().width < icon_text.preferred_size(DPI).unwrap().width
    );
}

#[test]
fn a_text_under_icon_button_is_taller() {
    let icon_text = widget(vec![
        ToolbarItem::new("Compose").with_icon(ToolbarIcon::Compose),
    ]);
    let text_under = widget(vec![
        ToolbarItem::new("Compose")
            .with_icon(ToolbarIcon::Compose)
            .label_mode(LabelMode::TextUnder),
    ]);
    assert!(text_under.height_px(DPI) > icon_text.height_px(DPI));
}

#[test]
fn an_item_with_an_id_can_be_found_and_toggled() {
    let mut widget = widget(vec![ToolbarItem::new("A"), ToolbarItem::new("B").id(42u32)]);
    assert_eq!(widget.index_of(ToolbarItemId::new(42)), Some(1));
    assert!(widget.set_checked(1, true));
    assert!(
        !widget.set_checked(1, true),
        "setting the same state is a no-op"
    );
    assert!(widget.set_enabled(1, false));
}
