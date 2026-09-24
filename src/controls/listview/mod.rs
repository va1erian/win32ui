#![forbid(unsafe_code)]

//! A virtual, owner-drawn report [`ListView`] over a typed [`ListModel`].
//!
//! The control is created with `LVS_OWNERDATA`, so it never stores the rows
//! itself: cell text is requested lazily through column accessors that borrow
//! `&str` from the row, and row colours/backgrounds are supplied through
//! `NM_CUSTOMDRAW`. Both hooks are handled inside the owner-draw plumbing and
//! never reach the application; only the meaningful events surface, mapped to
//! the app's `Msg` through the closures given at construction.
//!
//! ```rust
//! use win32ui::prelude::*;
//!
//! struct Mail {
//!     sender: String,
//!     subject: String,
//! }
//!
//! struct Mailbox {
//!     mails: Vec<Mail>,
//! }
//!
//! impl ListModel for Mailbox {
//!     type Item = Mail;
//!
//!     fn len(&self) -> usize {
//!         self.mails.len()
//!     }
//!
//!     fn get(&self, index: usize) -> Option<&Mail> {
//!         self.mails.as_slice().get(index)
//!     }
//! }
//!
//! enum Msg {
//!     Selected(Vec<usize>),
//!     Open(usize),
//! }
//!
//! fn build(ui: &mut Ui<Msg>, model: Mailbox) -> win32ui::Result<ListView<Mail, Msg>> {
//!     let list = ListView::new(ui)?
//!         .column("From", dip(180.0), |row: &Mail| row.sender.as_str())
//!         .column("Subject", Fill, |row: &Mail| row.subject.as_str())
//!         .multi_select(true)
//!         .on_select(|rows| Some(Msg::Selected(rows.to_vec())))
//!         .on_activate(|row| Some(Msg::Open(row)));
//!     list.set_model(model);
//!     Ok(list)
//! }
//! ```

use std::cell::RefCell;
use std::rc::Rc;

use windows::Win32::UI::Controls::LVS_SINGLESEL;

use crate::app::Ui;
use crate::controls::control::{AsControl, Control};
use crate::controls::listview::draw::{ListViewInner, StretchHandler};
use crate::controls::listview::events::{ListViewEvents, install_mapper};
use crate::controls::listview::header::HeaderDrawer;
use crate::controls::registry::{self, ControlEvents};
use crate::controls::{create_child, next_id, style};
use crate::error::Result;
use crate::gdi::{Font, FontWeight};
use crate::geometry::Rect;
use crate::hwnd::Hwnd;
use crate::sys;
use crate::theme::{Theme, Themed};

mod api;
mod builders;
mod draw;
pub(crate) mod events;
mod header;
mod model;
mod row_style;
mod theme;

pub use self::events::ListViewEvent;
pub use self::model::{Column, ColumnWidth, Fill, ListModel, SortDirection};
pub use self::row_style::{RowState, RowStyle};
pub use self::theme::ListViewTheme;

const LVS_REPORT: u32 = 0x0000_0001;
const LVS_SHOWSELALWAYS: u32 = 0x0000_0008;
const LVS_OWNERDATA: u32 = 0x0000_1000;
const LVS_EX_FULLROWSELECT: u32 = 0x0000_0020;
const LVS_EX_DOUBLEBUFFER: u32 = 0x0001_0000;

/// A virtual report list view over rows of type `T`, mapping its events to
/// the app's `Msg`.
///
/// Create it with [`new`](ListView::new), add columns with
/// [`column`](ListView::column), hand it a [`ListModel`] with
/// [`set_model`](ListView::set_model), and place it in the layout tree — the
/// window owns its bounds, so no rectangle is needed here.
pub struct ListView<T, M> {
    control: Control,
    header: Hwnd,
    inner: Rc<RefCell<ListViewInner<T>>>,
    header_subclass: Option<sys::listview_header::HeaderSubclass>,
    size_subclass: Option<sys::listview_header::SizeSubclass>,
    events: Rc<RefCell<ListViewEvents<M>>>,
    sink: Ui<M>,
}

