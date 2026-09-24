#![forbid(unsafe_code)]

//! The [`ListView`] builder chain: columns, selection mode, event mapping and
//! row appearance (`row_style`/`row_painter`/`row_height`/`zebra`).

use crate::controls::listview::ListView;
use crate::controls::listview::model::{Column, ColumnWidth};
use crate::controls::listview::style::{RowState, RowStyle};
use crate::gdi::Canvas;
use crate::geometry::Rect;
use crate::message::{Key, Modifiers};
use crate::sys;
use crate::units::Dip;

impl<T: 'static, M: 'static> ListView<T, M> {
    /// Adds a left-aligned column showing `text(row)`.
    pub fn column(
        self,
        title: impl Into<String>,
        width: impl Into<ColumnWidth>,
        text: impl for<'a> Fn(&'a T) -> &'a str + 'static,
    ) -> ListView<T, M> {
        self.push_column(Column::new(title, width, text));
        self
    }

    /// Adds a right-aligned column (numbers, durations) showing `text(row)`.
    pub fn column_right(
        self,
        title: impl Into<String>,
        width: impl Into<ColumnWidth>,
        text: impl for<'a> Fn(&'a T) -> &'a str + 'static,
    ) -> ListView<T, M> {
        self.push_column(Column::right(title, width, text));
        self
    }

    fn push_column(&self, column: Column<T>) {
        let view = self.control.hwnd();
        let index = self.inner.borrow().columns.len();
        // Fixed columns convert their design width now; `Fill` columns take a
        // placeholder until the restretch below (or the first `WM_SIZE`)
        // shares out the leftover client width.
        let fixed = match column.width {
            ColumnWidth::Fixed(width) => width.to_px(self.inner.borrow().dpi).value(),
            ColumnWidth::Fill => Dip::new(64.0).to_px(self.inner.borrow().dpi).value(),
        };
        sys::listview::lv_insert_column(
            view,
            index as i32,
            &column.title,
            fixed,
            column.align_right,
        );
        self.inner.borrow_mut().columns.push(column);
        self.inner.borrow().restretch(view);
    }

    /// Enables or disables multi-select (`LVS_SINGLESEL` off or on). The list
    /// starts single-select.
    pub fn multi_select(self, multi: bool) -> ListView<T, M> {
        sys::listview::lv_set_single_select(self.control.hwnd(), !multi);
        self
    }

    /// Maps a selection change to a message. The slice holds every selected
    /// row, ascending — empty when the selection was cleared.
    pub fn on_select(self, f: impl Fn(&[usize]) -> Option<M> + 'static) -> ListView<T, M> {
        self.events.borrow_mut().on_select = Some(Box::new(f));
        self
    }

    /// Maps a double-click or Enter (activation) to a message.
    pub fn on_activate(self, f: impl Fn(usize) -> Option<M> + 'static) -> ListView<T, M> {
        self.events.borrow_mut().on_activate = Some(Box::new(f));
        self
    }

    /// Maps a right-click to a message.
    pub fn on_context(self, f: impl Fn(usize) -> Option<M> + 'static) -> ListView<T, M> {
        self.events.borrow_mut().on_context = Some(Box::new(f));
        self
    }

    /// Maps a header click to a message. The app sorts (or asks for a sort)
    /// and shows the arrow with
    /// [`set_sort_indicator`](ListView::set_sort_indicator).
    pub fn on_sort(self, f: impl Fn(usize) -> Option<M> + 'static) -> ListView<T, M> {
        self.events.borrow_mut().on_sort = Some(Box::new(f));
        self
    }

    /// Maps a key pressed while the list has focus to a message, together with
    /// the modifier state at that moment.
    pub fn on_key(self, f: impl Fn(Key, Modifiers) -> Option<M> + 'static) -> ListView<T, M> {
        self.events.borrow_mut().on_key = Some(Box::new(f));
        self
    }

    /// Overrides a row's weight and colours from `row(T)`, a plain semantic
    /// override of the theme: every [`RowStyle`] field is optional, so a row
    /// that returns `RowStyle::default()` keeps the theme's usual look.
    /// Ignored for a row [`row_painter`](ListView::row_painter) fully paints.
    pub fn row_style(self, f: impl Fn(&T) -> RowStyle + 'static) -> ListView<T, M> {
        self.inner.borrow_mut().row_style = Some(Box::new(f));
        sys::window::invalidate(self.control.hwnd());
        self
    }

    /// Takes over painting a row, e.g. for multi-line rows (a sender and date
    /// over the subject). `paint(row, canvas, rect, state)` returns `true`
    /// once it painted the row itself, skipping the default painting; `false`
    /// falls back to the default (applying [`row_style`](ListView::row_style)
    /// if set).
    pub fn row_painter(
        self,
        paint: impl Fn(&T, &Canvas, Rect, RowState) -> bool + 'static,
    ) -> ListView<T, M> {
        self.inner.borrow_mut().row_painter = Some(Box::new(paint));
        sys::window::invalidate(self.control.hwnd());
        self
    }

    /// Sets a fixed row height, converted from `height` at the current DPI
    /// and kept correct across `WM_DPICHANGED` (the pixel height is
    /// recomputed from `height` whenever the list's effective DPI changes).
    /// See the doc comment on `sys::listview::lv_set_row_height` for why an
    /// image list — rather than `LVS_OWNERDRAWFIXED` + `WM_MEASUREITEM` —
    /// sets the height on this `LVS_OWNERDATA` list.
    pub fn row_height(self, height: Dip) -> ListView<T, M> {
        let view = self.control.hwnd();
        let mut inner = self.inner.borrow_mut();
        inner.row_height = Some(height);
        let px = height.to_px(inner.dpi).value();
        let previous = inner.row_image_list.take();
        inner.row_image_list = sys::listview::lv_set_row_height(view, px, previous);
        drop(inner);
        self
    }

    /// Turns on the subtle alternate-row zebra background (off by default);
    /// a [`row_style`](ListView::row_style) background override, or a row a
    /// [`row_painter`](ListView::row_painter) fully paints, wins over it.
    pub fn zebra(self, on: bool) -> ListView<T, M> {
        self.inner.borrow_mut().theme.zebra = on;
        sys::window::invalidate(self.control.hwnd());
        self
    }
}
