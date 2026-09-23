#![forbid(unsafe_code)]

//! The per-window widget-layer core: the message queue, drain scheduling, and
//! the shared state both [`Ui`](super::Ui) and the window handler need.
//!
//! The app itself deliberately lives *outside* this type, in the handler's
//! `RefCell`: `Ui` must stay usable while [`App::update`](super::App::update)
//! runs, and `update` holds the app borrow, so putting the app here would make
//! the drain's `try_borrow_mut` deadlock against itself.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;

use crate::accel::Shortcut;
use crate::geometry::Rect;
use crate::hwnd::Hwnd;
use crate::message::TimerId;
use crate::sys;
use crate::theme::Theme;

use super::layout::{Layout, Placed};

/// Maps a close request to an optional app message.
type CloseMapper<M> = Box<dyn Fn() -> Option<M>>;
/// Maps a timer tick to an optional app message.
type TimerMapper<M> = Box<dyn Fn(TimerId) -> Option<M>>;
/// Maps an accelerator activation to an optional app message.
type AccelMapper<M> = Box<dyn Fn() -> Option<M>>;

/// A registered shortcut and the message it raises. The registration order is
/// the command id assigned to the shortcut in the window's accelerator table.
struct Accelerator<M> {
    shortcut: Shortcut,
    mapper: AccelMapper<M>,
}

/// The shared, interior-mutable state behind a widget-layer window.
pub(crate) struct Core<M> {
    hwnd: Cell<Hwnd>,
    queue: RefCell<VecDeque<M>>,
    drain: u32,
    on_close: RefCell<Option<CloseMapper<M>>>,
    on_timer: RefCell<Option<TimerMapper<M>>>,
    accelerators: RefCell<Vec<Accelerator<M>>>,
    theme: Cell<Theme>,
    layout: RefCell<Option<Layout>>,
}

impl<M> Core<M> {
    pub(crate) fn new(theme: Theme) -> Core<M> {
        Core {
            hwnd: Cell::new(Hwnd::NULL),
            queue: RefCell::new(VecDeque::new()),
            drain: sys::message::drain_message(),
            on_close: RefCell::new(None),
            on_timer: RefCell::new(None),
            accelerators: RefCell::new(Vec::new()),
            theme: Cell::new(theme),
            layout: RefCell::new(None),
        }
    }

    pub(crate) fn set_hwnd(&self, hwnd: Hwnd) {
        self.hwnd.set(hwnd);
    }

    pub(crate) fn hwnd(&self) -> Hwnd {
        self.hwnd.get()
    }

    /// Whether `code` is this window's private drain message.
    pub(crate) fn is_drain(&self, code: u32) -> bool {
        self.drain != 0 && code == self.drain
    }

    /// Appends `msg` and, if the queue was empty, posts the private drain
    /// message that will deliver it. Posting only on the empty→non-empty edge
    /// means a burst of messages costs a single drain.
    pub(crate) fn enqueue(&self, msg: M) {
        let was_empty = self.queue.borrow().is_empty();
        self.queue.borrow_mut().push_back(msg);
        if was_empty {
            self.post_drain();
        }
    }

    /// Pops the oldest queued message, if any.
    pub(crate) fn next(&self) -> Option<M> {
        self.queue.borrow_mut().pop_front()
    }

    /// Puts a message back at the front (used when the app is busy).
    pub(crate) fn put_back(&self, msg: M) {
        self.queue.borrow_mut().push_front(msg);
    }

    /// Posts the private drain message to the window.
    pub(crate) fn post_drain(&self) {
        if self.drain != 0 {
            let _ = sys::window::post_message(self.hwnd.get(), self.drain, 0, 0);
        }
    }

    pub(crate) fn set_on_close(&self, f: impl Fn() -> Option<M> + 'static) {
        self.on_close.replace(Some(Box::new(f)));
    }

    pub(crate) fn set_on_timer(&self, f: impl Fn(TimerId) -> Option<M> + 'static) {
        self.on_timer.replace(Some(Box::new(f)));
    }

    /// Maps a close request: `Some(msg)` intercepts it (the app decides),
    /// `None` means "use the default" (close and quit).
    pub(crate) fn map_close(&self) -> Option<M> {
        self.on_close.borrow().as_ref().and_then(|f| f())
    }

