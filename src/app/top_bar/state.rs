#![forbid(unsafe_code)]

//! The material top bar's shared state: the flattened items, their measured
//! widths, the interaction state and the item rectangles, plus the mutators the
//! public [`MaterialTopBar`](super::MaterialTopBar) drives each sync.
//!
//! The state is `M`-free, so the [`Core`](crate::app::core::Core) can hold it
//! and the top-level window handler can paint and hit-test it; the app's event
//! mapping lives in `Core`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use crate::controls::slider::SliderState;
use crate::d2d::{Font, FontSpec, Layout, TextSystem};
use crate::geometry::Rect;

use super::layout::{self, Span};
use super::native::{self, Child};
use super::{DEFAULT_HEIGHT_DIP, TopBarId, TopBarItem, TopBarSpec};

/// A square icon/toggle button's design size.
pub(super) const BUTTON_DIP: f32 = 32.0;
/// A slider's default design width.
const SLIDER_DIP: f32 = 120.0;
/// The horizontal padding inside a label.
pub(super) const LABEL_PAD_DIP: f32 = 6.0;
/// A fixed spacer's design width.
const GAP_DIP: f32 = 6.0;
/// The left/right inset of the whole row.
pub(super) const SIDE_PAD_DIP: f32 = 8.0;
/// The icon glyph size, in device-independent pixels.
const ICON_SIZE: f32 = 16.0;
/// The label font size, in device-independent pixels.
const TEXT_SIZE: f32 = 12.0;

/// What a flattened item is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Icon,
    Toggle,
    Slider,
    Label,
    Spacer,
    Native,
}

/// One flattened item: its identity, resolved glyph/text layout and slider
/// state, and its intrinsic width in device-independent pixels.
pub(super) struct Item {
    pub(super) id: TopBarId,
    pub(super) kind: Kind,
    pub(super) enabled: bool,
    pub(super) tooltip: Option<String>,
    pub(super) checked: bool,
    /// A spacer that absorbs the row's leftover width.
    pub(super) fill: bool,
    /// An item that grows to take the row's leftover width (a slider that
    /// should fill the space, like a seek bar).
    pub(super) expand: bool,
    pub(super) glyph: Option<Layout>,
    pub(super) text: Option<Layout>,
    pub(super) slider: Option<SliderState>,
    pub(super) width_dip: f32,
    /// A native slot's own height, centred in the band.
    pub(super) height_dip: Option<f32>,
    /// The control a native slot keeps positioned.
    pub(super) child: Option<Child>,
}

/// The shared, `M`-free state behind a [`MaterialTopBar`](super::MaterialTopBar).
pub(crate) struct TopBarState {
    height_dip: Cell<f32>,
    pub(super) items: RefCell<Vec<Item>>,
    /// Item rectangles in client coordinates (device pixels), from the last
    /// [`relayout`](TopBarState::relayout).
    pub(super) rects: RefCell<Vec<Rect>>,
    pub(super) hover: Cell<Option<usize>>,
    pub(super) pressed: Cell<Option<usize>>,
    pub(super) focus: Cell<Option<usize>>,
}

/// The shared menu font, resolved once per UI thread. `None` when DirectWrite
/// is unavailable, in which case the material top bar cannot be built.
fn font() -> Option<Font> {
    thread_local! {
        static FONT: RefCell<Option<Font>> = const { RefCell::new(None) };
    }
    FONT.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            let system = TextSystem::new().ok()?;
            let spec = FontSpec::new("system-ui, Segoe UI, sans-serif", TEXT_SIZE);
            *slot = Some(system.font(&spec).ok()?);
        }
        slot.clone()
    })
}

