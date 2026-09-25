#![forbid(unsafe_code)]

//! The toolbar's Direct2D painter: state faces, separators, icons and labels.

use crate::controls::toolbar::LabelMode;
use crate::controls::toolbar::theme::ToolbarTheme;
use crate::controls::toolbar_icon::{ICON_SIZE, draw as draw_icon};
use crate::d2d::{D2dCanvas, Layout, PointF, RectF};

use super::item::ItemKind;
use super::widget::{GAP, INSET, ToolbarWidget, V_PAD};

/// Paints the whole toolbar.
pub(super) fn draw<M>(
    widget: &ToolbarWidget<M>,
    canvas: &mut D2dCanvas<'_>,
    bounds: RectF,
    theme: &ToolbarTheme,
) {
    canvas.fill_rect(bounds, theme.background);
    let rects = widget.layout(bounds.width());
    let enabled = widget.enabled.borrow();
    let checked = widget.checked.borrow();
    for (index, item) in widget.items.iter().enumerate() {
        let cell = rects[index];
        match item.kind {
            ItemKind::Separator => {
                let center = (cell.left + cell.right) / 2.0;
                canvas.fill_rect(
                    RectF::new(center, cell.top + V_PAD, center + 1.0, cell.bottom - V_PAD),
                    theme.separator,
                );
            }
            ItemKind::Spacer { .. } => {}
            ItemKind::Button => draw_button(
                widget,
                canvas,
                index,
                inset(cell),
                theme,
                enabled[index],
                checked[index],
            ),
        }
    }
    canvas.fill_rect(
        RectF::new(0.0, bounds.bottom - 1.0, bounds.right, bounds.bottom),
        theme.border,
    );
}

/// Paints one button: its state face, then its icon and label.
fn draw_button<M>(
    widget: &ToolbarWidget<M>,
    canvas: &mut D2dCanvas<'_>,
    index: usize,
    button: RectF,
    theme: &ToolbarTheme,
    enabled: bool,
    checked: bool,
) {
    let toggled = widget.items[index].toggled;
    let background = if !enabled {
        theme.button
    } else if toggled && checked {
        theme.button_checked
    } else if widget.pressed.get() == Some(index) {
        theme.button_pressed
    } else if widget.hover.get() == Some(index) {
        theme.button_hover
    } else {
        theme.button
    };
    canvas.fill_rounded_rect(button, 4.0, background);

    let color = if !enabled {
        theme.text_disabled
    } else if toggled && checked {
        theme.text_on_accent
    } else {
        theme.text
    };
    let mode = match widget.items[index].label_mode {
        LabelMode::TextWhenChecked if !checked => LabelMode::IconOnly,
        mode => mode,
    };
    let icon = widget.resolved.get(index).and_then(Option::as_ref);
    let label = widget.labels.get(index).and_then(Option::as_ref);
    let icon_width = if icon.is_some() { ICON_SIZE } else { 0.0 };

    match mode {
        LabelMode::IconOnly => {
            if let Some(icon) = icon {
                draw_icon(canvas, icon, centered(button, ICON_SIZE), color);
            }
        }
        LabelMode::IconText | LabelMode::TextWhenChecked => {
            let gap = if icon.is_some() && label.is_some() {
                GAP
            } else {
                0.0
            };
            let content = icon_width + gap + label.map_or(0.0, Layout::width);
            let start = button.left + (button.width() - content) / 2.0;
            if let Some(icon) = icon {
                draw_icon(
                    canvas,
                    icon,
                    RectF::new(
                        start,
                        button.top + (button.height() - ICON_SIZE) / 2.0,
                        start + ICON_SIZE,
                        button.top + (button.height() + ICON_SIZE) / 2.0,
                    ),
                    color,
                );
            }
            if let Some(label) = label {
                let y = button.top + (button.height() - label.height()) / 2.0;
                canvas.draw_text(label, PointF::new(start + icon_width + gap, y), color);
            }
        }
        LabelMode::TextUnder => {
            if let Some(icon) = icon {
                draw_icon(
                    canvas,
                    icon,
                    RectF::new(
                        button.left + (button.width() - ICON_SIZE) / 2.0,
                        button.top + V_PAD,
                        button.left + (button.width() + ICON_SIZE) / 2.0,
                        button.top + V_PAD + ICON_SIZE,
                    ),
                    color,
                );
            }
            if let Some(label) = label {
                let x = button.left + (button.width() - label.width()) / 2.0;
                let y = button.top + V_PAD + ICON_SIZE + GAP;
                canvas.draw_text(label, PointF::new(x, y), color);
            }
        }
    }
}

fn inset(rect: RectF) -> RectF {
    RectF::new(
        rect.left + INSET,
        rect.top + INSET,
        rect.right - INSET,
        rect.bottom - INSET,
    )
}

fn centered(button: RectF, size: f32) -> RectF {
    RectF::new(
        button.left + (button.width() - size) / 2.0,
        button.top + (button.height() - size) / 2.0,
        button.left + (button.width() + size) / 2.0,
        button.top + (button.height() + size) / 2.0,
    )
}