    /// Maps a timer tick: `Some(msg)` is enqueued by the caller.
    pub(crate) fn map_timer(&self, id: TimerId) -> Option<M> {
        self.on_timer.borrow().as_ref().and_then(|f| f(id))
    }

    /// Registers `shortcut` to raise the message its mapper returns, and
    /// rebuilds the window's accelerator table so the shortcut fires whichever
    /// widget has focus.
    pub(crate) fn add_accelerator(&self, shortcut: Shortcut, f: impl Fn() -> Option<M> + 'static) {
        self.accelerators.borrow_mut().push(Accelerator {
            shortcut,
            mapper: Box::new(f),
        });
        self.rebuild_accelerators();
    }

    /// Maps an accelerator command id to a message, if it belongs to one of
    /// this window's registered shortcuts.
    pub(crate) fn map_accelerator(&self, id: u16) -> Option<M> {
        let index = sys::looper::accelerator_index(id)?;
        let accelerators = self.accelerators.borrow();
        accelerators.get(index).and_then(|accel| (accel.mapper)())
    }

    /// Rebuilds the accelerator table from the current registrations.
    fn rebuild_accelerators(&self) {
        let hwnd = self.hwnd.get();
        if hwnd.is_null() {
            return;
        }
        let shortcuts: Vec<Shortcut> = self
            .accelerators
            .borrow()
            .iter()
            .map(|accelerator| accelerator.shortcut)
            .collect();
        let _ = sys::looper::set_accelerators(hwnd, &shortcuts);
    }

    /// The window's current theme.
    pub(crate) fn theme(&self) -> Theme {
        self.theme.get()
    }

    /// Records a new theme for the window.
    pub(crate) fn set_theme_value(&self, theme: Theme) {
        self.theme.set(theme);
    }

    /// Installs the layout tree and lays it out immediately.
    pub(crate) fn set_layout(&self, layout: Layout) {
        *self.layout.borrow_mut() = Some(layout);
        self.relayout();
    }

    /// Whether a layout tree has been installed.
    pub(crate) fn has_layout(&self) -> bool {
        self.layout.borrow().is_some()
    }

    /// Lays the tree out again at the window's current DPI.
    pub(crate) fn relayout(&self) {
        let hwnd = self.hwnd.get();
        if hwnd.is_null() {
            return;
        }
        self.relayout_at(sys::dpi::window_dpi(hwnd));
    }

    /// Lays the tree out again at `dpi` (used on `WM_DPICHANGED`, where the
    /// message carries the new value).
    pub(crate) fn relayout_with_dpi(&self, dpi: u32) {
        self.relayout_at(dpi);
    }

    fn relayout_at(&self, dpi: u32) {
        let hwnd = self.hwnd.get();
        if hwnd.is_null() {
            return;
        }
        let Some(layout) = self.layout.borrow().clone() else {
            return;
        };

        let client = sys::window::client_rect(hwnd);
        let placed: Vec<Placed> = layout
            .compute(client, dpi)
            .into_iter()
            .filter(|placed| placed.handle.hwnd().is_alive())
            .collect();
        let moves: Vec<(Hwnd, Rect)> = placed
            .iter()
            .map(|placed| (placed.handle.hwnd(), placed.rect))
            .collect();

        // Update the widgets' cached bounds before the batched OS move, so the
        // layout and the widgets agree even if a child handles `WM_SIZE`.
        for placed in &placed {
            placed.handle.set_bounds(placed.rect);
        }
        sys::layout::apply(&moves);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::Key;

    #[test]
    fn accelerators_map_their_reserved_command_ids() {
        // No window is set, so `add_accelerator` skips the table build but
        // still records the mapper.
        let core: Core<u32> = Core::new(Theme::light());
        core.add_accelerator(Shortcut::ctrl(Key::N), || Some(7));
        core.add_accelerator(Shortcut::ctrl(Key::Q), || None);

        assert_eq!(core.map_accelerator(sys::looper::command_id(0)), Some(7));
        assert_eq!(core.map_accelerator(sys::looper::command_id(1)), None);
        assert_eq!(core.map_accelerator(sys::looper::command_id(2)), None);
        assert_eq!(
            core.map_accelerator(0),
            None,
            "a control id is not a shortcut"
        );
    }
}
