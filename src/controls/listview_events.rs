#![forbid(unsafe_code)]

//! The [`ListViewEvent`] enum and the widget-layer mapping to the app's `Msg`.

use crate::message::{Key, Modifiers};

/// An event from the list view, delivered to the parent window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListViewEvent {
    /// The selection changed.
    ItemChanged {
        /// The row that changed.
        item: i32,
        /// Whether it is now selected.
        selected: bool,
    },
    /// A row was clicked.
    Click {
        /// The row clicked (`-1` for empty space).
        item: i32,
    },
    /// A row was double-clicked.
    DoubleClick {
        /// The row double-clicked.
        item: i32,
    },
    /// A row was right-clicked.
    RightClick {
        /// The row right-clicked.
        item: i32,
    },
    /// Enter was pressed.
    ReturnKey {
        /// The focused row.
        item: i32,
    },
    /// A column header was clicked.
    ColumnClick {
        /// The column clicked.
        column: i32,
    },
    /// A key was pressed while the list had focus.
    KeyDown {
        /// The virtual-key code.
        key: u16,
        /// The modifier keys held when the key was pressed.
        modifiers: Modifiers,
    },
}

/// Maps a focused key press, with its modifiers, to an optional app message.
pub(crate) type KeyMapper<M> = Box<dyn Fn(Key, Modifiers) -> Option<M>>;

/// The app-level events a [`ListView`](super::ListView) maps to `Msg`.
pub(crate) struct ListViewEvents<M> {
    pub(crate) on_select: Option<Box<dyn Fn(usize) -> Option<M>>>,
    pub(crate) on_activate: Option<Box<dyn Fn(usize) -> Option<M>>>,
    pub(crate) on_context: Option<Box<dyn Fn(usize) -> Option<M>>>,
    pub(crate) on_key: Option<KeyMapper<M>>,
}

impl<M> ListViewEvents<M> {
    pub(crate) fn new() -> ListViewEvents<M> {
        ListViewEvents {
            on_select: None,
            on_activate: None,
            on_context: None,
            on_key: None,
        }
    }
}
