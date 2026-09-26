#![forbid(unsafe_code)]

//! The split divider: an owner-drawn child window that drags the boundary.
//!
//! It is a [`CustomWidget`], so it reuses the crate's single owner-draw and
//! input path: a resize cursor on hover, mouse capture while dragging, arrow
//! keys when focused. It mutates only [`SplitShared::position`]; the handler
//! then relayouts and maps the move to the app's `Msg`.

use std::cell::Cell;
use std::rc::{Rc, Weak};

use crate::app::Ui;
use crate::app::core::Core;
use crate::controls::custom::{CustomWidget, Input, WidgetCx};
use crate::gdi::{Canvas, Paint};
use crate::geometry::{Rect, Size};
use crate::message::{Key, LResult, Message, MouseButton};
use crate::theme::Theme;
use crate::units::{Dip, Px};
use crate::window::{CursorShape, Window, WindowHandler};

use super::{ARROW_STEP_DIP, DIVIDER_DIP, SplitShared};

/// The divider's own event, before it is mapped to the app's `Msg`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum SplitEvent {
    /// The divider moved to this position, in design units.
    Moved(Dip),
}

/// The owner-drawn divider widget.
pub(super) struct Divider {
    shared: Rc<SplitShared>,
    hovered: Cell<bool>,
    dragging: Cell<bool>,
    grab: Cell<i32>,
}

impl Divider {
    pub(super) fn new(shared: Rc<SplitShared>) -> Divider {
        Divider {
            shared,
            hovered: Cell::new(false),
            dragging: Cell::new(false),
            grab: Cell::new(0),
        }
    }

    fn coordinate(&self, x: i32, y: i32) -> i32 {
        if self.shared.is_horizontal() { x } else { y }
    }

    fn cursor(&self) -> CursorShape {
        if self.shared.is_horizontal() {
            CursorShape::SizeHorizontal
        } else {
            CursorShape::SizeVertical
        }
    }

    /// Clamps `px` (the anchored pane's extent) to the panes' minimums and
    /// emits the move.
    fn emit_position(&self, cx: &WidgetCx<SplitEvent>, px: i32) {
        let dpi = self.shared.dpi.get().max(96);
        let position = self.shared.clamp_anchored(px);
        cx.emit(SplitEvent::Moved(Px(position).to_dip(dpi)));
    }
}

impl CustomWidget for Divider {
    type Event = SplitEvent;

    fn paint(&self, canvas: &Canvas, bounds: Rect, theme: &Theme) {
        canvas.fill_rect(bounds, theme.background);
        let active = self.dragging.get() || self.hovered.get();
        let color = if active { theme.accent } else { theme.border };
        let half = if active { 2 } else { 1 };
        if self.shared.is_horizontal() {
            let x = bounds.width() / 2;
            canvas.fill_rect(
                Rect::new(x - half, bounds.top, x + half, bounds.bottom),
                color,
            );
        } else {
            let y = bounds.height() / 2;
            canvas.fill_rect(
                Rect::new(bounds.left, y - half, bounds.right, y + half),
                color,
            );
        }
    }

    fn input(&self, input: Input, cx: &mut WidgetCx<SplitEvent>) {
        match input {
            Input::MouseMove { x, y, .. } => {
                if !self.hovered.get() {
                    self.hovered.set(true);
                    cx.invalidate();
                }
                cx.cursor(self.cursor());
                if self.dragging.get() {
                    let drag =
                        self.shared.anchored_sign() * (self.coordinate(x, y) - self.grab.get());
                    self.emit_position(cx, self.shared.position_px() + drag);
                }
            }
            Input::MouseDown {
                x,
                y,
                button: MouseButton::Left,
                ..
            } => {
                self.dragging.set(true);
                self.grab.set(self.coordinate(x, y));
                cx.capture();
                cx.focus();
                cx.invalidate();
            }
            Input::MouseUp {
                button: MouseButton::Left,
                ..
            } if self.dragging.get() => {
                self.dragging.set(false);
                cx.release_capture();
                cx.invalidate();
            }
            Input::MouseLeave => {
                if self.hovered.get() {
                    self.hovered.set(false);
                    cx.invalidate();
                }
            }
            Input::KeyDown { key, .. } => {
                let delta = match (self.shared.is_horizontal(), key) {
                    (true, Key::LEFT) | (false, Key::UP) => -1,
                    (true, Key::RIGHT) | (false, Key::DOWN) => 1,
                    _ => 0,
                };
                if delta != 0 {
                    let dpi = self.shared.dpi.get().max(96);
                    let step = self.shared.anchored_sign()
                        * Dip(ARROW_STEP_DIP).to_px(dpi).value()
                        * delta;
                    self.emit_position(cx, self.shared.position_px() + step);
                }
            }
            Input::CaptureChanged if self.dragging.get() => {
                self.dragging.set(false);
                cx.invalidate();
            }
            Input::SetFocus | Input::KillFocus => cx.invalidate(),
            _ => {}
        }
    }

    fn preferred_size(&self, dpi: u32) -> Option<Size> {
        let thickness = Dip(DIVIDER_DIP).to_px(dpi).value();
        Some(Size::new(thickness, thickness))
    }
}

/// The handler behind a divider's child window.
pub(super) struct DividerHandler<M> {
    pub(super) core: Weak<Core<M>>,
    pub(super) bounds: Rc<Cell<Rect>>,
    pub(super) emit: Rc<dyn Fn(SplitEvent)>,
    pub(super) widget: Divider,
}

impl<M: 'static> WindowHandler for DividerHandler<M> {
    fn message(&self, window: &Window, message: Message) -> Option<LResult> {
        match message {
            Message::Paint => {
                if let Some(paint) = Paint::begin(window.hwnd()) {
                    let theme = self
                        .core
                        .upgrade()
                        .map(|core| Ui::new(core).theme())
                        .unwrap_or_else(Theme::light);
                    self.widget.paint(paint.canvas(), self.bounds.get(), &theme);
                }
                Some(0)
            }
            Message::Size { width, height } => {
                self.bounds.set(Rect::new(0, 0, width, height));
                Some(0)
            }
            message => {
                if matches!(message, Message::MouseMove { .. }) {
                    let _ = window.track_mouse_leave();
                }
                let input = Input::from_message(message)?;
                let mut cx = WidgetCx::new(
                    window.hwnd(),
                    Rc::clone(&self.bounds),
                    Rc::clone(&self.emit),
                    crate::sys::dpi::window_dpi(window.hwnd()),
                    Rc::default(),
                );
                self.widget.input(input, &mut cx);
                Some(0)
            }
        }
    }
}
