//! Runtime tick changes for keyed menu items.

use crate::sys;

use super::data::{Entry, MenuData};

impl<M: 'static> MenuData<M> {
    /// Sets the checked state of the item keyed `key`, searching submenus, and
    /// updates the built native menu. Returns whether an item was found.
    pub(super) fn set_checked(&self, key: &str, checked: bool) -> bool {
        self.set_checked_in(self.handle.get(), key, checked)
    }

    fn set_checked_in(&self, root: isize, key: &str, checked: bool) -> bool {
        for entry in &self.entries {
            match entry {
                Entry::Item(item) if item.key == Some(key) => {
                    item.checked.set(checked);
                    if root != 0 {
                        sys::menu::check_item(root, item.id, checked, item.radio);
                    }
                    return true;
                }
                Entry::Submenu(submenu) if submenu.menu.data.set_checked_in(root, key, checked) => {
                    return true;
                }
                _ => {}
            }
        }
        false
    }
}
