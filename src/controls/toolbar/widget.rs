#![forbid(unsafe_code)]

//! The toolbar's state: item rectangles, hit testing and input transitions.

use std::cell::{Cell, RefCell};

use crate::accessibility::{AccessCx, Action, Node};
use crate::controls::custom::{CustomWidget, Input, Renderer, WidgetCx};
use crate::controls::toolbar::theme::ToolbarTheme;
use crate::controls::toolbar::{LabelMode, ToolbarItem, ToolbarItemId};
use crate::controls::toolbar_icon::{ICON_SIZE, ResolvedIcon, label_font, resolve};
use crate::d2d::{D2dCanvas, Layout, PointF, RectF, pixels_to_dips};
use crate::geometry::{Rect, Size};
use crate::message::MouseButton;
use crate::theme::Theme;
use crate::units::dip;

use super::item::ItemKind;

/// The horizontal padding inside a button.
pub(super) const PAD_X: f32 = 10.0;
/// The inset of the visible button face inside its cell.
pub(super) const INSET: f32 = 2.0;
/// The gap between an icon and the label beside or below it.
pub(super) const GAP: f32 = 6.0;
/// The vertical padding inside a button.
pub(super) const V_PAD: f32 = 6.0;
/// The minimum button width.
pub(super) const MIN_BUTTON: f32 = 32.0;
/// The width one separator occupies, including its margins.
const SEPARATOR_DIP: f32 = 9.0;
/// The width of a fixed spacer.
const SPACER_DIP: f32 = 6.0;
/// The fallback character width used to size a label when DirectWrite is
/// unavailable (the toolbar then paints nothing but its background).
const FALLBACK_CHAR: f32 = 7.0;

/// What a toolbar raises to the app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToolbarEvent {
    /// The button at `index` was clicked.
    Click(usize),
    /// The toggle at `index` changed to `checked`.
    Toggle(usize, bool),
}

/// The mutable state behind a [`Toolbar`](super::Toolbar).
pub(crate) struct ToolbarWidget<M> {
    pub(crate) items: Vec<ToolbarItem<M>>,
    pub(super) resolved: Vec<Option<ResolvedIcon>>,
    pub(super) labels: Vec<Option<Layout>>,
    widths_dip: Vec<f32>,
    height_dip: f32,
    pub(super) enabled: RefCell<Vec<bool>>,
    pub(super) checked: RefCell<Vec<bool>>,
    pub(super) hover: Cell<Option<usize>>,
    pub(super) pressed: Cell<Option<usize>>,
}

impl<M> ToolbarWidget<M> {
    pub(crate) fn new(items: Vec<ToolbarItem<M>>) -> ToolbarWidget<M> {
        let mut resolved = Vec::with_capacity(items.len());
        let mut labels = Vec::with_capacity(items.len());
        let mut widths_dip = Vec::with_capacity(items.len());
        for item in &items {
            let icon = item.icon.as_ref().map(resolve);
            let label = label_layout(item);
            widths_dip.push(item_width(item, &label));
            resolved.push(icon);
            labels.push(label);
        }
        let line = labels
            .iter()
            .flatten()
            .map(Layout::height)
            .fold(0.0_f32, f32::max);
        let height_dip = compute_height(&items, line);
        let enabled = items.iter().map(|item| item.enabled).collect();
        let checked = items.iter().map(|item| item.checked).collect();
        ToolbarWidget {
            items,
            resolved,
            labels,
            widths_dip,
            height_dip,
            enabled: RefCell::new(enabled),
            checked: RefCell::new(checked),
            hover: Cell::new(None),
            pressed: Cell::new(None),
        }
    }

    /// The toolbar height in device pixels at `dpi`.
    pub(crate) fn height_px(&self, dpi: u32) -> i32 {
        dip_to_px(self.height_dip, dpi).round() as i32
    }

