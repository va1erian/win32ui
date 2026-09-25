#![forbid(unsafe_code)]

//! [`run_app`] and the window handler that ties the widget layer together.

use std::cell::RefCell;
use std::rc::Rc;

use crate::error::Result;
use crate::geometry::Point;
use crate::message::{Key, LResult, Message, MouseButton};
use crate::sys;
use crate::window::{Window, WindowHandler};

use super::child::build;
use super::core::Core;
use super::spec::{App, WindowSpec};
use super::ui::Ui;

/// Builds the top-level window from `spec`, constructs the app through `make`,
/// and runs the message loop until the window is closed or [`Ui::quit`] is
/// called.
pub fn run_app<A, F>(spec: WindowSpec, make: F) -> Result<()>
where
    A: App + 'static,
    F: FnOnce(&mut Ui<A::Msg>) -> A,
{
    crate::init();

    let theme = spec.parts().3;
    let built = build(spec, theme, None, false, make)?;
    built.window.show_painted();
    let _code = crate::looper::run();
    built.window.destroy();
    Ok(())
}

/// The window handler behind a widget-layer window.
///
/// It owns the two halves the drain needs: the [`Core`] (queue + drain message)
/// and the app itself, held in a `RefCell` so `update` can be detected as
/// "in progress" with [`try_borrow_mut`](RefCell::try_borrow_mut).
pub(crate) struct AppHandler<A: App> {
    core: Rc<Core<A::Msg>>,
    app: Rc<RefCell<Option<A>>>,
}

impl<A: App> AppHandler<A> {
    pub(crate) fn new(core: Rc<Core<A::Msg>>, app: Rc<RefCell<Option<A>>>) -> AppHandler<A> {
        AppHandler { core, app }
    }

    /// Mouse and keyboard input for the strip menu. Returns `None` when no
    /// strip menu is active or the message is not one the strip handles.
    fn title_menu_input(&self, window: &Window, message: &Message) -> Option<LResult> {
        if !self.core.has_title_menu() {
            return None;
        }
        let hwnd = window.hwnd();
        let item_hit = |x: i32, y: i32| self.core.title_menu_hit(Point::new(x, y));
        match message {
            Message::MouseMove { x, y, .. } => {
                let _ = window.track_mouse_leave();
                self.core.title_menu_set_hover(item_hit(*x, *y));
                sys::window::invalidate(hwnd);
                Some(0)
            }
            Message::MouseLeave => {
                self.core.title_menu_set_hover(None);
                self.core.title_menu_set_pressed(None);
                sys::window::invalidate(hwnd);
                Some(0)
            }
            Message::MouseDown {
                x,
                y,
                button: MouseButton::Left,
                ..
            } => {
                self.core.title_menu_set_pressed(item_hit(*x, *y));
                sys::window::invalidate(hwnd);
                Some(0)
            }
            Message::MouseUp {
                x,
                y,
                button: MouseButton::Left,
                ..
            } => {
                let hit = item_hit(*x, *y);
                self.core.title_menu_set_pressed(None);
                if let Some(index) = hit {
                    self.core.open_title_menu(index);
                }
                Some(0)
            }
            Message::KeyDown {
                key,
                modifiers,
                system,
                ..
            } => self
                .core
                .title_menu_key(*key, *modifiers, *system)
                .then_some(0),
            Message::KeyUp { key: Key::MENU, .. } => {
                self.core.title_menu_on_alt_up();
                Some(0)
            }
            _ => None,
        }
    }

