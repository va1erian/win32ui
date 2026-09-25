#![forbid(unsafe_code)]

//! The list view's accessibility source. A list can hold thousands of rows, so
//! the tree lists only the rows in view (like a virtualized list in any
//! toolkit); a client scrolls to reach the rest. Each row is a selectable,
//! invokable item named after its first cell, with one text child per cell
//! named after that cell and identified by its column title.

use std::cell::RefCell;
use std::rc::Rc;

use super::api::apply_selection;
use super::draw::ListViewInner;
use super::events::ListViewEvents;
use crate::accessibility::registry::Source;
use crate::accessibility::{Action, Node, Role};
use crate::app::Ui;
use crate::hwnd::Hwnd;
use crate::sys;

pub(super) struct ListAccess<T, M> {
    pub(super) hwnd: Hwnd,
    pub(super) inner: Rc<RefCell<ListViewInner<T>>>,
    pub(super) events: Rc<RefCell<ListViewEvents<M>>>,
    pub(super) sink: Ui<M>,
}

impl<T: 'static, M: 'static> ListAccess<T, M> {
    fn row_count(&self) -> Option<usize> {
        let inner = self.inner.try_borrow().ok()?;
        Some(inner.model.as_ref().map_or(0, |model| model.len()))
    }

    fn row(&self, row: usize, titles: &[String]) -> Node {
        let view = self.hwnd;
        let cells =
            titles.iter().enumerate().map(|(column, title)| {
                let text = sys::listview::lv_item_text(view, row as i32, column as i32);
                Node::new(Role::Text, text).id(title.clone()).bounds(
                    sys::listview::lv_subitem_rect(view, row as i32, column as i32),
                )
            });
        // The row reads as its non-empty cells in column order, so a screen
        // reader announces the whole record rather than only a marker column.
        let name = (0..titles.len())
            .map(|column| sys::listview::lv_item_text(view, row as i32, column as i32))
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join(", ");
        Node::new(Role::ListItem, name)
            .id(format!("row-{row}"))
            .selected(sys::listview::lv_is_selected(view, row as i32))
            .invokable()
            .bounds(sys::listview::lv_subitem_rect(view, row as i32, 0))
            .children(cells)
    }
}

impl<T: 'static, M: 'static> Source for ListAccess<T, M> {
    fn snapshot(&self) -> Option<Node> {
        let total = self.row_count()?;
        let titles: Vec<String> = self
            .inner
            .try_borrow()
            .ok()?
            .columns
            .iter()
            .map(|column| column.title.clone())
            .collect();
        let view = self.hwnd;
        let rows = sys::listview::lv_visible_rows(view, total)
            .map(|row| self.row(row, &titles))
            .collect::<Vec<_>>();
        Some(
            Node::new(Role::List, "")
                .value(format!("{total} rows"))
                .enabled(sys::window_input::is_enabled(view))
                .focusable(sys::window_input::has_focus(view))
                .children(rows),
        )
    }

    fn perform(&self, path: &[usize], action: Action) -> bool {
        let total = self.row_count().unwrap_or(0);
        let visible = sys::listview::lv_visible_rows(self.hwnd, total);
        let row = match path {
            [index] => visible.start + *index,
            [] if action == Action::Focus => {
                sys::window_input::focus(self.hwnd);
                return true;
            }
            _ => return false,
        };
        if row >= visible.end {
            return false;
        }
        match action {
            Action::Select => {
                apply_selection(self.hwnd, &self.inner, &self.events, &self.sink, &[row]);
                true
            }
            Action::Invoke => {
                if let Some(msg) = self
                    .events
                    .borrow()
                    .on_activate
                    .as_ref()
                    .and_then(|f| f(row))
                {
                    self.sink.emit(msg);
                }
                true
            }
            _ => false,
        }
    }
}