    /// The item rectangles at a client `width` in device-independent pixels.
    pub(super) fn layout(&self, width: f32) -> Vec<RectF> {
        let fixed: f32 = self
            .items
            .iter()
            .zip(&self.widths_dip)
            .filter(|(item, _)| !is_flexible(item))
            .map(|(_, width)| *width)
            .sum();
        let flexible = self.items.iter().filter(|item| is_flexible(item)).count();
        let each = if flexible == 0 {
            0.0
        } else {
            (width - fixed).max(0.0) / flexible as f32
        };
        let mut rects = Vec::with_capacity(self.items.len());
        let mut x = 0.0;
        for (item, width) in self.items.iter().zip(&self.widths_dip) {
            let width = if is_flexible(item) { each } else { *width };
            rects.push(RectF::new(x, 0.0, x + width, self.height_dip));
            x += width;
        }
        rects
    }

    fn rects_px(&self, width_px: i32, dpi: u32) -> Vec<RectF> {
        self.layout(pixels_to_dips(width_px, dpi))
    }

    /// The index of an enabled button under `point` (dip), or `None`.
    fn hit_test(&self, point: PointF, rects: &[RectF]) -> Option<usize> {
        let enabled = self.enabled.borrow();
        self.items.iter().enumerate().position(|(index, item)| {
            item.kind == ItemKind::Button && enabled[index] && inside(rects[index], point)
        })
    }

    /// The index of the item with `id`, or `None`.
    pub(super) fn index_of(&self, id: ToolbarItemId) -> Option<usize> {
        self.items
            .iter()
            .enumerate()
            .position(|(index, item)| item_id(item, index) == id)
    }

    /// Sets the enabled state of the item at `index`. Returns whether it
    /// changed.
    pub(super) fn set_enabled(&mut self, index: usize, enabled: bool) -> bool {
        let mut states = self.enabled.borrow_mut();
        let Some(state) = states.get_mut(index) else {
            return false;
        };
        let changed = *state != enabled;
        *state = enabled;
        changed
    }

    /// Sets the checked state of the item at `index`. Returns whether it
    /// changed.
    pub(super) fn set_checked(&mut self, index: usize, checked: bool) -> bool {
        let mut states = self.checked.borrow_mut();
        let Some(state) = states.get_mut(index) else {
            return false;
        };
        let changed = *state != checked;
        *state = checked;
        changed
    }

    /// The button cell rectangle in device pixels, for a state-change
    /// invalidation.
    pub(super) fn cell_rect_px(&self, index: usize, width_px: i32, dpi: u32) -> Option<Rect> {
        let rects = self.rects_px(width_px, dpi);
        rects.get(index).copied().map(|rect| to_px(rect, dpi))
    }

