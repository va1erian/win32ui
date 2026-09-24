#![forbid(unsafe_code)]

//! Pointer and keyboard input for the material top bar, mapped to
//! [`TopBarEvent`]s. The top-level window handler routes the messages here and
//! enqueues the mapped messages.
//!
//! The band lies below the extended strip, so `WM_NCHITTEST` already reports the
//! client area there and a slider drag never starts a window drag. While a
//! slider is dragged the window holds the mouse capture, so the pointer may
//! leave the band and the value still tracks.

use crate::geometry::{Point, Rect};
use crate::message::Key;

use super::TopBarEvent;
use super::state::{Kind, TopBarState};

impl TopBarState {
    /// The pointer moved to `point`: continues a drag, or updates the hovered
    /// item and its slider hover. Returns a slider-change event while dragging.
    pub(crate) fn pointer_move(&self, point: Point, dpi: u32) -> Option<TopBarEvent> {
        if let Some(index) = self.dragging_index() {
            let rect = self.rects.borrow().get(index).copied()?;
            return self.drag_slider(index, point, rect, dpi);
        }
        let hit = self.hit(point);
        self.set_hover_index(hit);
        None
    }

    /// The pointer was pressed at `point`: focuses and presses the item, and
    /// starts a slider drag. Returns the first slider-change event, if any.
    pub(crate) fn pointer_down(&self, point: Point, dpi: u32) -> Option<TopBarEvent> {
        let Some(index) = self.hit(point) else {
            self.set_focus(None);
            return None;
        };
        if !self.items.borrow()[index].enabled {
            return None;
        }
        self.set_focus(Some(index));
        self.set_hover_index(Some(index));
        if self.items.borrow()[index].kind != Kind::Slider {
            self.pressed.set(Some(index));
            return None;
        }
        let rect = self.rects.borrow()[index];
        {
            let mut items = self.items.borrow_mut();
            let slider = items[index].slider.as_mut()?;
            slider.begin_drag(length_dip(rect, dpi), local_dip(point, rect, dpi));
        }
        self.drain_slider(index, false)
    }

    /// The pointer was released at `point`: commits a slider drag, or clicks the
    /// item pressed there. Returns the mapped event.
    pub(crate) fn pointer_up(&self, point: Point, dpi: u32) -> Option<TopBarEvent> {
        if let Some(index) = self.dragging_index() {
            let value = {
                let mut items = self.items.borrow_mut();
                items[index].slider.as_mut()?.end_drag()?
            };
            let id = self.items.borrow()[index].id;
            let _ = dpi;
            return Some(TopBarEvent::SliderCommit { id, value });
        }
        let pressed = self.pressed.take()?;
        let index = self.hit(point)?;
        if index != pressed {
            return None;
        }
        let (id, kind, enabled, checked) = {
            let items = self.items.borrow();
            let item = &items[index];
            (item.id, item.kind, item.enabled, item.checked)
        };
        if !enabled {
            return None;
        }
        match kind {
            Kind::Icon => Some(TopBarEvent::Click(id)),
            Kind::Toggle => {
                let checked = !checked;
                self.set_checked(id, checked);
                Some(TopBarEvent::Toggle { id, checked })
            }
            _ => None,
        }
    }

    /// The pointer left the band: clears the hover and press, unless a slider
    /// drag is in flight (the window holds the capture, so it keeps tracking).
    pub(crate) fn pointer_leave(&self) {
        if self.dragging_index().is_none() {
            self.pressed.set(None);
            self.set_hover_index(None);
        }
    }

