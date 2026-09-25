#![forbid(unsafe_code)]

//! A menu described to accessibility: every item a node, submenus nesting,
//! separators skipped. Choosing a leaf raises the item's message directly (no
//! popup is opened, so a client is never blocked in a menu's modal loop).

use crate::accessibility::{Node, Role};

use super::Menu;
use super::data::{Action, Entry};

/// A label without its `&` accelerator markers (`&&` is a literal `&`).
fn plain(label: &str) -> String {
    let mut out = String::with_capacity(label.len());
    let mut chars = label.chars();
    while let Some(c) = chars.next() {
        if c == '&' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else {
            out.push(c);
        }
    }
    out
}

impl<M: 'static> Menu<M> {
    /// The menu's items as accessibility nodes, in order, separators skipped.
    pub(crate) fn access_nodes(&self) -> Vec<Node> {
        self.data
            .entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Separator { .. } => None,
                Entry::Item(item) => {
                    let mut node = Node::new(Role::MenuItem, plain(&item.label))
                        .enabled(item.enabled)
                        .invokable();
                    if item.checked.get() || item.key.is_some() || item.radio {
                        node = node.checked(item.checked.get());
                    }
                    if let Some(shortcut) = item.shortcut {
                        node = node.help(shortcut.to_string());
                    }
                    Some(node)
                }
                Entry::Submenu(submenu) => Some(
                    Node::new(Role::MenuItem, plain(&submenu.label))
                        .children(submenu.menu.access_nodes()),
                ),
            })
            .collect()
    }

    /// The action of the leaf item at `path` (indices among the visible items
    /// at each level), if it exists and is enabled.
    pub(crate) fn access_action(&self, path: &[usize]) -> Option<Action<M>> {
        let (first, rest) = path.split_first()?;
        let entry = self
            .data
            .entries
            .iter()
            .filter(|entry| !matches!(entry, Entry::Separator { .. }))
            .nth(*first)?;
        match (entry, rest.is_empty()) {
            (Entry::Item(item), true) if item.enabled => Some(std::rc::Rc::clone(&item.action)),
            (Entry::Submenu(submenu), false) => submenu.menu.access_action(rest),
            _ => None,
        }
    }
}
