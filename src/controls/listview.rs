#![forbid(unsafe_code)]

//! A virtual, owner-drawn report [`ListView`].
//!
//! The control is created with `LVS_OWNERDATA`, so it never stores the rows
//! itself: cell text is requested lazily through [`ListSource`], and row
//! colours/backgrounds are supplied through `NM_CUSTOMDRAW`. Both hooks are
//! handled inside the owner-draw plumbing and never reach the application;
//! only the meaningful events surface, mapped to the app's `Msg`.

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::Ui;
use crate::controls::control::{AsControl, Control};
use crate::controls::listview_inner::{HeaderDrawer, ListViewInner};
use crate::controls::registry::{self, ControlEvents};
use crate::controls::{create_child, next_id, style};
use crate::error::Result;
use crate::gdi::Font;
use crate::geometry::Rect;
use crate::hwnd::Hwnd;
use crate::message::{Key, Message, Modifiers, Notify};
use crate::sys;
use crate::theme::Theme;
use crate::units::Dip;

const LVS_REPORT: u32 = 0x0000_0001;
const LVS_SHOWSELALWAYS: u32 = 0x0000_0008;
const LVS_OWNERDATA: u32 = 0x0000_1000;
const LVS_EX_FULLROWSELECT: u32 = 0x0000_0020;
const LVS_EX_DOUBLEBUFFER: u32 = 0x0001_0000;

/// A report-mode column.
#[derive(Clone, Debug)]
pub struct Column {
    /// Header label.
    pub title: String,
    /// Initial width as a [`Dip`] design value.
    pub width: Dip,
    /// Whether the column's cells are right-aligned.
    pub align_right: bool,
}

impl Column {
    /// A left-aligned column.
    pub fn new(title: impl Into<String>, width: Dip) -> Column {
        Column {
            title: title.into(),
            width,
            align_right: false,
        }
    }

    /// A right-aligned column (numbers, durations).
    pub fn right(title: impl Into<String>, width: Dip) -> Column {
        Column {
            title: title.into(),
            width,
            align_right: true,
        }
    }
}

/// Supplies the virtual list view with its row count and cell text.
pub trait ListSource {
    /// The number of rows.
    fn item_count(&self) -> usize;

    /// The text of the cell at `item`/`column` (both zero-based).
    fn text(&self, item: usize, column: usize) -> String;
}

/// Which way a column is sorted, for the header arrow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDirection {
    /// Ascending (`HDF_SORTUP`).
    Ascending,
    /// Descending (`HDF_SORTDOWN`).
    Descending,
}

/// Colours used while owner-drawing the list.
#[derive(Clone, Copy, Debug)]
pub struct ListViewTheme {
    /// Even-row background.
    pub background: crate::Color,
    /// Odd-row background (the subtle zebra shade).
    pub alternate: crate::Color,
    /// Normal cell text.
    pub text: crate::Color,
    /// Selected-row background.
    pub selection: crate::Color,
    /// Background of the currently playing row.
    pub playing: crate::Color,
    /// Text colour on the playing/selected row.
    pub on_playing: crate::Color,
    /// Column-separator colour.
    pub border: crate::Color,
    /// Header background.
    pub header_background: crate::Color,
    /// Header label colour.
    pub header_text: crate::Color,
}

impl ListViewTheme {
    /// Derives a list palette from the app [`Theme`], tuned to match the egui
    /// frontend: a subtle two-shade zebra, a blue playing/selection highlight
    /// and thin column separators.
    pub fn from_theme(theme: &Theme) -> ListViewTheme {
        ListViewTheme {
            background: theme.background,
            alternate: theme.background.lerp(theme.text, 0.04),
            text: theme.text.lerp(theme.background, 0.12),
            selection: crate::Color::hex(0x2f_5f_8f),
            playing: crate::Color::hex(0x2f_5f_8f),
            on_playing: crate::Color::rgb(0xf2, 0xf2, 0xf2),
            border: theme.border,
            header_background: theme.background.lerp(theme.text, 0.07),
            header_text: theme.text.lerp(theme.background, 0.25),
        }
    }
}

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
type KeyMapper<M> = Box<dyn Fn(Key, Modifiers) -> Option<M>>;

