#![forbid(unsafe_code)]

//! [`run_app`] and the window handler that ties the widget layer together.

use std::cell::RefCell;
use std::rc::Rc;

use crate::error::Result;
use crate::geometry::Rect;
use crate::message::{LResult, Message};
use crate::sys;
use crate::window::{Window, WindowClass, WindowExStyle, WindowHandler, WindowStyle};

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

    let (title, width, height, theme) = spec.parts();
    let core = Rc::new(Core::new(theme));
    let app: Rc<RefCell<Option<A>>> = Rc::new(RefCell::new(None));
    let handler = AppHandler {
        core: Rc::clone(&core),
        app: Rc::clone(&app),
    };

    let dpi = sys::dpi::system_dpi();
    let class = WindowClass::register("win32ui.app", theme.background)?;
    let window = Window::create(
        class,
        None,
        WindowStyle::overlapped().min_max(),
        WindowExStyle::new(),
        Rect::new(0, 0, width.to_px(dpi).value(), height.to_px(dpi).value()),
        title,
        handler,
    )?;
    core.set_hwnd(window.hwnd());
    window.set_theme(theme);

    // Construct the app once the window (and thus `Ui`) exists, then store it
    // where the handler can reach it. Messages raised while `make` runs are
    // queued and delivered as soon as the loop starts.
    let mut ui = Ui::new(Rc::clone(&core));
    let built = make(&mut ui);
    *app.borrow_mut() = Some(built);

    window.show();
    let _code = crate::looper::run();
    window.destroy();
    Ok(())
}

/// The window handler behind a widget-layer window.
///
/// It owns the two halves the drain needs: the [`Core`] (queue + drain message)
/// and the app itself, held in a `RefCell` so `update` can be detected as
/// "in progress" with [`try_borrow_mut`](RefCell::try_borrow_mut).
struct AppHandler<A: App> {
    core: Rc<Core<A::Msg>>,
    app: Rc<RefCell<Option<A>>>,
}

impl<A: App> AppHandler<A> {
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
                        crate::looper::quit(0);
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
            _ => None,
        }
    }
}