    /// Handles a key for the keyboard-focused item.
    pub(crate) fn key_down(&self, key: Key, dpi: u32) -> Option<TopBarEvent> {
        let index = self.focus.get()?;
        let (id, kind, enabled, checked) = {
            let items = self.items.borrow();
            let item = items.get(index)?;
            (item.id, item.kind, item.enabled, item.checked)
        };
        if !enabled {
            return None;
        }
        match key {
            Key::ESCAPE => {
                self.set_focus(None);
                None
            }
            Key::LEFT | Key::RIGHT if kind == Kind::Slider => {
                let sign = if key == Key::LEFT { -1.0 } else { 1.0 };
                let _ = dpi;
                let changed = {
                    let mut items = self.items.borrow_mut();
                    let slider = items[index].slider.as_mut()?;
                    slider.step_small(sign)
                };
                if changed {
                    self.drain_slider(index, true)
                } else {
                    None
                }
            }
            Key::LEFT => {
                self.move_focus(-1);
                None
            }
            Key::RIGHT => {
                self.move_focus(1);
                None
            }
            Key::SPACE | Key::RETURN => match kind {
                Kind::Icon => Some(TopBarEvent::Click(id)),
                Kind::Toggle => {
                    let checked = !checked;
                    self.set_checked(id, checked);
                    Some(TopBarEvent::Toggle { id, checked })
                }
                _ => None,
            },
            _ => None,
        }
    }

    /// Whether a slider drag is in flight, so the window keeps the capture.
    pub(crate) fn slider_dragging(&self) -> bool {
        self.dragging_index().is_some()
    }

    /// The index of the slider currently dragged, if any.
    fn dragging_index(&self) -> Option<usize> {
        self.items
            .borrow()
            .iter()
            .position(|item| item.slider.as_ref().is_some_and(|slider| slider.dragging()))
    }

    /// Continues the drag of the `index`-th slider at `point`.
    fn drag_slider(&self, index: usize, point: Point, rect: Rect, dpi: u32) -> Option<TopBarEvent> {
        {
            let mut items = self.items.borrow_mut();
            let slider = items[index].slider.as_mut()?;
            slider.drag_to(length_dip(rect, dpi), local_dip(point, rect, dpi));
        }
        self.drain_slider(index, false)
    }

    /// Takes the queued change of the `index`-th slider and maps it to a commit
    /// (`commit = true`) or a live change event.
    fn drain_slider(&self, index: usize, commit: bool) -> Option<TopBarEvent> {
        let mut items = self.items.borrow_mut();
        let id = items[index].id;
        let value = items[index].slider.as_mut()?.take_change()?;
        Some(if commit {
            TopBarEvent::SliderCommit { id, value }
        } else {
            TopBarEvent::SliderChange { id, value }
        })
    }

    /// Records the keyboard focus and settles the sliders' focus rings.
    fn set_focus(&self, focus: Option<usize>) {
        self.focus.set(focus);
        let mut items = self.items.borrow_mut();
        for (index, item) in items.iter_mut().enumerate() {
            if let Some(slider) = item.slider.as_mut() {
                slider.set_focused(Some(index) == focus);
            }
        }
    }

    /// Records the hovered item and settles the sliders' hover visuals.
    fn set_hover_index(&self, hit: Option<usize>) {
        let previous = self.hover.get();
        if previous == hit {
            return;
        }
        self.hover.set(hit);
        let mut items = self.items.borrow_mut();
        if let Some(index) = previous
            && let Some(slider) = items.get_mut(index).and_then(|item| item.slider.as_mut())
        {
            slider.set_hover(false);
        }
        if let Some(index) = hit
            && let Some(slider) = items.get_mut(index).and_then(|item| item.slider.as_mut())
        {
            slider.set_hover(true);
        }
    }

    /// Moves the keyboard focus `step` items over the enabled, non-spacer ones.
    fn move_focus(&self, step: i32) {
        let items = self.items.borrow();
        let len = items.len();
        if len == 0 {
            return;
        }
        let current = self.focus.get().unwrap_or(0) as i32;
        let mut index = current;
        for _ in 0..len {
            index = (index + step).rem_euclid(len as i32);
            let item = &items[index as usize];
            if item.enabled && item.kind != Kind::Spacer {
                self.focus.set(Some(index as usize));
                return;
            }
        }
    }
}

/// The main-axis length of `rect` in device-independent pixels.
fn length_dip(rect: Rect, dpi: u32) -> f64 {
    f64::from(rect.width()) * 96.0 / f64::from(dpi.max(1))
}

/// Where `point` falls along `rect`'s main axis, in device-independent pixels
/// from the rect's left edge.
fn local_dip(point: Point, rect: Rect, dpi: u32) -> f64 {
    f64::from(point.x - rect.left) * 96.0 / f64::from(dpi.max(1))
}
