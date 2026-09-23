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
use crate::theme::{Theme, Themed};
use crate::units::Dip;

pub use super::listview_events::ListViewEvent;
use super::listview_events::ListViewEvents;
pub use super::listview_theme::ListViewTheme;

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

/// A virtual report list view.
pub struct ListView<M> {
    control: Control,
    header: Hwnd,
    inner: Rc<RefCell<ListViewInner>>,
    header_subclass: Option<sys::control::HeaderSubclass>,
    events: Rc<RefCell<ListViewEvents<M>>>,
}

impl<M: 'static> ListView<M> {
    /// Creates the control as a child of the window behind `ui`, adopting
    /// `ui`'s theme. Use [`Themed::apply_theme`] for a one-off override.
    pub fn new(
        ui: &mut Ui<M>,
        bounds: Rect,
        columns: &[Column],
        source: Box<dyn ListSource>,
    ) -> Result<ListView<M>> {
        let dpi = ui.dpi();
        let window = ui.hwnd();
        let theme = ListViewTheme::from_theme(&ui.theme());
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
        // header into the theme's visual style, and owner-draw the header.
        let font = Font::system_ui(dpi)?;
        sys::control::set_control_font(hwnd, font.raw());
        sys::apply_native_theme(hwnd, sys::NativeControlKind::Scrollable, ui.theme().is_dark);
        let header = sys::control::lv_header(hwnd);
        if !header.is_null() {
            sys::control::set_control_font(header, font.raw());
            sys::apply_native_theme(
                header,
                sys::NativeControlKind::Scrollable,
                ui.theme().is_dark,
            );
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

        let events = Rc::new(RefCell::new(ListViewEvents::new()));
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

        {
            let weak = Rc::downgrade(&inner);
            let header_copy = header;
            crate::theme::register_themed(
                window,
                hwnd,
                Rc::new(move |applied| {
                    if let Some(inner) = weak.upgrade() {
                        inner.borrow_mut().theme = ListViewTheme::from_theme(applied);
                        sys::control::lv_set_colors(
                            hwnd,
                            inner.borrow().theme.background,
                            inner.borrow().theme.text,
                        );
                        sys::apply_native_theme(
                            hwnd,
                            sys::NativeControlKind::Scrollable,
                            applied.is_dark,
                        );
                        if !header_copy.is_null() {
                            sys::apply_native_theme(
                                header_copy,
                                sys::NativeControlKind::Scrollable,
                                applied.is_dark,
                            );
                            sys::window::invalidate(header_copy);
                        }
                        sys::window::invalidate(hwnd);
                    }
                }),
            );
        }

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

impl<M> Themed for ListView<M> {
    fn apply_theme(&self, theme: &Theme) {
        self.inner.borrow_mut().theme = ListViewTheme::from_theme(theme);
        let applied = self.inner.borrow().theme;
        sys::control::lv_set_colors(self.control.hwnd(), applied.background, applied.text);
        sys::apply_native_theme(
            self.control.hwnd(),
            sys::NativeControlKind::Scrollable,
            theme.is_dark,
        );
        if !self.header.is_null() {
            sys::apply_native_theme(
                self.header,
                sys::NativeControlKind::Scrollable,
                theme.is_dark,
            );
            sys::window::invalidate(self.header);
        }
        sys::window::invalidate(self.control.hwnd());
    }
}

impl<M> Drop for ListView<M> {
    fn drop(&mut self) {
        // Remove the header subclass before the window (and its header) go away.
        self.header_subclass = None;
        registry::unregister(self.control.hwnd());
        registry::unregister_app_events(self.control.hwnd());
        crate::theme::unregister_themed(self.control.hwnd());
    }
}
