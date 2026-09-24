//! Accelerator registration and menu-bar bookkeeping for [`Core`].

use super::{Accelerator, Core};
use crate::accel::Shortcut;
use crate::controls::menu::Menu;
use crate::sys;

impl<M> Core<M> {
    /// Registers `shortcut` to raise the message its mapper returns, and
    /// rebuilds the window's accelerator table so the shortcut fires whichever
    /// widget has focus.
    pub(crate) fn add_accelerator(&self, shortcut: Shortcut, f: impl Fn() -> Option<M> + 'static) {
        self.accelerators.borrow_mut().push(Accelerator {
            shortcut,
            mapper: Box::new(f),
            from_menu: false,
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
}

impl<M: 'static> Core<M> {
    /// Replaces the accelerators registered by the previous menu bar with
    /// `menu`'s shortcuts, keeping those added through [`Self::add_accelerator`].
    pub(crate) fn set_menu_accelerators(&self, menu: &Menu<M>) {
        {
            let mut accelerators = self.accelerators.borrow_mut();
            accelerators.retain(|accelerator| !accelerator.from_menu);
            for (shortcut, action) in menu.shortcuts() {
                accelerators.push(Accelerator {
                    shortcut,
                    mapper: Box::new(move || Some(action())),
                    from_menu: true,
                });
            }
        }
        self.rebuild_accelerators();
    }

    /// Whether the installed menu bar has an item named `key`, after setting
    /// its tick. Redraws the strip menu when one is shown.
    pub(crate) fn set_menu_checked(&self, key: &str, checked: bool) -> bool {
        let found = self
            .menu_bar()
            .is_some_and(|menu| menu.set_checked(key, checked));
        if found {
            sys::window::invalidate(self.hwnd());
        }
        found
    }
}
