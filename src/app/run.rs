#![forbid(unsafe_code)]

//! [`run_app`] and the window handler that ties the widget layer together.

use std::cell::RefCell;
use std::rc::Rc;

use crate::error::Result;
use crate::message::{LResult, Message};
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
    built.window.show();
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
            Message::Size { .. } if self.core.has_layout() => {
                // The caption buttons move with the window (and when it is
                // maximized), so re-read the inset before laying out; the
                // extended strip is re-applied so the frame survives a resize.
                sys::nc::apply_extended_frame(window.hwnd());
                sys::nc::refresh_caption_inset(window.hwnd());
                self.core.relayout();
                Some(0)
            }
            Message::DpiChanged { dpi, suggested } => {
                if !suggested.is_empty() {
                    sys::window::move_window(window.hwnd(), suggested);
                }
                self.core.relayout_with_dpi(dpi);
                // The caption strip height and the caption buttons move with the
                // DPI, so both are re-read for the new scale.
                sys::nc::apply_extended_frame(window.hwnd());
                sys::nc::refresh_caption_inset(window.hwnd());
                Some(0)
            }
            _ => None,
        }
    }
}
