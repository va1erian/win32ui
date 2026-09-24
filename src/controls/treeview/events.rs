#![forbid(unsafe_code)]

//! Mapping decoded tree notifications to the app's `Msg`, resolving each
//! native node token back to its typed key.

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::Ui;
use crate::controls::registry;
use crate::controls::treeview::TreeViewEvent;
use std::hash::Hash;

use crate::controls::treeview::inner::TreeViewInner;
use crate::hwnd::Hwnd;
use crate::message::{Message, Notify};

/// Maps the currently selected key to an optional app message.
pub(crate) type SelectMapper<K, M> = Box<dyn Fn(&K) -> Option<M>>;

/// Maps an expanded/collapsed node to an optional app message.
pub(crate) type ToggleMapper<K, M> = Box<dyn Fn(&K, bool) -> Option<M>>;

/// The app-level events a [`TreeView`](super::TreeView) maps to `Msg`.
pub(crate) struct TreeViewEvents<K, M> {
    pub(crate) on_select: Option<SelectMapper<K, M>>,
    pub(crate) on_toggle: Option<ToggleMapper<K, M>>,
    pub(crate) on_activate: Option<SelectMapper<K, M>>,
    pub(crate) on_context: Option<SelectMapper<K, M>>,
}

impl<K, M> TreeViewEvents<K, M> {
    pub(crate) fn new() -> TreeViewEvents<K, M> {
        TreeViewEvents {
            on_select: None,
            on_toggle: None,
            on_activate: None,
            on_context: None,
        }
    }
}

/// Routes decoded [`Notify::TreeView`] messages to the app's `Msg` through the
/// closures given at construction, delivering them through the existing `Msg`
/// queue (never re-entered).
pub(crate) fn install_mapper<K: Clone + Eq + Hash + 'static, M: 'static>(
    inner: Rc<RefCell<TreeViewInner<K>>>,
    events: Rc<RefCell<TreeViewEvents<K, M>>>,
    view: Hwnd,
    sink: Ui<M>,
) {
    let mapper: Rc<dyn Fn(&Message) -> bool> = Rc::new(move |message| {
        let Message::Notify(Notify::TreeView { event, .. }) = message else {
            return false;
        };
        match *event {
            TreeViewEvent::SelectionChanged { item: Some(token) } => {
                let key = {
                    let mut state = inner.borrow_mut();
                    if state.selection_muted || state.last_selection == Some(token) {
                        return true;
                    }
                    state.last_selection = Some(token);
                    state.key_of_token(token)
                };
                if let Some(key) = key
                    && let Some(msg) = events.borrow().on_select.as_ref().and_then(|f| f(&key))
                {
                    sink.emit(msg);
                }
            }
            TreeViewEvent::Expanded { item, expanded } => {
                let key = {
                    let state = inner.borrow();
                    if state.toggle_muted {
                        return true;
                    }
                    state.key_of_token(item)
                };
                if let Some(key) = key
                    && let Some(msg) = events
                        .borrow()
                        .on_toggle
                        .as_ref()
                        .and_then(|f| f(&key, expanded))
                {
                    sink.emit(msg);
                }
            }
            TreeViewEvent::DoubleClick | TreeViewEvent::RightClick => {
                let Some(key) = inner.borrow().selected_key(view) else {
                    return true;
                };
                let msg = {
                    let events = events.borrow();
                    let mapper = if matches!(*event, TreeViewEvent::DoubleClick) {
                        events.on_activate.as_ref()
                    } else {
                        events.on_context.as_ref()
                    };
                    mapper.and_then(|f| f(&key))
                };
                if let Some(msg) = msg {
                    sink.emit(msg);
                }
            }
            TreeViewEvent::SelectionChanged { item: None } | TreeViewEvent::Click => {}
        }
        true
    });
    registry::register_app_events(view, mapper);
}