    /// One `(slot, rect, text)` per button, for the shared tooltip.
    pub(super) fn tooltip_regions(&self, width_px: i32, dpi: u32) -> Vec<(usize, Rect, String)> {
        let rects = self.rects_px(width_px, dpi);
        self.items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.kind == ItemKind::Button)
            .filter_map(|(index, item)| {
                let rect = *rects.get(index)?;
                Some((
                    index,
                    to_px(rect, dpi),
                    item.tooltip_text().unwrap_or_default(),
                ))
            })
            .collect()
    }

    /// Applies one input event, returning the raised event and the dirty
    /// rectangles (in dip) to repaint.
    pub(super) fn interact(
        &self,
        input: Input,
        dpi: u32,
        bounds: Rect,
    ) -> (Option<ToolbarEvent>, Vec<RectF>) {
        let rects = self.rects_px(bounds.width(), dpi);
        let point = |x: i32, y: i32| PointF::new(pixels_to_dips(x, dpi), pixels_to_dips(y, dpi));
        match input {
            Input::MouseMove { x, y, .. } => {
                let hover = self.hit_test(point(x, y), &rects);
                self.changed(self.hover.replace(hover), hover, &rects)
            }
            Input::MouseDown {
                x,
                y,
                button: MouseButton::Left,
                ..
            } => {
                let pressed = self.hit_test(point(x, y), &rects);
                self.changed(self.pressed.replace(pressed), pressed, &rects)
            }
            Input::MouseUp {
                x,
                y,
                button: MouseButton::Left,
                ..
            } => {
                let pressed = self.pressed.take();
                let hit = self.hit_test(point(x, y), &rects);
                let (_, mut dirty) = self.changed(pressed, hit, &rects);
                let event = match (pressed, hit) {
                    (Some(pressed), Some(hit)) if pressed == hit => {
                        let (event, toggled) = self.activate_event(hit);
                        if toggled {
                            dirty.push(rects[hit]);
                        }
                        Some(event)
                    }
                    _ => None,
                };
                (event, dirty)
            }
            Input::MouseLeave => {
                let old = self.hover.take();
                self.changed(old, None, &rects)
            }
            _ => (None, Vec::new()),
        }
    }

    /// Flips a toggle's checked state and returns its event; a plain button
    /// raises a click. The bool is whether the item's appearance changed (a
    /// toggle flipped), so the caller repaints it.
    fn activate_event(&self, index: usize) -> (ToolbarEvent, bool) {
        if !self.items[index].toggled {
            return (ToolbarEvent::Click(index), false);
        }
        let mut checked = self.checked.borrow_mut();
        checked[index] = !checked[index];
        (ToolbarEvent::Toggle(index, checked[index]), true)
    }

    /// The item index of the `ordinal`-th button (skipping separators and
    /// spacers), for an accessibility path.
    fn button_index(&self, ordinal: usize) -> Option<usize> {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.is_button())
            .nth(ordinal)
            .map(|(index, _)| index)
    }

    /// The device-pixel rectangle of every item at the current client `bounds`,
    /// for the accessibility tree.
    pub(crate) fn accessibility_rects(&self, bounds: Rect, dpi: u32) -> Vec<Rect> {
        self.rects_px(bounds.width(), dpi)
            .into_iter()
            .map(|rect| to_px(rect, dpi))
            .collect()
    }

    /// The dirty rectangles for a hover/press change from `old` to `new`.
    fn changed(
        &self,
        old: Option<usize>,
        new: Option<usize>,
        rects: &[RectF],
    ) -> (Option<ToolbarEvent>, Vec<RectF>) {
        let mut dirty = Vec::new();
        for index in [old, new].into_iter().flatten() {
            if let Some(rect) = rects.get(index) {
                dirty.push(*rect);
            }
        }
        (None, dirty)
    }
}

impl<M: 'static> CustomWidget for ToolbarWidget<M> {
    type Event = ToolbarEvent;

    /// Never called: the toolbar paints with Direct2D, and when Direct2D is
    /// unavailable the host fills the theme background instead.
    fn paint(&self, _canvas: &crate::gdi::Canvas, _bounds: Rect, _theme: &Theme) {}

    fn renderer(&self) -> Renderer {
        Renderer::Direct2D
    }

    fn paint_d2d(&self, canvas: &mut D2dCanvas<'_>, bounds: RectF, theme: &Theme) {
        let theme = ToolbarTheme::from_theme(theme);
        super::paint::draw(self, canvas, bounds, &theme);
    }

    fn input(&self, input: Input, cx: &mut WidgetCx<ToolbarEvent>) {
        let (event, dirty) = self.interact(input, cx.dpi(), cx.bounds());
        for rect in dirty {
            cx.invalidate_rect(to_px(rect, cx.dpi()));
        }
        if let Some(event) = event {
            cx.emit(event);
        }
    }

    fn preferred_size(&self, dpi: u32) -> Option<Size> {
        let width: f32 = self.widths_dip.iter().sum();
        Some(Size::new(
            dip_to_px(width, dpi).round() as i32,
            self.height_px(dpi),
        ))
    }

    fn accessibility(&self, cx: &AccessCx) -> Option<Node> {
        Some(crate::controls::toolbar_access::node(self, cx))
    }

    fn accessibility_action(
        &self,
        path: &[usize],
        action: Action,
        cx: &mut WidgetCx<ToolbarEvent>,
    ) -> bool {
        let ([ordinal], Action::Invoke) = (path, action) else {
            return false;
        };
        let Some(index) = self.button_index(*ordinal) else {
            return false;
        };
        let (event, toggled) = self.activate_event(index);
        if toggled && let Some(rect) = self.accessibility_rects(cx.bounds(), cx.dpi()).get(index) {
            cx.invalidate_rect(*rect);
        }
        cx.emit(event);
        true
    }
}