/// The shared icon fonts, one per glyph size, resolved lazily on the UI thread.
///
/// An icon/toggle button normally uses [`ICON_SIZE`], but a custom button height
/// scales its glyph down (see [`TopBarItem::height`](crate::TopBarItem::height)),
/// so more than one size can appear. Resolving is not on a paint path (it
/// happens when the item list is set).
fn icon_font(size_dip: f32) -> Option<Font> {
    thread_local! {
        static FONTS: RefCell<HashMap<u32, Font>> = RefCell::new(HashMap::new());
    }
    // Key on half-dip steps, so a custom size does not create a font per item.
    let key = (size_dip * 2.0).round() as u32;
    FONTS.with(|cell| {
        let mut fonts = cell.borrow_mut();
        if let Some(font) = fonts.get(&key) {
            return Some(font.clone());
        }
        let system = TextSystem::new().ok()?;
        let spec = FontSpec::new(
            "Segoe Fluent Icons, Segoe MDL2 Assets, sans-serif",
            key as f32 / 2.0,
        );
        let font = system.font(&spec).ok()?;
        fonts.insert(key, font.clone());
        Some(font)
    })
}

/// The glyph size for a button of `height_dip`, or the default when unset.
///
/// A custom height scales the glyph proportionally, so a smaller button keeps
/// the glyph's margin. The glyph never grows past [`ICON_SIZE`], so a larger
/// button does not get an oversized icon.
fn icon_size(height_dip: Option<f32>) -> f32 {
    height_dip.map_or(ICON_SIZE, |height| ICON_SIZE.min(height * 0.625))
}

/// One glyph at `size_dip`, laid out on one line, or `None` when DirectWrite is
/// unavailable.
fn glyph_layout(glyph: char, size_dip: f32) -> Option<Layout> {
    icon_font(size_dip).and_then(|font| font.layout(&glyph.to_string(), f32::INFINITY).ok())
}

/// Flattens a public item into its resolved form.
fn resolve(item: TopBarItem) -> Item {
    let TopBarItem {
        id,
        spec,
        tooltip,
        checked,
        enabled,
        width,
        expand,
        height,
        child,
    } = item;
    let fill = matches!(&spec, TopBarSpec::Spacer { fill: true });
    let height_dip = height.map(|height| height.value());
    let glyph_size = icon_size(height_dip);
    let (kind, glyph, text, slider, default_width) = match spec {
        TopBarSpec::Icon(glyph) => (
            Kind::Icon,
            glyph_layout(glyph, glyph_size),
            None,
            None,
            BUTTON_DIP,
        ),
        TopBarSpec::Toggle(glyph) => (
            Kind::Toggle,
            glyph_layout(glyph, glyph_size),
            None,
            None,
            BUTTON_DIP,
        ),
        TopBarSpec::Slider { value, min, max } => {
            let mut state = SliderState::new(min, max);
            state.set_value_quiet(value);
            state.set_enabled(enabled);
            (Kind::Slider, None, None, Some(state), SLIDER_DIP)
        }
        TopBarSpec::Label(text) => {
            let layout = font().and_then(|font| font.layout(&text, f32::INFINITY).ok());
            let width = layout.as_ref().map_or(0.0, Layout::width) + LABEL_PAD_DIP * 2.0;
            (Kind::Label, None, layout, None, width)
        }
        TopBarSpec::Spacer { fill } => (
            Kind::Spacer,
            None,
            None,
            None,
            if fill { 0.0 } else { GAP_DIP },
        ),
        TopBarSpec::Native => (Kind::Native, None, None, None, 0.0),
    };
    let width_dip = width.map_or(default_width, |width| width.value());
    Item {
        id,
        kind,
        enabled,
        tooltip,
        checked,
        fill,
        expand,
        glyph,
        text,
        slider,
        width_dip,
        height_dip,
        child,
    }
}

impl TopBarState {
    /// Creates empty state, or `None` when DirectWrite (the fonts) is
    /// unavailable, in which case the caller falls back to a child row.
    pub(crate) fn new() -> Option<Rc<TopBarState>> {
        font()?;
        icon_font(ICON_SIZE)?;
        Some(Rc::new(TopBarState {
            height_dip: Cell::new(DEFAULT_HEIGHT_DIP),
            items: RefCell::new(Vec::new()),
            rects: RefCell::new(Vec::new()),
            hover: Cell::new(None),
            pressed: Cell::new(None),
            focus: Cell::new(None),
        }))
    }

    /// The band's design height in device-independent pixels.
    pub(crate) fn height_dip(&self) -> f32 {
        self.height_dip.get()
    }