impl<T: 'static, M: 'static> ListView<T, M> {
    /// Creates the control as a child of the window behind `ui`, adopting
    /// `ui`'s theme. Use [`Themed::apply_theme`] for a one-off override.
    ///
    /// The list starts single-select with no columns and no rows; the
    /// builders below shape it before it is placed in the layout.
    pub fn new(ui: &mut Ui<M>) -> Result<ListView<T, M>> {
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
            | LVS_OWNERDATA
            | LVS_SINGLESEL;
        let hwnd = create_child(
            "ListView",
            "SysListView32",
            window,
            style,
            style::WS_EX_CLIENTEDGE,
            next_id(),
            Rect::default(),
        )?;

        sys::listview::lv_set_extended_style(hwnd, LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER);
        sys::listview::lv_set_colors(hwnd, theme.background, theme.text);
        sys::listview::lv_set_item_count(hwnd, 0);

        // Match the egui frontend's font/row height, opt the control and its
        // header into the theme's visual style, and owner-draw the header.
        let font = Font::system_ui(dpi)?;
        // The bold variant for `RowStyle::bold` rows, created once here (never
        // per paint) so the hot custom-draw path allocates no GDI object.
        let bold_font = Font::new("Segoe UI", 9.75, FontWeight::Bold, dpi)?;
        sys::control::set_control_font(hwnd, font.raw());
        sys::apply_native_theme(hwnd, sys::NativeControlKind::Scrollable, ui.theme().is_dark);
        let header = sys::listview::lv_header(hwnd);
        if !header.is_null() {
            sys::control::set_control_font(header, font.raw());
            sys::apply_native_theme(
                header,
                sys::NativeControlKind::Scrollable,
                ui.theme().is_dark,
            );
        }

        let inner = Rc::new(RefCell::new(ListViewInner {
            model: None,
            theme,
            columns: Vec::new(),
            font,
            bold_font,
            row_style: None,
            row_painter: None,
            row_height: None,
            row_image_list: None,
            sort: None,
            dpi,
            last_selection: Vec::new(),
            selection_muted: false,
        }));
        let control_events: Rc<RefCell<dyn ControlEvents>> = inner.clone();
        registry::register(hwnd, control_events);

        let header_subclass = if header.is_null() {
            None
        } else {
            sys::listview_header::HeaderSubclass::install(
                hwnd,
                header,
                Box::new(HeaderDrawer::new(hwnd, Rc::clone(&inner))),
            )
        };
        let size_subclass = sys::listview_header::SizeSubclass::install(
            hwnd,
            Box::new(StretchHandler {
                view: hwnd,
                inner: Rc::clone(&inner),
            }),
        );

        let events = Rc::new(RefCell::new(ListViewEvents::new()));
        install_mapper(Rc::clone(&inner), Rc::clone(&events), hwnd, ui.clone());

        {
            let weak = Rc::downgrade(&inner);
            let header_copy = header;
            crate::theme::register_themed(
                window,
                hwnd,
                Rc::new(move |applied| {
                    if let Some(inner) = weak.upgrade() {
                        let mut state = inner.borrow_mut();
                        let zebra = state.theme.zebra;
                        state.theme = ListViewTheme::from_theme(applied);
                        state.theme.zebra = zebra;
                        drop(state);
                        sys::listview::lv_set_colors(
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
            control: Control::own(hwnd, Rect::default()),
            header,
            inner,
            header_subclass,
            size_subclass,
            events,
            sink: ui.clone(),
        })
    }
}

impl<T, M> AsControl for ListView<T, M> {
    fn control(&self) -> &Control {
        &self.control
    }
}

impl<T, M> Themed for ListView<T, M> {
    fn apply_theme(&self, theme: &Theme) {
        let mut inner = self.inner.borrow_mut();
        let zebra = inner.theme.zebra;
        inner.theme = ListViewTheme::from_theme(theme);
        inner.theme.zebra = zebra;
        drop(inner);
        let applied = self.inner.borrow().theme;
        sys::listview::lv_set_colors(self.control.hwnd(), applied.background, applied.text);
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

impl<T, M> Drop for ListView<T, M> {
    fn drop(&mut self) {
        // Remove the subclasses before the window (and its header) go away.
        self.header_subclass = None;
        self.size_subclass = None;
        registry::unregister(self.control.hwnd());
        registry::unregister_app_events(self.control.hwnd());
        crate::theme::unregister_themed(self.control.hwnd());
        if let Some(list) = self.inner.borrow_mut().row_image_list.take() {
            sys::listview::lv_destroy_image_list(list);
        }
    }
}
