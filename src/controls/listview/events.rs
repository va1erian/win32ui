#![forbid(unsafe_code)]

//! The [`ListViewEvent`] enum and the widget-layer mapping to the app's `Msg`.

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::Ui;
use crate::controls::listview::draw::ListViewInner;
use crate::controls::registry;
use crate::geometry::Point;
use crate::hwnd::Hwnd;
use crate::message::{Key, Message, Modifiers, MouseButton, Notify};
use crate::sys;

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
    /// An owner-data range changed its selection state (`LVN_ODSTATECHANGED`).
    /// The widget answers this by reading the whole selection, so one user
    /// gesture maps to one selection message no matter how many rows moved.
    SelectionChanged {
        /// First row of the changed range.
        from: i32,
        /// Last row of the changed range.
        to: i32,
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
    /// The user started dragging rows (`LVN_BEGINDRAG`, or `LVN_BEGINRDRAG`
    /// with the right button). The dragged rows are the selection; answer with
    /// [`ListView::begin_drag`](super::ListView::begin_drag).
    BeginDrag {
        /// The row under the pointer when the drag started.
        item: i32,
        /// Whether the right button started the drag.
        right_button: bool,
    },
    /// A key was pressed while the list had focus.
    KeyDown {
        /// The virtual-key code.
        key: u16,
        /// The modifier keys held when the key was pressed.
        modifiers: Modifiers,
    },
}

/// Maps the current selection to an optional app message.
pub(crate) type SelectMapper<M> = Box<dyn Fn(&[usize]) -> Option<M>>;

/// Maps a focused key press, with its modifiers, to an optional app message.
pub(crate) type KeyMapper<M> = Box<dyn Fn(Key, Modifiers) -> Option<M>>;

/// Maps a left click on a cell — `(item, sub_item, point)` — to an optional
/// app message. A `Some` message consumes the click.
pub(crate) type CellClickMapper<M> = Box<dyn Fn(usize, usize, Point) -> Option<M>>;

/// Maps the dragged rows and the button that dragged them to an optional app
/// message.
pub(crate) type BeginDragMapper<M> = Box<dyn Fn(&[usize], MouseButton) -> Option<M>>;

/// The app-level events a [`ListView`](super::ListView) maps to `Msg`.
pub(crate) struct ListViewEvents<M> {
    pub(crate) on_select: Option<SelectMapper<M>>,
    pub(crate) on_activate: Option<Box<dyn Fn(usize) -> Option<M>>>,
    pub(crate) on_context: Option<Box<dyn Fn(usize) -> Option<M>>>,
    pub(crate) on_sort: Option<Box<dyn Fn(usize) -> Option<M>>>,
    pub(crate) on_cell_click: Option<CellClickMapper<M>>,
    pub(crate) on_key: Option<KeyMapper<M>>,
    pub(crate) on_begin_drag: Option<BeginDragMapper<M>>,
    pub(crate) drop: super::dnd::DropHooks<M>,
}

impl<M> ListViewEvents<M> {
    pub(crate) fn new() -> ListViewEvents<M> {
        ListViewEvents {
            on_select: None,
            on_activate: None,
            on_context: None,
            on_sort: None,
            on_cell_click: None,
            on_key: None,
            on_begin_drag: None,
            drop: super::dnd::DropHooks::new(),
        }
    }
}

/// Routes decoded [`Notify::ListView`] messages to the app's `Msg` through
/// the closures given at construction, delivering them through the existing
/// `Msg` queue (never re-entered).
///
/// Selection notifications funnel into one coalesced report; every other
/// event maps straight to a message.
pub(crate) fn install_mapper<T: 'static, M: 'static>(
    inner: Rc<RefCell<ListViewInner<T>>>,
    events: Rc<RefCell<ListViewEvents<M>>>,
    view: Hwnd,
    sink: Ui<M>,
) {
    let mapper: Rc<dyn Fn(&Message) -> bool> = Rc::new(move |message| {
        let Message::Notify(Notify::ListView { event, .. }) = message else {
            return false;
        };
        match *event {
            ListViewEvent::SelectionChanged { .. } | ListViewEvent::ItemChanged { .. } => {
                emit_selection(&inner, &events, view, &sink);
            }
            _ => {
                let msg = {
                    let events = events.borrow();
                    match *event {
                        ListViewEvent::DoubleClick { item } if item >= 0 => {
                            events.on_activate.as_ref().and_then(|f| f(item as usize))
                        }
                        ListViewEvent::ReturnKey { item } if item >= 0 => {
                            events.on_activate.as_ref().and_then(|f| f(item as usize))
                        }
                        ListViewEvent::RightClick { item } if item >= 0 => {
                            events.on_context.as_ref().and_then(|f| f(item as usize))
                        }
                        ListViewEvent::ColumnClick { column } if column >= 0 => {
                            events.on_sort.as_ref().and_then(|f| f(column as usize))
                        }
                        ListViewEvent::KeyDown { key, modifiers } => {
                            let context = wants_context_menu(key, modifiers)
                                .then(|| sys::listview::lv_focused(view))
                                .flatten()
                                .and_then(|row| events.on_context.as_ref().and_then(|f| f(row)));
                            if let Some(context) = context {
                                sink.emit(context);
                            }
                            events
                                .on_key
                                .as_ref()
                                .and_then(|f| f(Key::from_code(key), modifiers))
                        }
                        ListViewEvent::BeginDrag { item, right_button } => {
                            events.on_begin_drag.as_ref().and_then(|f| {
                                let mut rows = sys::listview::lv_selected_all(view);
                                if item >= 0 && !rows.contains(&(item as usize)) {
                                    rows = vec![item as usize];
                                }
                                let button = if right_button {
                                    MouseButton::Right
                                } else {
                                    MouseButton::Left
                                };
                                f(&rows, button)
                            })
                        }
                        _ => None,
                    }
                };
                if let Some(msg) = msg {
                    sink.emit(msg);
                }
            }
        }
        true
    });
    registry::register_app_events(view, mapper);
}

/// Whether a key press asks for the context menu of the focused row: the
/// Apps key, or Shift+F10 (`WM_CONTEXTMENU` from the keyboard).
fn wants_context_menu(key: u16, modifiers: Modifiers) -> bool {
    key == Key::APPS.code() || (key == Key::F10.code() && modifiers.shift)
}

/// Reads the control's current selection and maps it to a message, unless it
/// is unchanged since the last report or the widget is applying a programmatic
/// change (which reports its own single event instead).
///
/// Both `LVN_ITEMCHANGED` and `LVN_ODSTATECHANGED` funnel through here, so a
/// gesture that raises several notifications still emits at most one message —
/// and only when the selection actually moved.
pub(crate) fn emit_selection<T: 'static, M: 'static>(
    inner: &Rc<RefCell<ListViewInner<T>>>,
    events: &Rc<RefCell<ListViewEvents<M>>>,
    view: Hwnd,
    sink: &Ui<M>,
) {
    let selection = sys::listview::lv_selected_all(view);
    let mut state = inner.borrow_mut();
    if state.selection_muted || selection == state.last_selection {
        return;
    }
    state.last_selection = selection.clone();
    drop(state);
    if let Some(msg) = events
        .borrow()
        .on_select
        .as_ref()
        .and_then(|f| f(&selection))
    {
        sink.emit(msg);
    }
}
