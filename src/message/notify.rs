#![forbid(unsafe_code)]

//! Notifications routed from child controls (`WM_NOTIFY`) and timer ids.

use crate::controls::listview::ListViewEvent;
use crate::controls::treeview::TreeViewEvent;
use crate::hwnd::Hwnd;

/// A notification routed from a child control (`WM_NOTIFY`).
#[derive(Clone, Copy, Debug)]
pub enum Notify {
    /// A [`ListView`](crate::ListView) event.
    ListView {
        /// The list view's control id.
        id: usize,
        /// What happened.
        event: ListViewEvent,
    },
    /// A [`TreeView`](crate::TreeView) event.
    TreeView {
        /// The tree view's control id.
        id: usize,
        /// What happened.
        event: TreeViewEvent,
    },
    /// A notification we don't model, left for the caller to interpret.
    Other {
        /// The control's id.
        id: usize,
        /// The raw `NMHDR.code`.
        code: u32,
        /// The control that sent it.
        hwnd: Hwnd,
    },
}

/// A `SetTimer` identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TimerId(pub usize);
