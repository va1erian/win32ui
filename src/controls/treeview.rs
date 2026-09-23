#![forbid(unsafe_code)]

//! A [`TreeView`] that populates itself lazily.
//!
//! Only root nodes are inserted up front. When a node is first expanded, the
//! `TVN_ITEMEXPANDING` hook asks the [`TreeSource`] for its children and
//! inserts them. That notification is consumed inside [`TreeViewInner`]; the
//! application instead sees [`TreeViewEvent`]s mapped to its own `Msg`.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use windows::Win32::UI::Controls::TVN_ITEMEXPANDING;

use crate::app::Ui;
use crate::controls::control::{AsControl, Control};
use crate::controls::registry::{self, ControlEvents, ControlKind};
use crate::controls::{create_child, next_id, style};
use crate::error::Result;
use crate::geometry::Rect;
use crate::hwnd::Hwnd;
use crate::message::{Message, Notify};
use crate::sys;
use crate::units::dip;

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

/// A mapping from a tree event's payload to an optional app message.
type TreeMapper<M> = Box<dyn Fn(Option<i64>) -> Option<M>>;

/// The app-level events a [`TreeView`] maps to `Msg`.
struct TreeViewEvents<M> {
    on_select: Option<TreeMapper<M>>,
    on_activate: Option<TreeMapper<M>>,
    on_context: Option<TreeMapper<M>>,
}

/// A lazily-populated tree view.
pub struct TreeView<M> {
    control: Control,
    inner: Rc<RefCell<TreeViewInner>>,
    events: Rc<RefCell<TreeViewEvents<M>>>,
}

impl<M: 'static> TreeView<M> {
    /// Creates the control as a child of the window behind `ui`.
    pub fn new(ui: &mut Ui<M>, bounds: Rect, source: Box<dyn TreeSource>) -> Result<TreeView<M>> {
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
            ui.hwnd(),
            style,
            style::WS_EX_CLIENTEDGE,
            next_id(),
            bounds,
        )?;

        sys::control::tv_set_extended_style(hwnd, TVS_EX_DOUBLEBUFFER);
        sys::control::tv_set_item_height(hwnd, dip(20.0).to_px(ui.dpi()).value());

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
        let control_events: Rc<RefCell<dyn ControlEvents>> = inner.clone();
        registry::register(hwnd, control_events);

        let events = Rc::new(RefCell::new(TreeViewEvents {
            on_select: None,
            on_activate: None,
            on_context: None,
        }));
        let sink = ui.clone();
        let events_for_mapper = events.clone();
        let mapper: Rc<dyn Fn(&Message) -> bool> = Rc::new(move |message| {
            let Message::Notify(Notify::TreeView { event, .. }) = message else {
                return false;
            };
            let msg = {
                let events = events_for_mapper.borrow();
                match *event {
                    TreeViewEvent::SelectionChanged { item } => {
                        events.on_select.as_ref().and_then(|f| f(item))
                    }
                    TreeViewEvent::DoubleClick => events
                        .on_activate
                        .as_ref()
                        .and_then(|f| f(sys::control::tv_selected(hwnd))),
                    TreeViewEvent::RightClick => events
                        .on_context
                        .as_ref()
                        .and_then(|f| f(sys::control::tv_selected(hwnd))),
                    _ => None,
                }
            };
            if let Some(msg) = msg {
                sink.emit(msg);
            }
            true
        });
        registry::register_app_events(hwnd, mapper);

        Ok(TreeView {
            control: Control::own(hwnd, bounds),
            inner,
            events,
        })
    }

    /// Maps a selection change to a message.
    pub fn on_select(self, f: impl Fn(Option<i64>) -> Option<M> + 'static) -> TreeView<M> {
        self.events.borrow_mut().on_select = Some(Box::new(f));
        self
    }

    /// Maps a double-click (activation) to a message.
    pub fn on_activate(self, f: impl Fn(Option<i64>) -> Option<M> + 'static) -> TreeView<M> {
        self.events.borrow_mut().on_activate = Some(Box::new(f));
        self
    }

    /// Maps a right-click to a message.
    pub fn on_context(self, f: impl Fn(Option<i64>) -> Option<M> + 'static) -> TreeView<M> {
        self.events.borrow_mut().on_context = Some(Box::new(f));
        self
    }

    /// Sets the tree's background and text colours.
    pub fn set_colors(&self, background: crate::Color, text: crate::Color) {
        sys::control::tv_set_colors(self.control.hwnd(), background, text);
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
        sys::control::tv_selected(self.control.hwnd())
    }

    /// The total number of materialised nodes.
    pub fn node_count(&self) -> i32 {
        sys::control::tv_count(self.control.hwnd())
    }
}

impl<M> AsControl for TreeView<M> {
    fn control(&self) -> &Control {
        &self.control
    }
}

impl<M> Drop for TreeView<M> {
    fn drop(&mut self) {
        registry::unregister(self.control.hwnd());
        registry::unregister_app_events(self.control.hwnd());
    }
}
