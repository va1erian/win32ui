#![forbid(unsafe_code)]

//! The tab strip's accessibility source: a tab list whose children are the
//! tabs, the current one selected. Selecting a tab repages exactly as a click
//! does, so the app's `on_change` mapping fires.

use std::rc::Weak;

use super::TabsShared;
use crate::accessibility::registry::Source;
use crate::accessibility::{Action, Node, Role};
use crate::sys;

pub(super) struct TabsAccess {
    pub(super) shared: Weak<TabsShared>,
}

impl Source for TabsAccess {
    fn snapshot(&self) -> Option<Node> {
        let shared = self.shared.upgrade()?;
        let hwnd = shared.hwnd.get();
        let selected = sys::tabs::cur_sel(hwnd);
        let tabs = shared
            .titles
            .try_borrow()
            .ok()?
            .iter()
            .enumerate()
            .map(|(index, title)| {
                let tab = Node::new(Role::TabItem, title.clone()).selected(selected == Some(index));
                match sys::tabs::tab_rect(hwnd, index) {
                    Some(rect) => tab.bounds(rect),
                    None => tab,
                }
            })
            .collect::<Vec<_>>();
        Some(Node::new(Role::Tab, "").children(tabs))
    }

    fn perform(&self, path: &[usize], action: Action) -> bool {
        let ([index], Action::Select | Action::Invoke) = (path, action) else {
            return false;
        };
        let Some(shared) = self.shared.upgrade() else {
            return false;
        };
        if *index >= shared.count.get() {
            return false;
        }
        sys::tabs::set_cur_sel(shared.hwnd.get(), *index);
        let relayout = shared.relayout.borrow().clone();
        match relayout {
            Some(relayout) => {
                relayout(true);
                true
            }
            None => false,
        }
    }
}
