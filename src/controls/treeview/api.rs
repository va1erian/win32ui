#![forbid(unsafe_code)]

//! The [`TreeView`] runtime API: refresh, model replacement, selection and
//! expansion by key.

use crate::controls::treeview::TreeView;
use std::hash::Hash;

use crate::controls::treeview::model::TreeModel;
use crate::sys;

impl<K: Clone + Eq + Hash + 'static, M: 'static> TreeView<K, M> {
    /// Diffs the loaded tree against the model and applies only the changes:
    /// nodes with the same key keep their native item, so expansion, selection
    /// and scroll position survive; new keys are inserted, gone keys removed.
    ///
    /// Only branches that are already loaded are re-read from the model, so an
    /// unopened branch costs nothing. Call it after mutating the model in
    /// place (an unread count changing, a folder appearing) to update the view.
    pub fn refresh(&self) {
        let hwnd = self.control.hwnd();
        self.inner.borrow_mut().refresh(hwnd);
    }

    /// Replaces the model and rebuilds from scratch (`refresh` on an empty
    /// tree). Expansion and selection are dropped, as none of the old keys are
    /// assumed to still exist.
    pub fn set_model(&self, model: impl TreeModel<Key = K> + 'static) {
        let hwnd = self.control.hwnd();
        self.inner.borrow_mut().set_model(hwnd, Box::new(model));
    }

    /// Selects `key`, scrolling it into view. Emits a single selection message
    /// when the selection actually moved; the per-item notifications the
    /// control sends while this runs are muted, so calling it from an
    /// `on_select` handler cannot loop.
    pub fn select(&self, key: &K) {
        let hwnd = self.control.hwnd();
        // The borrow is released before any native call: selecting sends a
        // notification the mapper resolves through the same state.
        let Some(handle) = self.inner.borrow().handles.get(key).copied() else {
            return;
        };
        self.inner.borrow_mut().selection_muted = true;
        sys::treeview::tv_select(hwnd, handle);
        sys::treeview::tv_ensure_visible(hwnd, handle);
        self.inner.borrow_mut().selection_muted = false;

        let token = sys::treeview::tv_selected(hwnd).map(|(_, token)| token);
        let changed = {
            let mut inner = self.inner.borrow_mut();
            let changed = token != inner.last_selection;
            inner.last_selection = token;
            changed
        };
        if changed && let Some(msg) = self.events.borrow().on_select.as_ref().and_then(|f| f(key)) {
            self.sink.emit(msg);
        }
    }

    /// Expands or collapses `key`. Unlike a user gesture this does not emit
    /// `on_toggle`, so calling it from that handler cannot loop. The branch's
    /// children load lazily on expand either way.
    pub fn expand(&self, key: &K, expand: bool) {
        let hwnd = self.control.hwnd();
        let Some(handle) = self.inner.borrow().handles.get(key).copied() else {
            return;
        };
        // Released before `tv_expand`, whose synchronous `TVN_ITEMEXPANDING`
        // the control's notification hook must be free to borrow for.
        self.inner.borrow_mut().toggle_muted = true;
        sys::treeview::tv_expand(hwnd, handle, expand);
        self.inner.borrow_mut().toggle_muted = false;
    }

    /// Whether `key` is currently expanded.
    pub fn is_expanded(&self, key: &K) -> bool {
        let hwnd = self.control.hwnd();
        self.inner.borrow().is_expanded_key(hwnd, key)
    }

    /// The currently selected key, if any.
    pub fn selected(&self) -> Option<K> {
        let hwnd = self.control.hwnd();
        self.inner.borrow().selected_key(hwnd)
    }

    /// The total number of materialized items (roots plus every expanded
    /// branch).
    pub fn node_count(&self) -> i32 {
        sys::treeview::tv_count(self.control.hwnd())
    }

    /// Scrolls `key` into view.
    pub fn ensure_visible(&self, key: &K) {
        let hwnd = self.control.hwnd();
        if let Some(handle) = self.inner.borrow().handles.get(key).copied() {
            sys::treeview::tv_ensure_visible(hwnd, handle);
        }
    }
}