/// The app-level events a [`ListView`] maps to `Msg`.
struct ListViewEvents<M> {
    on_select: Option<Box<dyn Fn(usize) -> Option<M>>>,
    on_activate: Option<Box<dyn Fn(usize) -> Option<M>>>,
    on_context: Option<Box<dyn Fn(usize) -> Option<M>>>,
    on_key: Option<KeyMapper<M>>,
}

/// A virtual report list view.
pub struct ListView<M> {
    control: Control,
    header: Hwnd,
    inner: Rc<RefCell<ListViewInner>>,
    header_subclass: Option<sys::control::HeaderSubclass>,
    events: Rc<RefCell<ListViewEvents<M>>>,
}

impl<M: 'static> ListView<M> {
    /// Creates the control as a child of the window behind `ui`.
    pub fn new(
        ui: &mut Ui<M>,
        bounds: Rect,
        columns: &[Column],
        source: Box<dyn ListSource>,
        theme: ListViewTheme,
    ) -> Result<ListView<M>> {
        let dpi = ui.dpi();
        let style = style::WS_CHILD
            | style::WS_VISIBLE
            | style::WS_BORDER
            | style::WS_TABSTOP
            | style::WS_VSCROLL
            | LVS_REPORT
            | LVS_SHOWSELALWAYS
            | LVS_OWNERDATA;
        let hwnd = create_child(
            "ListView",
            "SysListView32",
            ui.hwnd(),
            style,
            style::WS_EX_CLIENTEDGE,
            next_id(),
            bounds,
        )?;

        sys::control::lv_set_extended_style(hwnd, LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER);
        sys::control::lv_set_colors(hwnd, theme.background, theme.text);
        for (index, column) in columns.iter().enumerate() {
            sys::control::lv_insert_column(
                hwnd,
                index as i32,
                &column.title,
                column.width.to_px(dpi).value(),
                column.align_right,
            );
        }
        sys::control::lv_set_item_count(hwnd, source.item_count());

        // Match the egui frontend's font/row height, opt the control and its
        // header into the dark visual-style theme, and owner-draw the header.
        let font = Font::system_ui(dpi)?;
        sys::control::set_control_font(hwnd, font.raw());
        sys::control::set_dark_theme(hwnd);
        let header = sys::control::lv_header(hwnd);
        if !header.is_null() {
            sys::control::set_control_font(header, font.raw());
            sys::control::set_dark_theme(header);
        }

        let inner = Rc::new(RefCell::new(ListViewInner {
            source,
            theme,
            columns: columns.to_vec(),
            font,
            playing: None,
            sort: None,
        }));
        let control_events: Rc<RefCell<dyn ControlEvents>> = inner.clone();
        registry::register(hwnd, control_events);

        let header_subclass = if header.is_null() {
            None
        } else {
            sys::control::HeaderSubclass::install(
                hwnd,
                header,
                Box::new(HeaderDrawer::new(Rc::clone(&inner))),
            )
        };

        let events = Rc::new(RefCell::new(ListViewEvents {
            on_select: None,
            on_activate: None,
            on_context: None,
            on_key: None,
        }));
        let sink = ui.clone();
        let events_for_mapper = events.clone();
        let mapper: Rc<dyn Fn(&Message) -> bool> = Rc::new(move |message| {
            let Message::Notify(Notify::ListView { event, .. }) = message else {
                return false;
            };
            let msg = {
                let events = events_for_mapper.borrow();
                match *event {
                    ListViewEvent::ItemChanged {
                        item,
                        selected: true,
                    } if item >= 0 => events.on_select.as_ref().and_then(|f| f(item as usize)),
                    ListViewEvent::DoubleClick { item } if item >= 0 => {
                        events.on_activate.as_ref().and_then(|f| f(item as usize))
                    }
                    ListViewEvent::ReturnKey { item } if item >= 0 => {
                        events.on_activate.as_ref().and_then(|f| f(item as usize))
                    }
                    ListViewEvent::RightClick { item } if item >= 0 => {
                        events.on_context.as_ref().and_then(|f| f(item as usize))
                    }
                    ListViewEvent::KeyDown { key, modifiers } => events
                        .on_key
                        .as_ref()
                        .and_then(|f| f(Key::from_code(key), modifiers)),
                    _ => None,
                }
            };
            if let Some(msg) = msg {
                sink.emit(msg);
            }
            true
        });
        registry::register_app_events(hwnd, mapper);

        Ok(ListView {
            control: Control::own(hwnd, bounds),
            header,
            inner,
            header_subclass,
            events,
        })
    }

    /// Maps a selection change to a message.
    pub fn on_select(self, f: impl Fn(usize) -> Option<M> + 'static) -> ListView<M> {
        self.events.borrow_mut().on_select = Some(Box::new(f));
        self
    }

    /// Maps a double-click or Enter (activation) to a message.
    pub fn on_activate(self, f: impl Fn(usize) -> Option<M> + 'static) -> ListView<M> {
        self.events.borrow_mut().on_activate = Some(Box::new(f));
        self
    }

    /// Maps a right-click to a message.
    pub fn on_context(self, f: impl Fn(usize) -> Option<M> + 'static) -> ListView<M> {
        self.events.borrow_mut().on_context = Some(Box::new(f));
        self
    }

    /// Maps a key pressed while the list has focus to a message, together with
    /// the modifier state at that moment.
    pub fn on_key(self, f: impl Fn(Key, Modifiers) -> Option<M> + 'static) -> ListView<M> {
        self.events.borrow_mut().on_key = Some(Box::new(f));
        self
    }

    /// Updates the number of virtual rows after the source changed.
    pub fn set_item_count(&self, count: usize) {
        sys::control::lv_set_item_count(self.control.hwnd(), count);
    }

    /// Replaces the data source and refreshes the view.
    pub fn set_source(&self, source: Box<dyn ListSource>) {
        self.inner.borrow_mut().source = source;
        sys::control::lv_set_item_count(
            self.control.hwnd(),
            self.inner.borrow().source.item_count(),
        );
        sys::window::invalidate(self.control.hwnd());
    }

    /// Marks `row` as the now-playing row (highlighted during custom draw).
    pub fn set_playing(&self, row: Option<usize>) {
        self.inner.borrow_mut().playing = row;
        sys::window::invalidate(self.control.hwnd());
    }

    /// Shows a sort arrow on `column`.
    pub fn set_sort_indicator(&self, column: usize, direction: SortDirection) {
        self.inner.borrow_mut().sort = Some((column, direction == SortDirection::Ascending));
        sys::window::invalidate(self.header);
    }

    /// Removes the sort arrow from `column`.
    pub fn clear_sort_indicator(&self, column: usize) {
        let mut inner = self.inner.borrow_mut();
        if inner.sort.map(|(sorted, _)| sorted) == Some(column) {
            inner.sort = None;
        }
        drop(inner);
        sys::window::invalidate(self.header);
    }

    /// The first selected row, if any.
    pub fn selected(&self) -> Option<usize> {
        sys::control::lv_selected(self.control.hwnd()).map(|index| index as usize)
    }

    /// The control's background colour.
    pub fn background_color(&self) -> crate::Color {
        sys::control::lv_background(self.control.hwnd())
    }

    /// Reads back a cell's text (which re-enters the owner-data path). Useful
    /// for tests and for accessibility.
    pub fn cell_text(&self, item: usize, column: usize) -> String {
        sys::control::lv_item_text(self.control.hwnd(), item as i32, column as i32)
    }

    /// Selects and focuses `row`.
    pub fn select(&self, row: usize) {
        sys::control::lv_select(self.control.hwnd(), row as i32);
    }
}

impl<M> AsControl for ListView<M> {
    fn control(&self) -> &Control {
        &self.control
    }
}

impl<M> Drop for ListView<M> {
    fn drop(&mut self) {
        // Remove the header subclass before the window (and its header) go away.
        self.header_subclass = None;
        registry::unregister(self.control.hwnd());
        registry::unregister_app_events(self.control.hwnd());
    }
}