    /// Records a new band design height.
    pub(crate) fn set_height_dip(&self, height_dip: f32) {
        self.height_dip.set(height_dip.max(1.0));
    }

    /// Replaces the item list, resolving glyphs, label text and slider state.
    pub(crate) fn set_items(&self, items: Vec<TopBarItem>) {
        *self.items.borrow_mut() = items.into_iter().map(resolve).collect();
        self.hover.set(None);
        self.pressed.set(None);
        self.focus.set(None);
    }

    /// Lays the items out across the band `top_px..top_px + height_px` of a
    /// client `width_px` wide at `dpi`, and stores the rectangles.
    pub(crate) fn relayout(&self, width_px: i32, top_px: i32, height_px: i32, dpi: u32) {
        let scale = dpi as f32 / 96.0;
        let items = self.items.borrow();
        let spans: Vec<Span> = items
            .iter()
            .map(|item| {
                if item.fill || item.expand {
                    Span::Fill
                } else {
                    Span::Fixed(item.width_dip * scale)
                }
            })
            .collect();
        let placed = layout::distribute(
            &spans,
            SIDE_PAD_DIP * scale,
            width_px as f32 - SIDE_PAD_DIP * scale,
        );
        let rects = items
            .iter()
            .zip(placed)
            .map(|(item, (left, width))| {
                let (top, bottom) = match item.kind {
                    Kind::Icon | Kind::Toggle => {
                        // A button's height defaults to its square design size;
                        // a custom height makes it compact (still capped to the
                        // band, with the same inset as the default).
                        let wanted = item.height_dip.unwrap_or(BUTTON_DIP);
                        let control = (wanted * scale).min((height_px as f32 - 4.0).max(0.0));
                        let t = top_px as f32 + (height_px as f32 - control) / 2.0;
                        (t, t + control)
                    }
                    Kind::Native => native::slot_span(top_px, height_px, scale, item.height_dip),
                    _ => (top_px as f32, top_px as f32 + height_px as f32),
                };
                Rect::new(
                    left.round() as i32,
                    top.round() as i32,
                    (left + width).round() as i32,
                    bottom.round() as i32,
                )
            })
            .collect::<Vec<Rect>>();
        native::place_children(&items, &rects);
        *self.rects.borrow_mut() = rects;
    }

    /// The index of the item under `point`, if any.
    pub(crate) fn hit(&self, point: crate::geometry::Point) -> Option<usize> {
        layout::hit(&self.rects.borrow(), point)
    }

    /// Whether an item currently has keyboard focus.
    pub(crate) fn has_focus(&self) -> bool {
        self.focus.get().is_some()
    }

    /// The rectangle of the item with `id`, if it is laid out.
    pub(crate) fn rect_of(&self, id: TopBarId) -> Option<Rect> {
        let items = self.items.borrow();
        let index = items.iter().position(|item| item.id == id)?;
        self.rects.borrow().get(index).copied()
    }

    /// Calls `f` with the centre point (client coordinates) of each native
    /// slot, so the caller can repaint a child control hosted there after the
    /// bar's frame was presented over it. Allocates nothing.
    pub(crate) fn for_each_native_point(&self, mut f: impl FnMut(crate::geometry::Point)) {
        let items = self.items.borrow();
        let rects = self.rects.borrow();
        for (index, item) in items.iter().enumerate() {
            if item.kind == Kind::Native
                && let Some(rect) = rects.get(index)
            {
                f(crate::geometry::Point::new(
                    (rect.left + rect.right) / 2,
                    (rect.top + rect.bottom) / 2,
                ));
            }
        }
    }

