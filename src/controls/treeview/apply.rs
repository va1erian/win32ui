#![forbid(unsafe_code)]

//! Mutation behind the tree: lazy loading, the keyed refresh, and the native
//! edits it produces. Split from [`inner`](super::inner), which owns the state
//! and the notification hook.

use std::hash::Hash;

use windows::Win32::UI::Controls::I_IMAGENONE;

use crate::controls::treeview::diff::{Desired, Edit, Entry, desired_tree, reconcile};
use crate::controls::treeview::inner::TreeViewInner;
use crate::controls::treeview::model::TreeModel;
use crate::controls::treeview::style::NodeStyle;
use crate::controls::treeview::{TVI_FIRST, TVI_ROOT};
use crate::hwnd::Hwnd;
use crate::sys;

impl<K: Clone + Eq + Hash + 'static> TreeViewInner<K> {
    /// The native image index for a cached style, or `I_IMAGENONE`.
    fn image_of(&self, style: &NodeStyle) -> i32 {
        match style.icon {
            Some(index) if self.image_list.is_some() => index as i32,
            _ => I_IMAGENONE,
        }
    }

    /// Re-reads the style closure for every materialized node and caches it,
    /// updating each node's native icon. Called after the style or image list
    /// changes and after a refresh (the style may depend on model state).
    pub(crate) fn rebuild_styles(&mut self, hwnd: Hwnd) {
        self.styles.clear();
        let entries = std::mem::take(&mut self.entries);
        self.rebuild_styles_level(hwnd, &entries);
        self.entries = entries;
    }

    fn rebuild_styles_level(&mut self, hwnd: Hwnd, entries: &[Entry<K>]) {
        for entry in entries {
            let style = self
                .style
                .as_ref()
                .map(|style| style(&entry.key))
                .unwrap_or_default();
            if let Some(handle) = self.handles.get(&entry.key).copied() {
                let image = self.image_of(&style);
                sys::treeview::tv_set_item(hwnd, handle, None, None, Some(image));
            }
            self.styles.insert(entry.token, style);
            if let Some(children) = &entry.children {
                self.rebuild_styles_level(hwnd, children);
            }
        }
    }

    /// Loads `key`'s children from the model into the native tree (the lazy
    /// `TVN_ITEMEXPANDING` hook). A branch that is already loaded is left
    /// alone, so expanding twice never duplicates its children.
    pub(crate) fn load_children(&mut self, hwnd: Hwnd, handle: isize, key: &K) {
        let nodes = match &self.model {
            Some(model) => model.children(Some(key)),
            None => Vec::new(),
        };
        let mut after = TVI_FIRST;
        let mut child_entries = Vec::with_capacity(nodes.len());
        for node in nodes {
            let entry = self.insert_node(
                hwnd,
                handle,
                after,
                Desired {
                    key: node.key,
                    text: node.text,
                    has_children: node.has_children,
                    children: None,
                },
            );
            after = self.handles.get(&entry.key).copied().unwrap_or(TVI_FIRST);
            child_entries.push(entry);
        }
        let empty = child_entries.is_empty();
        if let Some(entry) = find_entry_mut(&mut self.entries, key) {
            entry.children = Some(child_entries);
            if empty && entry.has_children {
                entry.has_children = false;
                sys::treeview::tv_set_item(hwnd, handle, None, Some(false), None);
            }
        }
    }

    /// Inserts one node (and any already-loaded descendants) and records its
    /// handle and token.
    fn insert_node(
        &mut self,
        hwnd: Hwnd,
        parent: isize,
        after: isize,
        desired: Desired<K>,
    ) -> Entry<K> {
        let token = self.alloc_token();
        let style = self
            .style
            .as_ref()
            .map(|style| style(&desired.key))
            .unwrap_or_default();
        let image = self.image_of(&style);
        let handle = sys::treeview::tv_insert(
            hwnd,
            parent,
            after,
            &desired.text,
            token,
            desired.has_children,
            image,
        );
        self.handles.insert(desired.key.clone(), handle);
        self.keys.insert(token, desired.key.clone());
        self.styles.insert(token, style);
        let mut entry = Entry {
            key: desired.key,
            text: desired.text,
            has_children: desired.has_children,
            children: None,
            token,
        };
        if let Some(children) = desired.children {
            let mut after = TVI_FIRST;
            let mut child_entries = Vec::with_capacity(children.len());
            for child in children {
                let child_entry = self.insert_node(hwnd, handle, after, child);
                after = self
                    .handles
                    .get(&child_entry.key)
                    .copied()
                    .unwrap_or(TVI_FIRST);
                child_entries.push(child_entry);
            }
            entry.children = Some(child_entries);
        }
        entry
    }

    fn remove_node(&mut self, hwnd: Hwnd, entry: &Entry<K>) {
        if let Some(handle) = self.handles.get(&entry.key).copied() {
            sys::treeview::tv_delete(hwnd, handle);
        }
        self.forget_entry(entry);
    }

    fn forget_entry(&mut self, entry: &Entry<K>) {
        self.handles.remove(&entry.key);
        self.keys.remove(&entry.token);
        self.styles.remove(&entry.token);
        if let Some(children) = &entry.children {
            for child in children {
                self.forget_entry(child);
            }
        }
    }

    /// Applies one level's edits to the native tree, rebuilding that level's
    /// materialized entries. Kept items keep their handles, so expansion and
    /// selection survive a refresh.
    fn apply_level(
        &mut self,
        hwnd: Hwnd,
        parent: isize,
        existing: Vec<Entry<K>>,
        edits: Vec<Edit<K>>,
    ) -> Vec<Entry<K>> {
        let mut remaining = existing;
        let mut next = Vec::with_capacity(edits.len());
        let mut previous = TVI_FIRST;

        for edit in edits {
            match edit {
                Edit::Keep {
                    key,
                    text,
                    has_children,
                    children,
                } => {
                    let Some(index) = remaining.iter().position(|entry| entry.key == key) else {
                        continue;
                    };
                    let mut entry = remaining.remove(index);
                    let handle = self.handles.get(&key).copied().unwrap_or(0);
                    if handle == 0 {
                        // Lost track of the handle: re-insert rather than drop.
                        let rebuilt = self.insert_node(
                            hwnd,
                            parent,
                            previous,
                            Desired {
                                key,
                                text,
                                has_children,
                                children: None,
                            },
                        );
                        previous = self.handles.get(&rebuilt.key).copied().unwrap_or(TVI_FIRST);
                        next.push(rebuilt);
                        continue;
                    }
                    if entry.text != text || entry.has_children != has_children {
                        sys::treeview::tv_set_item(
                            hwnd,
                            handle,
                            Some(&text),
                            Some(has_children),
                            None,
                        );
                    }
                    entry.text = text;
                    entry.has_children = has_children;
                    if let Some(child_entries) = entry.children.take() {
                        let child_entries = self.apply_level(hwnd, handle, child_entries, children);
                        entry.children = Some(child_entries);
                    }
                    previous = handle;
                    next.push(entry);
                }
                Edit::Insert(desired) => {
                    let entry = self.insert_node(hwnd, parent, previous, desired);
                    previous = self.handles.get(&entry.key).copied().unwrap_or(TVI_FIRST);
                    next.push(entry);
                }
                Edit::Remove { key } => {
                    if let Some(index) = remaining.iter().position(|entry| entry.key == key) {
                        let entry = remaining.remove(index);
                        self.remove_node(hwnd, &entry);
                    }
                }
            }
        }

        // Anything left was not wanted; it should not normally happen because
        // `reconcile` removes every unmatched key, but never leak a native item.
        for entry in remaining {
            self.remove_node(hwnd, &entry);
        }
        next
    }

    /// Diffs the loaded tree against the model, preserving expansion and
    /// selection by key. Pure diff, impure only in reading the model and
    /// writing the native control.
    pub(crate) fn refresh(&mut self, hwnd: Hwnd) {
        let desired = match &self.model {
            Some(model) => desired_tree(model.as_ref(), None, &self.entries),
            None => Vec::new(),
        };
        let edits = reconcile(&self.entries, &desired);
        let entries = std::mem::take(&mut self.entries);
        self.entries = self.apply_level(hwnd, TVI_ROOT, entries, edits);
        // A style may depend on model state (an unread count), so re-read it.
        self.rebuild_styles(hwnd);
        // Deleting the selected node changes the selection silently.
        self.last_selection = sys::treeview::tv_selected(hwnd).map(|(_, token)| token);
        // Inserted/updated items need a paint; that paint runs with no borrow
        // held, so the custom-draw hook recolours every row normally.
        sys::window::invalidate(hwnd);
    }

    /// Replaces the model and rebuilds from scratch.
    pub(crate) fn set_model(&mut self, hwnd: Hwnd, model: Box<dyn TreeModel<Key = K>>) {
        sys::treeview::tv_delete(hwnd, TVI_ROOT);
        self.entries.clear();
        self.handles.clear();
        self.keys.clear();
        self.model = Some(model);
        self.refresh(hwnd);
    }
}

/// Finds a materialized entry by key at any depth, mutably.
fn find_entry_mut<'a, K: PartialEq>(
    entries: &'a mut [Entry<K>],
    key: &K,
) -> Option<&'a mut Entry<K>> {
    entries.iter_mut().find_map(|entry| {
        if &entry.key == key {
            Some(entry)
        } else {
            entry
                .children
                .as_mut()
                .and_then(|children| find_entry_mut(children, key))
        }
    })
}
