#![forbid(unsafe_code)]

//! A typed, lazily-loaded [`TreeView`] over a keyed [`TreeModel`].
//!
//! The model returns [`Node`]s keyed by `K`; the tree materializes them on
//! demand and keeps the key-to-handle mapping. [`TreeView::refresh`] then
//! diffs the loaded tree against the model by key — a pure function, unit
//! tested in [`diff`] — so a changing unread count updates in place while
//! expansion and selection survive. Event closures map to the app's `Msg`,
//! like every widget.
//!
//! ```no_run
//! use win32ui::prelude::*;
//!
//! #[derive(Clone, PartialEq, Eq, Hash)]
//! struct FolderId(u32);
//!
//! struct Folders;
//!
//! impl TreeModel for Folders {
//!     type Key = FolderId;
//!
//!     fn children(&self, parent: Option<&FolderId>) -> Vec<Node<FolderId>> {
//!         match parent {
//!             None => vec![Node::branch(FolderId(1), "Inbox")],
//!             Some(_) => Vec::new(),
//!         }
//!     }
//! }
//!
//! enum Msg {
//!     Open(FolderId),
//!     Folded(FolderId, bool),
//! }
//!
//! fn build(ui: &mut Ui<Msg>) -> win32ui::Result<TreeView<FolderId, Msg>> {
//!     let tree = TreeView::new(ui, Folders)?
//!         .style(|folder| NodeStyle::new().bold(folder.0 == 1).badge("3"))
//!         .on_select(|id| Some(Msg::Open(id.clone())))
//!         .on_toggle(|id, expanded| Some(Msg::Folded(id.clone(), expanded)));
//!     Ok(tree)
//! }
//! ```

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::Ui;
use crate::controls::control::{AsControl, Control};
use crate::controls::registry::{self, ControlEvents};
use crate::controls::treeview::events::{TreeViewEvents, install_mapper};
use std::hash::Hash;

use crate::controls::treeview::inner::TreeViewInner;
use crate::controls::{create_child, next_id, style as window_style};
use crate::error::Result;
use crate::gdi::Font;
use crate::geometry::Rect;
use crate::sys;
use crate::theme::{Theme, Themed};
use crate::units::{Dip, dip};

mod api;
mod apply;
mod diff;
mod draw;
mod events;
mod image_list;
mod inner;
mod model;
mod style;

pub use self::image_list::ImageList;
pub use self::model::{Node, TreeModel};
pub use self::style::NodeStyle;

/// The native item order helpers, re-exported for the retained state.
pub(crate) use crate::sys::treeview::{TVI_FIRST, TVI_ROOT};

// Tree-view styles, from `commctrl.h`.
const TVS_HASBUTTONS: u32 = 0x0000_0001;
const TVS_HASLINES: u32 = 0x0000_0002;
const TVS_LINESATROOT: u32 = 0x0000_0004;
const TVS_SHOWSELALWAYS: u32 = 0x0000_0020;
const TVS_FULLROWSELECT: u32 = 0x0000_1000;
const TVS_EX_DOUBLEBUFFER: u32 = 0x0000_0004;

/// A meaningful tree event, delivered to the parent window.
///
/// The widget layer maps these to the app's `Msg`; the `item` payloads are the
/// tree's internal node tokens and are only meaningful to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeViewEvent {
    /// The selected node changed (`None` when the selection was cleared).
    SelectionChanged {
        /// The new selection's node token, if any.
        item: Option<i64>,
    },
    /// A node was expanded or collapsed.
    Expanded {
        /// The node's token.
        item: i64,
        /// Whether it is now expanded.
        expanded: bool,
    },
    /// The selection was clicked.
    Click,
    /// The selection was double-clicked.
    DoubleClick,
    /// The selection was right-clicked.
    RightClick,
}

/// A lazily-loaded tree over nodes keyed by `K`, mapping its events to the
/// app's `Msg`.
///
/// Create it with [`new`](TreeView::new) and a [`TreeModel`], shape it with the
/// builders below, and place it in the layout tree — the window owns its
/// bounds, so no rectangle is needed here.
pub struct TreeView<K, M> {
    control: Control,
    inner: Rc<RefCell<TreeViewInner<K>>>,
    events: Rc<RefCell<TreeViewEvents<K, M>>>,
    sink: Ui<M>,
}

