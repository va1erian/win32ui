#![forbid(unsafe_code)]

//! A [`TreeView`] that populates itself lazily.
//!
//! Only root nodes are inserted up front. When a node is first expanded, the
//! `TVN_ITEMEXPANDING` hook asks the [`TreeSource`] for its children and
//! inserts them. That notification is consumed inside [`TreeViewInner`]; the
//! application instead sees [`TreeViewEvent`]s.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use windows::Win32::UI::Controls::TVN_ITEMEXPANDING;

use crate::controls::registry::{self, ControlEvents, ControlKind};
use crate::controls::{create_child, style};
use crate::error::Result;
use crate::geometry::Rect;
use crate::hwnd::Hwnd;
use crate::sys;
use crate::window::dpi_scale;

const TVS_HASBUTTONS: u32 = 0x0000_0001;
const TVS_HASLINES: u32 = 0x0000_0002;
const TVS_LINESATROOT: u32 = 0x0000_0004;
const TVS_SHOWSELALWAYS: u32 = 0x0000_0020;
const TVS_EX_DOUBLEBUFFER: u32 = 0x0000_0004;

const TVI_ROOT: isize = -65536;
const TVI_LAST: isize = -65534;

/// One node returned by a [`TreeSource`].
#[derive(Clone, Debug)]
pub struct TreeEntry {
    /// Display text.
    pub text: String,
    /// Opaque id passed back to [`TreeSource::children`] and reported in
    /// [`TreeViewEvent`]s.
    pub data: i64,
    /// Whether to show an expand button before the node's children load.
    pub has_children: bool,
}

impl TreeEntry {
    /// A leaf entry.
    pub fn leaf(text: impl Into<String>, data: i64) -> TreeEntry {
        TreeEntry {
            text: text.into(),
            data,
            has_children: false,
        }
    }

    /// An entry that can be expanded to load more children.
    pub fn branch(text: impl Into<String>, data: i64) -> TreeEntry {
        TreeEntry {
            text: text.into(),
            data,
            has_children: true,
        }
    }
}

/// Lazily supplies a [`TreeView`] with nodes.
pub trait TreeSource {
    /// The children of `parent`, or the roots when `parent` is `None`.
    fn children(&self, parent: Option<i64>) -> Vec<TreeEntry>;
}

/// A meaningful tree event, delivered to the parent window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeViewEvent {
    /// A node finished expanding.
    Expanded {
        /// The expanded node's id.
        item: i64,
    },
    /// The selected node changed.
    SelectionChanged {
        /// The new selection's id, if any.
        item: Option<i64>,
    },
    /// The selection was clicked.
    Click,
    /// The selection was double-clicked.
    DoubleClick,
    /// The selection was right-clicked.
    RightClick,
}

struct TreeViewInner {
    source: Box<dyn TreeSource>,
    populated: HashSet<isize>,
}

impl ControlEvents for TreeViewInner {
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
            if let Some((handle, data)) = sys::control::tv_expanding(lparam)
                && self.populated.insert(handle)
            {
                for entry in self.source.children(Some(data)) {
                    sys::control::tv_insert(
                        hwnd,
                        handle,
                        TVI_LAST,
                        &entry.text,
                        entry.data,
                        entry.has_children,
                    );
                }
            }
            // Returning 0 allows the expansion to proceed.
            return Some(0);
        }
        None
    }
}

/// A lazily-populated tree view.
pub struct TreeView {
    hwnd: Hwnd,
    inner: Rc<RefCell<TreeViewInner>>,
}

impl TreeView {
    /// Creates the control as a child of `parent`.
    pub fn new(
        parent: Hwnd,
        id: usize,
        bounds: Rect,
        source: Box<dyn TreeSource>,
        dpi: u32,
    ) -> Result<TreeView> {
        let style = style::WS_CHILD
            | style::WS_VISIBLE
            | style::WS_BORDER
            | style::WS_TABSTOP
            | TVS_HASBUTTONS
            | TVS_HASLINES
            | TVS_LINESATROOT
            | TVS_SHOWSELALWAYS;
        let hwnd = create_child(
            "TreeView",
            "SysTreeView32",
            parent,
            style,
            style::WS_EX_CLIENTEDGE,
            id,
            bounds,
        )?;

        sys::control::tv_set_extended_style(hwnd, TVS_EX_DOUBLEBUFFER);
        sys::control::tv_set_item_height(hwnd, dpi_scale(20, dpi));

        for entry in source.children(None) {
            sys::control::tv_insert(
                hwnd,
                TVI_ROOT,
                TVI_LAST,
                &entry.text,
                entry.data,
                entry.has_children,
            );
        }

        let inner = Rc::new(RefCell::new(TreeViewInner {
            source,
            populated: HashSet::new(),
        }));
        let events: Rc<RefCell<dyn ControlEvents>> = inner.clone();
        registry::register(hwnd, events);

        Ok(TreeView { hwnd, inner })
    }

    /// The control handle.
    pub fn hwnd(&self) -> Hwnd {
        self.hwnd
    }

    /// Moves/resizes the control.
    pub fn set_bounds(&self, bounds: Rect) {
        sys::window::move_window(self.hwnd, bounds);
    }

    /// Sets the tree's background and text colours.
    pub fn set_colors(&self, background: crate::Color, text: crate::Color) {
        sys::control::tv_set_colors(self.hwnd, background, text);
    }

    /// Replaces the data source. Existing nodes are kept; expansion state for
    /// not-yet-loaded branches is reset.
    pub fn set_source(&self, source: Box<dyn TreeSource>) {
        let mut inner = self.inner.borrow_mut();
        inner.source = source;
        inner.populated.clear();
    }

    /// The id of the selected node, if any.
    pub fn selected(&self) -> Option<i64> {
        sys::control::tv_selected(self.hwnd)
    }

    /// The total number of materialised nodes.
    pub fn node_count(&self) -> i32 {
        sys::control::tv_count(self.hwnd)
    }
}

impl Drop for TreeView {
    fn drop(&mut self) {
        registry::unregister(self.hwnd);
        sys::window::destroy(self.hwnd);
    }
}