/// The id of `item`, or its position when it has no explicit id.
pub(super) fn item_id<M>(item: &ToolbarItem<M>, index: usize) -> ToolbarItemId {
    item.id.unwrap_or(ToolbarItemId::new(index as u64))
}

fn is_flexible<M>(item: &ToolbarItem<M>) -> bool {
    matches!(item.kind, ItemKind::Spacer { flexible: true })
}

fn label_layout<M>(item: &ToolbarItem<M>) -> Option<Layout> {
    if item.kind != ItemKind::Button || item.label.is_empty() {
        return None;
    }
    label_font().and_then(|font| font.layout(&item.label, f32::INFINITY).ok())
}

fn item_width<M>(item: &ToolbarItem<M>, label: &Option<Layout>) -> f32 {
    match item.kind {
        ItemKind::Separator => SEPARATOR_DIP,
        ItemKind::Spacer { flexible } => {
            if flexible {
                0.0
            } else {
                SPACER_DIP
            }
        }
        ItemKind::Button => {
            let text = label.as_ref().map_or_else(
                || item.label.chars().count() as f32 * FALLBACK_CHAR,
                Layout::width,
            );
            let has_icon = item.icon.is_some();
            let has_text = !item.label.is_empty();
            let width = match item.label_mode {
                LabelMode::IconOnly => {
                    if has_icon {
                        ICON_SIZE
                    } else {
                        text
                    }
                }
                LabelMode::TextUnder => ICON_SIZE.max(text),
                LabelMode::IconText | LabelMode::TextWhenChecked => {
                    let gap = if has_icon && has_text { GAP } else { 0.0 };
                    (if has_icon { ICON_SIZE } else { 0.0 }) + gap + text
                }
            };
            (width + PAD_X * 2.0).max(MIN_BUTTON)
        }
    }
}

fn compute_height<M>(items: &[ToolbarItem<M>], line: f32) -> f32 {
    let line = if line > 0.0 {
        line
    } else {
        crate::controls::toolbar_icon::LABEL_SIZE * 1.35
    };
    let mut height = line + V_PAD * 2.0;
    for item in items {
        if item.kind != ItemKind::Button {
            continue;
        }
        let has_icon = item.icon.is_some();
        let has_text = !item.label.is_empty();
        let mut item_height = line + V_PAD * 2.0;
        if has_icon {
            item_height = item_height.max(ICON_SIZE + V_PAD * 2.0);
        }
        if item.label_mode == LabelMode::TextUnder && has_icon && has_text {
            item_height = (ICON_SIZE + GAP + line + V_PAD * 2.0).max(item_height);
        }
        height = height.max(item_height);
    }
    height
}

fn inside(rect: RectF, point: PointF) -> bool {
    point.x >= rect.left && point.x < rect.right && point.y >= rect.top && point.y < rect.bottom
}

pub(super) fn dip_to_px(value: f32, dpi: u32) -> f32 {
    dip(value).to_px(dpi).value() as f32
}

pub(super) fn to_px(rect: RectF, dpi: u32) -> Rect {
    Rect::new(
        dip_to_px(rect.left, dpi).round() as i32,
        dip_to_px(rect.top, dpi).round() as i32,
        dip_to_px(rect.right, dpi).round() as i32,
        dip_to_px(rect.bottom, dpi).round() as i32,
    )
}
