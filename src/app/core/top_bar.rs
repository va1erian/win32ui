#![forbid(unsafe_code)]

//! The material top bar half of [`Core`](super::Core): install, layout, paint
//! and input for the interactive band on the top backdrop material.
//!
//! Like the bottom [`MaterialStatusBar`](crate::MaterialStatusBar), the bar is
//! not a child window — a child composites opaquely and would hide the
//! material. The state lives here (it is `M`-free); the app's event mapping is
//! stored alongside it, so the window handler can enqueue messages.

use std::rc::Rc;

use super::Core;
use crate::app::top_bar::{TopBarEvent, TopBarState};
use crate::d2d::{D2dCanvas, RectF, Rgba};
use crate::geometry::{Point, Rect};
use crate::message::Key;
use crate::sys;
use crate::theme::Theme;
use crate::units::dip;

/// The event mapper installed by [`MaterialTopBar::on_event`](crate::MaterialTopBar::on_event).
pub(crate) type TopBarMapper<M> = Box<dyn Fn(TopBarEvent) -> Option<M>>;

impl<M: 'static> Core<M> {
    /// Installs the material top bar: records it and reserves the band.
    pub(crate) fn set_material_top_bar(&self, state: Rc<TopBarState>) {
        *self.material_top_bar.borrow_mut() = Some(state);
        self.refresh_material_top_bar();
    }

    /// Whether a material top bar is installed.
    pub(crate) fn has_material_top_bar(&self) -> bool {
        self.material_top_bar.borrow().is_some()
    }

    /// The material top bar's height in device pixels (0 when not installed).
    pub(crate) fn material_top_bar_height_px(&self) -> i32 {
        crate::window::nc::top_bar(self.hwnd.get())
    }

    /// Installs the app's event mapper, replacing any previous one.
    pub(crate) fn set_material_top_bar_events(
        &self,
        f: impl Fn(TopBarEvent) -> Option<M> + 'static,
    ) {
        self.top_bar_events.replace(Some(Box::new(f)));
    }

    /// Maps a top bar event to a message, if a mapper is installed.
    fn map_top_bar_event(&self, event: TopBarEvent) -> Option<M> {
        self.top_bar_events
            .borrow()
            .as_ref()
            .and_then(|map| map(event))
    }

    /// Recomputes the band's height for the window's DPI, re-lays the items out
    /// for the current client size and repaints.
    pub(crate) fn refresh_material_top_bar(&self) {
        let hwnd = self.hwnd.get();
        if hwnd.is_null() {
            return;
        }
        let dpi = sys::dpi::window_dpi(hwnd);
        let client = sys::window::client_rect(hwnd);
        let strip = sys::nc::title_bar_height(hwnd);
        let state = self.material_top_bar.borrow().as_ref().cloned();
        match state {
            Some(state) => {
                let height = dip(state.height_dip()).to_px(dpi).value();
                crate::window::nc::set_top_bar(hwnd, height);
                state.relayout(client.right, strip, height, dpi);
                // Register each tooltipped item as a region on the window's
                // shared tooltip. Items without a tooltip register nothing (a
                // blank region would pop an empty tooltip); the layout keeps
                // each slot's rectangle current.
                for (slot, rect, text) in state.tooltip_slots() {
                    if !text.is_empty() {
                        crate::controls::tooltip::set_region_tooltip(hwnd, slot, rect, &text);
                    }
                }
            }
            None => crate::window::nc::set_top_bar(hwnd, 0),
        }
        sys::nc::apply_extended_frame(hwnd);
        sys::window::invalidate(hwnd);
    }

    /// Whether `point` (client coordinates) lies on the top bar band.
    fn over_top_bar(&self, point: Point) -> bool {
        let hwnd = self.hwnd.get();
        let band = crate::window::nc::top_bar(hwnd);
        if band <= 0 {
            return false;
        }
        let strip = sys::nc::title_bar_height(hwnd);
        (strip..strip + band).contains(&point.y)
    }

    /// The rectangle of the item with `id` in client coordinates, if laid out.
    pub(crate) fn top_bar_rect(&self, id: crate::app::top_bar::TopBarId) -> Option<Rect> {
        self.material_top_bar
            .borrow()
            .as_ref()
            .and_then(|state| state.rect_of(id))
    }

    /// Repaints a child control hosted in a top bar native slot, after the
    /// bar's frame was presented over it, making it paint opaquely over the
    /// material first. Allocates nothing once the child is set up.
    pub(crate) fn repaint_top_bar_native_children(&self) {
        let hwnd = self.hwnd.get();
        if let Some(state) = self.material_top_bar.borrow().as_ref() {
            state.for_each_native_point(|point| {
                sys::glass_child::repaint_opaque_child_at(hwnd, point)
            });
        }
    }

    /// Whether a top bar item currently has keyboard focus.
    pub(crate) fn top_bar_has_focus(&self) -> bool {
        self.material_top_bar
            .borrow()
            .as_ref()
            .is_some_and(|state| state.has_focus())
    }

    /// Whether a top bar slider is being dragged.
    pub(crate) fn top_bar_slider_dragging(&self) -> bool {
        self.material_top_bar
            .borrow()
            .as_ref()
            .is_some_and(|state| state.slider_dragging())
    }

    /// Routes a pointer move to the bar. Returns whether it was consumed (the
    /// pointer is over the band, or a slider drag is in flight).
    pub(crate) fn top_bar_pointer_move(&self, point: Point) -> bool {
        let Some(state) = self.material_top_bar.borrow().as_ref().cloned() else {
            return false;
        };
        if !state.slider_dragging() && !self.over_top_bar(point) {
            return false;
        }
        let event = state.pointer_move(point, sys::dpi::window_dpi(self.hwnd.get()));
        self.emit_top_bar(event);
        true
    }

    /// Routes a left-button press to the bar.
    pub(crate) fn top_bar_pointer_down(&self, point: Point) -> bool {
        let Some(state) = self.material_top_bar.borrow().as_ref().cloned() else {
            return false;
        };
        if !self.over_top_bar(point) {
            return false;
        }
        let event = state.pointer_down(point, sys::dpi::window_dpi(self.hwnd.get()));
        self.emit_top_bar(event);
        true
    }

    /// Routes a left-button release to the bar.
    pub(crate) fn top_bar_pointer_up(&self, point: Point) -> bool {
        let Some(state) = self.material_top_bar.borrow().as_ref().cloned() else {
            return false;
        };
        if !state.slider_dragging() && !self.over_top_bar(point) {
            return false;
        }
        let event = state.pointer_up(point, sys::dpi::window_dpi(self.hwnd.get()));
        self.emit_top_bar(event);
        true
    }

    /// Clears the bar's hover/press when the pointer leaves the window.
    pub(crate) fn top_bar_pointer_leave(&self) {
        if let Some(state) = self.material_top_bar.borrow().as_ref() {
            state.pointer_leave();
        }
        sys::window::invalidate(self.hwnd.get());
    }

    /// Routes a key press to the focused bar item. Returns whether it was
    /// consumed (only while an item is focused).
    pub(crate) fn top_bar_key(&self, key: Key) -> bool {
        if !self.top_bar_has_focus() {
            return false;
        }
        let Some(state) = self.material_top_bar.borrow().as_ref().cloned() else {
            return false;
        };
        let event = state.key_down(key, sys::dpi::window_dpi(self.hwnd.get()));
        self.emit_top_bar(event);
        true
    }

    /// Enqueues a bar event's message and repaints the band.
    pub(crate) fn emit_top_bar(&self, event: Option<TopBarEvent>) {
        if let Some(event) = event
            && let Some(msg) = self.map_top_bar_event(event)
        {
            self.enqueue(msg);
        }
        sys::window::invalidate(self.hwnd.get());
    }

    /// Paints the material top bar: the band background (opaque when the
    /// material is inactive), a bottom hairline, and the items. The surface was
    /// cleared transparent, so DWM composites the material through the band's
    /// untouched pixels.
    pub(crate) fn paint_material_top_bar(&self, canvas: &mut D2dCanvas, dpi: u32, theme: &Theme) {
        let Some(state) = self.material_top_bar.borrow().as_ref().cloned() else {
            return;
        };
        let hwnd = self.hwnd.get();
        let band = crate::window::nc::top_bar(hwnd);
        if band <= 0 {
            return;
        }
        let client = sys::window::client_rect(hwnd);
        let strip = sys::nc::title_bar_height(hwnd);
        let scale = dpi as f32 / 96.0;
        let (top_dip, bottom_dip) = (strip as f32 / scale, (strip + band) as f32 / scale);
        // The band is part of the extended frame, so its transparent pixels show
        // the material; a native child in a slot paints opaquely over it (see
        // `sys::glass_child`). Without an active material the band is opaque.
        if !crate::theme::backdrop_active(hwnd) {
            canvas.fill_rect_rgba(
                RectF::new(0.0, top_dip, client.right as f32 / scale, bottom_dip),
                Rgba::from(theme.surface),
            );
        }
        canvas.fill_rect_rgba(
            RectF::new(
                0.0,
                (strip + band - 1) as f32 / scale,
                client.right as f32 / scale,
                bottom_dip,
            ),
            Rgba::from(theme.border),
        );
        state.paint_items(canvas, dpi, theme);
    }
}