impl<K: Clone + Eq + Hash + 'static, M: 'static> TreeView<K, M> {
    /// Creates the control as a child of the window behind `ui`, adopting
    /// `ui`'s theme, and loads the model's roots.
    pub fn new(ui: &mut Ui<M>, model: impl TreeModel<Key = K> + 'static) -> Result<TreeView<K, M>> {
        let dpi = ui.dpi();
        let window = ui.hwnd();
        let theme = ui.theme();
        let style = window_style::WS_CHILD
            | window_style::WS_VISIBLE
            | window_style::WS_BORDER
            | window_style::WS_TABSTOP
            | TVS_HASBUTTONS
            | TVS_HASLINES
            | TVS_LINESATROOT
            | TVS_SHOWSELALWAYS
            | TVS_FULLROWSELECT;
        let hwnd = create_child(
            "TreeView",
            "SysTreeView32",
            window,
            style,
            window_style::WS_EX_CLIENTEDGE,
            next_id(),
            Rect::default(),
        )?;

        sys::client_edge::set(hwnd, !theme.is_dark);
        sys::treeview::tv_set_extended_style(hwnd, TVS_EX_DOUBLEBUFFER);
        sys::treeview::tv_set_item_height(hwnd, dip(20.0).to_px(dpi).value());
        sys::treeview::tv_set_colors(hwnd, theme.background, theme.text);
        sys::apply_native_theme(hwnd, sys::NativeControlKind::Scrollable, theme.is_dark);

        let font = Font::system_ui(dpi)?;

        let inner = Rc::new(RefCell::new(TreeViewInner {
            model: Some(Box::new(model)),
            entries: Vec::new(),
            handles: std::collections::HashMap::new(),
            keys: std::collections::HashMap::new(),
            styles: std::collections::HashMap::new(),
            next_token: 0,
            theme,
            font,
            style: None,
            image_list: None,
            dpi,
            last_selection: None,
            selection_muted: false,
            toggle_muted: false,
        }));

        let control_events: Rc<RefCell<dyn ControlEvents>> = inner.clone();
        registry::register(hwnd, control_events);

        let events = Rc::new(RefCell::new(TreeViewEvents::new()));
        install_mapper(Rc::clone(&inner), Rc::clone(&events), hwnd, ui.clone());

        // Load the roots now that notifications route to this tree.
        inner.borrow_mut().refresh(hwnd);

        {
            let weak = Rc::downgrade(&inner);
            crate::theme::register_themed(
                window,
                hwnd,
                Rc::new(move |applied| {
                    if let Some(inner) = weak.upgrade() {
                        inner.borrow_mut().theme = *applied;
                        sys::treeview::tv_set_colors(hwnd, applied.background, applied.text);
                        sys::apply_native_theme(
                            hwnd,
                            sys::NativeControlKind::Scrollable,
                            applied.is_dark,
                        );
                        sys::client_edge::set(hwnd, !applied.is_dark);
                        sys::window::invalidate(hwnd);
                    }
                }),
            );
        }

        Ok(TreeView {
            control: Control::own(hwnd, Rect::default()),
            inner,
            events,
            sink: ui.clone(),
        })
    }

    /// Overrides a node's appearance from `style(key)`. The closure is read
    /// when nodes are inserted and on every [`refresh`](TreeView::refresh), so
    /// it may depend on model state (an unread count) without running in the
    /// paint path; icons need an [`images`](TreeView::images) list.
    pub fn style(self, style: impl Fn(&K) -> NodeStyle + 'static) -> TreeView<K, M> {
        let hwnd = self.control.hwnd();
        self.inner.borrow_mut().style = Some(Box::new(style));
        self.inner.borrow_mut().rebuild_styles(hwnd);
        self
    }

    /// Gives the tree an image list for [`NodeStyle::icon`]. The list is owned
    /// by the tree and released when it drops.
    pub fn images(self, list: ImageList) -> TreeView<K, M> {
        let hwnd = self.control.hwnd();
        sys::treeview::tv_set_image_list(hwnd, Some(list.raw()));
        self.inner.borrow_mut().image_list = Some(list);
        self.inner.borrow_mut().rebuild_styles(hwnd);
        self
    }

    /// Sets every item's height, converted from `height` at the current DPI.
    pub fn item_height(self, height: Dip) -> TreeView<K, M> {
        let dpi = self.inner.borrow().dpi;
        sys::treeview::tv_set_item_height(self.control.hwnd(), height.to_px(dpi).value());
        self
    }

    /// Maps a selection change to a message.
    pub fn on_select(self, f: impl Fn(&K) -> Option<M> + 'static) -> TreeView<K, M> {
        self.events.borrow_mut().on_select = Some(Box::new(f));
        self
    }

    /// Maps a user expand/collapse to a message.
    pub fn on_toggle(self, f: impl Fn(&K, bool) -> Option<M> + 'static) -> TreeView<K, M> {
        self.events.borrow_mut().on_toggle = Some(Box::new(f));
        self
    }

    /// Maps a double-click (activation) of the selected node to a message.
    pub fn on_activate(self, f: impl Fn(&K) -> Option<M> + 'static) -> TreeView<K, M> {
        self.events.borrow_mut().on_activate = Some(Box::new(f));
        self
    }

    /// Maps a right-click of the selected node to a message.
    pub fn on_context(self, f: impl Fn(&K) -> Option<M> + 'static) -> TreeView<K, M> {
        self.events.borrow_mut().on_context = Some(Box::new(f));
        self
    }
}

impl<K, M> AsControl for TreeView<K, M> {
    fn control(&self) -> &Control {
        &self.control
    }
}

impl<K, M> Themed for TreeView<K, M> {
    fn apply_theme(&self, theme: &Theme) {
        self.inner.borrow_mut().theme = *theme;
        sys::treeview::tv_set_colors(self.control.hwnd(), theme.background, theme.text);
        sys::apply_native_theme(
            self.control.hwnd(),
            sys::NativeControlKind::Scrollable,
            theme.is_dark,
        );
        sys::client_edge::set(self.control.hwnd(), !theme.is_dark);
        sys::window::invalidate(self.control.hwnd());
    }
}

impl<K, M> Drop for TreeView<K, M> {
    fn drop(&mut self) {
        registry::unregister(self.control.hwnd());
        registry::unregister_app_events(self.control.hwnd());
        crate::theme::unregister_themed(self.control.hwnd());
    }
}