    /// Mouse and keyboard input for the material top bar. Returns `None` when
    /// no top bar is active or the message is not one the bar handles.
    fn top_bar_input(&self, window: &Window, message: &Message) -> Option<LResult> {
        if !self.core.has_material_top_bar() {
            return None;
        }
        let hwnd = window.hwnd();
        match message {
            Message::MouseMove { x, y, .. } => {
                if self.core.top_bar_pointer_move(Point::new(*x, *y)) {
                    let _ = window.track_mouse_leave();
                    Some(0)
                } else {
                    None
                }
            }
            Message::MouseDown {
                x,
                y,
                button: MouseButton::Left,
                ..
            } => {
                if self.core.top_bar_pointer_down(Point::new(*x, *y)) {
                    // A slider drag keeps tracking outside the band through the
                    // capture, and must not start a window drag.
                    if self.core.top_bar_slider_dragging() {
                        sys::window_input::set_capture(hwnd);
                    }
                    Some(0)
                } else {
                    None
                }
            }
            Message::MouseUp {
                x,
                y,
                button: MouseButton::Left,
                ..
            } => {
                if self.core.top_bar_pointer_up(Point::new(*x, *y)) {
                    if !self.core.top_bar_slider_dragging() {
                        sys::window_input::release_capture();
                    }
                    Some(0)
                } else {
                    None
                }
            }
            Message::KeyDown { key, .. } => self.core.top_bar_key(*key).then_some(0),
            _ => None,
        }
    }

    /// Delivers queued messages, one `update` at a time, until the queue is
    /// empty or the app is busy.
    ///
    /// The `try_borrow_mut` is the whole re-entrancy guard: `update` holds the
    /// app borrow for its full duration, so a drain that runs while `update`
    /// is still on the stack (only possible inside a modal loop, which pumps
    /// posted messages) finds the app busy and leaves the queue for the drain
    /// that resumes after `update` returns.
    fn drain(&self) {
        loop {
            let Some(msg) = self.core.next() else {
                return;
            };
            let Ok(mut slot) = self.app.try_borrow_mut() else {
                self.core.put_back(msg);
                return;
            };
            let Some(app) = slot.as_mut() else {
                self.core.put_back(msg);
                return;
            };
            let mut ui = Ui::new(Rc::clone(&self.core));
            app.update(msg, &mut ui);
        }
    }
}

