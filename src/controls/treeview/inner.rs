#![forbid(unsafe_code)]

//! The tree view's retained state and the notification plumbing behind
//! [`TreeView`](super::TreeView): lazy loading, keyed refresh, selection and
//! expansion by key.

use std::collections::HashMap;
use std::hash::Hash;

use windows::Win32::UI::Controls::{NM_CUSTOMDRAW, TVN_ITEMEXPANDING};

use crate::controls::registry::{ControlEvents, ControlKind};
use crate::controls::treeview::diff::Entry;
use crate::controls::treeview::image_list::ImageList;
use crate::controls::treeview::model::TreeModel;
use crate::controls::treeview::style::NodeStyle;
use crate::gdi::Font;
use crate::hwnd::Hwnd;
use crate::sys;
use crate::theme::Theme;

/// The app's per-node style closure.
pub(crate) type StyleFn<K> = Box<dyn Fn(&K) -> NodeStyle>;

/// Everything a [`TreeView`](super::TreeView) keeps alive.
pub(crate) struct TreeViewInner<K> {
    pub(crate) model: Option<Box<dyn TreeModel<Key = K>>>,
    /// The materialized tree, in native order.
    pub(crate) entries: Vec<Entry<K>>,
    /// Materialized keys to their native handles.
    pub(crate) handles: HashMap<K, isize>,
    /// Node tokens (native `lParam`) back to their keys.
    pub(crate) keys: HashMap<i64, K>,
    /// Node tokens to their cached style, so the paint path neither calls the
    /// style closure nor allocates a badge.
    pub(crate) styles: HashMap<i64, NodeStyle>,
    pub(crate) next_token: i64,
    pub(crate) theme: Theme,
    pub(crate) font: Font,
    /// The bold variant of `font`, created once so the custom-draw hot path
    /// allocates no GDI object.
    pub(crate) bold_font: Font,
    pub(crate) style: Option<StyleFn<K>>,
    pub(crate) image_list: Option<ImageList>,
    pub(crate) dpi: u32,
    /// The selection token last reported; unchanged notifications emit nothing.
    pub(crate) last_selection: Option<i64>,
    /// While set, a programmatic selection reports its own single event.
    pub(crate) selection_muted: bool,
    /// While set, a programmatic expand does not emit `on_toggle`.
    pub(crate) toggle_muted: bool,
}

impl<K: Clone + Eq + Hash + 'static> TreeViewInner<K> {
    /// The key behind a node token, if it is still materialized.
    pub(crate) fn key_of_token(&self, token: i64) -> Option<K> {
        self.keys.get(&token).cloned()
    }

    pub(crate) fn alloc_token(&mut self) -> i64 {
        self.next_token += 1;
        self.next_token
    }

    fn is_loaded(&self, key: &K) -> bool {
        find_entry(&self.entries, key).is_some_and(|entry| entry.children.is_some())
    }

    /// The currently selected key, if any.
    pub(crate) fn selected_key(&self, hwnd: Hwnd) -> Option<K> {
        sys::treeview::tv_selected(hwnd).and_then(|(_, token)| self.key_of_token(token))
    }

    /// Whether `key` is currently expanded.
    pub(crate) fn is_expanded_key(&self, hwnd: Hwnd, key: &K) -> bool {
        self.handles
            .get(key)
            .is_some_and(|handle| sys::treeview::tv_is_expanded(hwnd, *handle))
    }
}

impl<K: Clone + Eq + Hash + 'static> ControlEvents for TreeViewInner<K> {
    fn kind(&self) -> ControlKind {
        ControlKind::TreeView
    }

    fn on_notification(
        &mut self,
        hwnd: Hwnd,
        code: u32,
        _wparam: usize,
        lparam: isize,
    ) -> Option<isize> {
        if code == TVN_ITEMEXPANDING {
            if let Some((handle, token)) = sys::treeview::tv_expanding(lparam)
                && let Some(key) = self.key_of_token(token)
                && !self.is_loaded(&key)
            {
                self.load_children(hwnd, handle, &key);
            }
            // Returning 0 allows the expansion to proceed.
            return Some(0);
        }
        if code == NM_CUSTOMDRAW {
            return Some(sys::treeview::tv_custom_draw(lparam, |ctx| {
                self.custom_draw(hwnd, ctx)
            }));
        }
        None
    }
}

/// Finds a materialized entry by key at any depth.
fn find_entry<'a, K: PartialEq>(entries: &'a [Entry<K>], key: &K) -> Option<&'a Entry<K>> {
    for entry in entries {
        if &entry.key == key {
            return Some(entry);
        }
        if let Some(children) = &entry.children
            && let Some(found) = find_entry(children, key)
        {
            return Some(found);
        }
    }
    None
}