    /// One `(index, rect, text)` per laid-out item, with an empty text for
    /// items without a tooltip, so the caller can register or clear the shared
    /// tooltip slot for every item.
    pub(crate) fn tooltip_slots(&self) -> Vec<(usize, Rect, String)> {
        let items = self.items.borrow();
        let rects = self.rects.borrow();
        items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                let rect = *rects.get(index)?;
                Some((index, rect, item.tooltip.clone().unwrap_or_default()))
            })
            .collect()
    }

    /// Sets a slider's value (ignored while it is dragged), or a label's text.
    /// Returns whether anything changed.
    pub(crate) fn set_value(&self, id: TopBarId, value: f64) -> bool {
        let mut items = self.items.borrow_mut();
        let Some(item) = items.iter_mut().find(|item| item.id == id) else {
            return false;
        };
        match item.slider.as_mut() {
            Some(slider) => slider.set_value_quiet(value),
            None => false,
        }
    }

    /// Sets a toggle's checked state. Returns whether it changed.
    pub(crate) fn set_checked(&self, id: TopBarId, checked: bool) -> bool {
        let mut items = self.items.borrow_mut();
        let Some(item) = items
            .iter_mut()
            .find(|item| item.id == id && item.kind == Kind::Toggle)
        else {
            return false;
        };
        let changed = item.checked != checked;
        item.checked = checked;
        changed
    }

    /// Enables or disables an item. Returns whether it changed.
    pub(crate) fn set_enabled(&self, id: TopBarId, enabled: bool) -> bool {
        let mut items = self.items.borrow_mut();
        let Some(item) = items.iter_mut().find(|item| item.id == id) else {
            return false;
        };
        let changed = item.enabled != enabled;
        item.enabled = enabled;
        if let Some(slider) = item.slider.as_mut() {
            slider.set_enabled(enabled);
        }
        changed
    }

    /// Sets a label's text, re-laying it out. Returns whether it changed.
    pub(crate) fn set_text(&self, id: TopBarId, text: &str) -> bool {
        let mut items = self.items.borrow_mut();
        let Some(item) = items
            .iter_mut()
            .find(|item| item.id == id && item.kind == Kind::Label)
        else {
            return false;
        };
        if item
            .text
            .as_ref()
            .is_some_and(|layout| layout.text() == text)
        {
            return false;
        }
        item.text = font().and_then(|font| font.layout(text, f32::INFINITY).ok());
        item.width_dip = item.text.as_ref().map_or(0.0, Layout::width) + LABEL_PAD_DIP * 2.0;
        true
    }

    /// The text of a label item, for tests and callers.
    #[cfg(test)]
    pub(super) fn label_text(&self, id: TopBarId) -> Option<String> {
        let items = self.items.borrow();
        let item = items
            .iter()
            .find(|item| item.id == id && item.kind == Kind::Label)?;
        Some(item.text.as_ref()?.text().to_string())
    }

    /// The current value of a slider item, for tests and callers.
    #[cfg(test)]
    pub(super) fn slider_value(&self, id: TopBarId) -> Option<f64> {
        let items = self.items.borrow();
        let item = items.iter().find(|item| item.id == id)?;
        item.slider.as_ref().map(SliderState::current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Point;
    use crate::units::dip;
    use crate::{Fluent, TopBarEvent, TopBarId, TopBarItem};

    /// Builds a state, or `None` when DirectWrite is unavailable (skip).
    fn state() -> Option<Rc<TopBarState>> {
        TopBarState::new()
    }

    #[test]
    fn a_custom_size_makes_a_compact_icon_button() {
        let Some(state) = state() else {
            return;
        };
        state.set_items(vec![
            TopBarItem::icon_button(1u32, Fluent::PLAY)
                .width(dip(20.0))
                .height(dip(20.0)),
        ]);
        state.relayout(200, 0, 40, 96);
        let rect = state.rects.borrow()[0];
        assert_eq!(rect.width(), 20, "the width is the custom span");
        assert_eq!(rect.height(), 20, "the height is the custom pill height");
        assert_eq!(
            (rect.top + rect.bottom) / 2,
            20,
            "the compact button stays centred"
        );
    }

    #[test]
    fn a_custom_height_scales_the_glyph_down() {
        assert_eq!(icon_size(None), ICON_SIZE, "the default is unchanged");
        assert_eq!(
            icon_size(Some(20.0)),
            12.5,
            "a compact button gets a smaller glyph"
        );
        assert_eq!(
            icon_size(Some(48.0)),
            ICON_SIZE,
            "a larger button does not grow the glyph past the default"
        );
    }

    #[test]
    fn items_lay_out_left_to_right_with_a_fill_spacer() {
        let Some(state) = state() else {
            return;
        };
        state.set_items(vec![
            TopBarItem::icon_button(1u32, Fluent::PLAY),
            TopBarItem::flexible_spacer(),
            TopBarItem::slider(2u32, 0.5, 0.0..=1.0).width(dip(120.0)),
        ]);
        state.relayout(200, 40, 40, 96);
        let rects = state.rects.borrow();
        assert_eq!(rects.len(), 3);
        assert!(rects[0].left < 10, "the first item starts at the left");
        assert!(
            rects[2].right >= 190,
            "the fill spacer pushes the slider to the right edge, got {}",
            rects[2].right
        );
        let point = Point::new(rects[0].left + 1, rects[0].top + 1);
        assert_eq!(state.hit(point), Some(0));
        assert_eq!(
            state.hit(Point::new(rects[2].left + 1, rects[2].top + 1)),
            Some(2)
        );
    }

    #[test]
    fn a_slider_drag_maps_position_to_value_and_commits() {
        let Some(state) = state() else {
            return;
        };
        state.set_items(vec![
            TopBarItem::slider(7u32, 0.0, 0.0..=100.0).width(dip(120.0)),
        ]);
        state.relayout(200, 0, 40, 96);
        let rect = state.rects.borrow()[0];

        // A press at the right edge jumps the thumb near the maximum.
        state.pointer_down(Point::new(rect.right - 1, rect.top + 1), 96);
        let high = state.slider_value(TopBarId::new(7)).expect("slider value");
        assert!(
            high > 90.0,
            "right edge should map near the max, got {high}"
        );

        // A drag to the left edge tracks down near the minimum.
        state.pointer_move(Point::new(rect.left + 1, rect.top + 1), 96);
        let low = state.slider_value(TopBarId::new(7)).expect("slider value");
        assert!(low < 10.0, "left edge should map near the min, got {low}");

        let event = state.pointer_up(Point::new(rect.left + 1, rect.top + 1), 96);
        assert!(
            matches!(event, Some(TopBarEvent::SliderCommit { id, .. }) if id == TopBarId::new(7)),
            "the release commits, got {event:?}"
        );
        assert!(!state.slider_dragging());
    }

    #[test]
    fn a_toggle_flips_and_reports_its_new_state() {
        let Some(state) = state() else {
            return;
        };
        state.set_items(vec![TopBarItem::toggle(3u32, Fluent::REPEAT)]);
        state.relayout(200, 0, 40, 96);
        let rect = state.rects.borrow()[0];
        let point = Point::new(rect.left + 1, rect.top + 1);

        assert_eq!(state.pointer_down(point, 96), None);
        let event = state.pointer_up(point, 96);
        assert_eq!(
            event,
            Some(TopBarEvent::Toggle {
                id: TopBarId::new(3),
                checked: true
            })
        );
    }

    #[test]
    fn an_expanding_item_takes_the_leftover_width() {
        let Some(state) = state() else {
            return;
        };
        state.set_items(vec![
            TopBarItem::icon_button(1u32, Fluent::PLAY),
            TopBarItem::slider(2u32, 0.0, 0.0..=1.0).expand(true),
            TopBarItem::label(3u32, "0:00"),
        ]);
        state.relayout(400, 0, 40, 96);
        let rects = state.rects.borrow();
        assert_eq!(rects.len(), 3);
        assert!(
            rects[1].width() > rects[0].width() * 3,
            "the expanding slider should take the row's leftover width, got {}",
            rects[1].width()
        );
        assert!(
            rects[2].right <= 400,
            "the label after the expanding item stays inside the row"
        );
    }

    #[test]
    fn set_text_relays_a_label() {
        let Some(state) = state() else {
            return;
        };
        state.set_items(vec![TopBarItem::label(4u32, "0:00")]);
        assert_eq!(state.label_text(TopBarId::new(4)), Some("0:00".to_string()));
        assert!(state.set_text(TopBarId::new(4), "1:23"));
        assert_eq!(state.label_text(TopBarId::new(4)), Some("1:23".to_string()));
        assert!(
            !state.set_text(TopBarId::new(4), "1:23"),
            "an unchanged text is a no-op"
        );
    }
}