impl<A: App> WindowHandler for AppHandler<A> {
    fn message(&self, window: &Window, message: Message) -> Option<LResult> {
        // The strip menu (acrylic title bar) paints the window's transparent
        // Direct2D surface and reads its own mouse/keyboard input.
        if matches!(&message, Message::Paint) && self.core.try_paint_material() {
            return Some(0);
        }
        // A pointer leaving the window must clear both the strip menu's and the
        // top bar's hover, so it is handled once here rather than by whichever
        // of the two is routed first.
        if matches!(&message, Message::MouseLeave) {
            let mut handled = false;
            if self.core.has_title_menu() {
                self.core.title_menu_set_hover(None);
                self.core.title_menu_set_pressed(None);
                handled = true;
            }
            if self.core.has_material_top_bar() {
                self.core.top_bar_pointer_leave();
                handled = true;
            }
            if handled {
                return Some(0);
            }
        }
        if let Some(result) = self.top_bar_input(window, &message) {
            return Some(result);
        }
        if let Some(result) = self.title_menu_input(window, &message) {
            return Some(result);
        }
        // Owner-drawn menu items are measured and painted here, on the thread
        // that owns the menu.
        if let Message::MeasureItem { menu: true, .. } = &message
            && self.core.measure_menu_item(&message)
        {
            return Some(1);
        }
        if let Message::DrawItem { menu: true, .. } = &message
            && self.core.draw_menu_item(&message)
        {
            return Some(1);
        }
        match message {
            Message::Other { code, .. } if self.core.is_drain(code) => {
                self.drain();
                Some(0)
            }
            // Posted at the end of a DPI change: paint the whole tree once more,
            // after the common controls have finished re-laying themselves out,
            // so no text is left unpainted until the pointer hovers it.
            Message::Other { code, .. }
                if code != 0 && code == sys::message::dpi_settled_message() =>
            {
                sys::window::paint_now(window.hwnd());
                Some(0)
            }
            Message::Close => {
                match self.core.map_close() {
                    Some(msg) => self.core.enqueue(msg),
                    None => {
                        window.destroy();
                        if self.core.quits_loop() {
                            crate::looper::quit(0);
                        }
                    }
                }
                Some(0)
            }
            Message::Timer { id } => {
                if let Some(msg) = self.core.map_timer(id) {
                    self.core.enqueue(msg);
                }
                Some(0)
            }
            // The monitor layout changed (a display was added or removed): let
            // the app re-enumerate and react. A no-op without a mapping.
            Message::DisplayChange { .. } => {
                if let Some(msg) = self.core.map_display_change() {
                    self.core.enqueue(msg);
                }
                Some(0)
            }
            // An accelerator is translated into a `WM_COMMAND` with no control
            // and one of our reserved command ids; a menu bar click arrives the
            // same way with one of the menu's command ids.
            Message::Command(command) => {
                if command.control.is_none() {
                    if let Some(msg) = self.core.map_accelerator(command.id) {
                        self.core.enqueue(msg);
                        return Some(0);
                    }
                    if let Some(msg) = self.core.map_menu_command(command.id) {
                        self.core.enqueue(msg);
                        return Some(0);
                    }
                }
                None
            }
            // The window owns the layout: a resize re-runs the tree so the
            // application never has to handle `WM_SIZE`.
            Message::Size { width, height } if self.core.has_layout() => {
                // The caption buttons move with the window (and when it is
                // maximized), so re-read the inset before laying out; the
                // extended strip is re-applied so the frame survives a resize.
                self.core.resize_strip(width, height);
                sys::nc::apply_extended_frame(window.hwnd());
                // A maximize/de-maximize can move the window without a fresh
                // `WM_NCCALCSIZE`, leaving the client at the wrong size; force
                // one now that the window rectangle is final.
                if sys::nc::client_mismatch(window.hwnd()) {
                    sys::nc::reframe(window.hwnd());
                }
                sys::nc::refresh_caption_inset(window.hwnd());
                self.core.relayout();
                // The top bar's items are laid out against the new client
                // width, and its band against the (possibly re-read) strip.
                self.core.refresh_material_top_bar();
                // `relayout` marked the whole tree for repaint (which also
                // brings back a material surface the DWM frame dropped). During
                // a live resize the window manager presents a frame before a
                // deferred `WM_PAINT` runs, leaving ghosts of moved widgets in
                // the area they vacated, so paint now. Every borrow taken above
                // has been released, so the nested paint cannot conflict.
                sys::window::paint_now(window.hwnd());
                Some(0)
            }
            // Becoming active (e.g. restoring from minimized) can leave the
            // material surface and its children blank until the next input;
            // invalidate the whole window and every child for a repaint.
            Message::Activate { active: true, .. } if self.core.has_material_surface() => {
                sys::window::redraw_children(window.hwnd());
                None
            }
            Message::DpiChanged { dpi, suggested } => {
                if !suggested.is_empty() {
                    sys::window::move_window(window.hwnd(), suggested);
                }
                sys::control::refresh_ui_fonts(window.hwnd(), dpi);
                self.core.relayout_with_dpi(dpi);
                // The caption strip height and the caption buttons move with the
                // DPI, so both are re-read for the new scale.
                self.core.refresh_menu_strip();
                self.core.refresh_material_status_bar();
                self.core.refresh_material_top_bar();
                sys::nc::apply_extended_frame(window.hwnd());
                sys::nc::refresh_caption_inset(window.hwnd());
                // Common controls reopen their visual-style data for the new DPI
                // and can drop the dark sub-app name; re-apply the theme so no
                // native part (header, scroll bar, tab frame) falls back to
                // light, then paint the whole tree so nothing is left blank.
                crate::theme::retheme_children(window.hwnd(), &self.core.theme());
                sys::window::paint_now(window.hwnd());
                let _ = sys::window::post_message(
                    window.hwnd(),
                    sys::message::dpi_settled_message(),
                    0,
                    0,
                );
                Some(0)
            }
            _ => None,
        }
    }
}
