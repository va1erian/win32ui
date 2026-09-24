#![forbid(unsafe_code)]

//! Runtime driving of a built [`Tabs`] node: change the selected page or hide
//! the strip without rebuilding the window. The node is normally driven by the
//! native control; these methods page it from code.

use crate::sys;

use super::Tabs;

impl Tabs {
    /// Selects the page at `index` at runtime: the strip repages (showing only
    /// that page) and `on_change` is raised as if the user had clicked the tab.
    ///
    /// Selecting the page that is already current does not raise `on_change`,
    /// so an app may call this from its own `on_change` handler without
    /// looping.
    pub fn set_selected(&self, index: usize) {
        let count = self.pages.len();
        if count == 0 {
            return;
        }
        let index = index.min(count - 1);
        let changed = index != self.shared.selected.get();
        self.shared.selected.set(index);
        let hwnd = self.shared.hwnd.get();
        if hwnd.is_alive() {
            sys::tabs::set_cur_sel(hwnd, index);
        }
        if changed && let Some(relayout) = self.shared.relayout.borrow().clone() {
            relayout(true);
        }
    }

    /// The currently selected page index.
    pub fn selected(&self) -> usize {
        self.shared.selected.get()
    }

    /// Shows or hides the strip without dropping the node. A hidden strip takes
    /// no space and every page is hidden; showing it repages the selection.
    pub fn set_visible(&self, visible: bool) {
        self.shared.visible.set(visible);
        if let Some(bound) = self.shared.bound.borrow().as_ref() {
            self.shared.handle(bound).set_visible(visible);
        }
        for (_, page) in &self.pages {
            page.set_tree_visible(visible);
        }
        if let Some(relayout) = self.shared.relayout.borrow().clone() {
            relayout(false);
        }
    }

    /// Whether the strip is shown.
    pub fn is_visible(&self) -> bool {
        self.shared.visible.get()
    }
}
