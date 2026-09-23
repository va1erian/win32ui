#![forbid(unsafe_code)]

//! The [`ListView`](super::ListView) runtime API: models, targeted updates,
//! selection, columns and indicators.

use std::ops::Range;

use crate::controls::listview::{ColumnWidth, ListModel, ListView, SortDirection};
use crate::sys;

impl<T: 'static, M: 'static> ListView<T, M> {
    /// Replaces the data source and refreshes the view: the row count is
    /// re-read from the model and the whole control repaints.
    ///
    /// The control keeps its selected indices; the selection cache is
    /// re-synced silently, so no selection message is emitted.
    pub fn set_model(&self, model: impl ListModel<Item = T> + 'static) {
        let view = self.control.hwnd();
        self.inner.borrow_mut().model = Some(Box::new(model));
        sys::listview::lv_set_item_count(view, self.model_len());
        self.inner.borrow_mut().last_selection = sys::listview::lv_selected_all(view);
        sys::window::invalidate(view);
    }

    /// Repaints the rows in `range` (`LVM_REDRAWITEMS`), clamped to the model.
    /// Use after mutating those rows in place; an empty range does nothing.
    pub fn rows_changed(&self, rows: Range<usize>) {
        let len = self.model_len();
        let first = rows.start.min(len);
        let last = rows.end.min(len);
        if first < last {
            sys::listview::lv_redraw_items(self.control.hwnd(), first, last - 1);
        }
    }

    /// Refreshes the row count after inserting rows, then repaints from
    /// `range.start` to the end (everything there shifted). The view does not
    /// scroll (`LVSICF_NOSCROLL`).
    pub fn rows_inserted(&self, rows: Range<usize>) {
        self.recount(rows.start);
    }

    /// Refreshes the row count after removing rows, then repaints from
    /// `range.start` to the end. The view does not scroll.
    pub fn rows_removed(&self, rows: Range<usize>) {
        self.recount(rows.start);
    }

    fn recount(&self, changed_from: usize) {
        let view = self.control.hwnd();
        let len = self.model_len();
        sys::listview::lv_set_item_count(view, len);
        if changed_from < len {
            sys::listview::lv_redraw_items(view, changed_from, len - 1);
        }
    }

    fn model_len(&self) -> usize {
        self.inner
            .borrow()
            .model
            .as_ref()
            .map(|model| model.len())
            .unwrap_or(0)
    }

    /// Every selected row, ascending. Allocates; for the event path use
    /// [`on_select`](ListView::on_select) instead.
    pub fn selection(&self) -> Vec<usize> {
        sys::listview::lv_selected_all(self.control.hwnd())
    }

    /// The first selected row, if any.
    pub fn selected(&self) -> Option<usize> {
        sys::listview::lv_selected(self.control.hwnd()).map(|index| index as usize)
    }

    /// Makes `rows` the selection, deselecting everything else, and focuses
    /// the first one. Out-of-range and duplicate rows are dropped.
    ///
    /// Exactly one selection message is emitted when the selection actually
    /// changed — the per-item notifications the control sends while this runs
    /// are muted — so calling this from an `on_select` handler for the same
    /// rows cannot loop. A no-op when the selection already matches.
    pub fn set_selection(&self, rows: &[usize]) {
        let view = self.control.hwnd();
        let len = self.model_len();
        let mut next: Vec<usize> = rows.iter().copied().filter(|&row| row < len).collect();
        next.sort_unstable();
        next.dedup();
        if next == sys::listview::lv_selected_all(view) {
            self.inner.borrow_mut().last_selection = next;
            return;
        }
        self.inner.borrow_mut().selection_muted = true;
        sys::listview::lv_set_selection(view, &next);
        {
            let mut inner = self.inner.borrow_mut();
            inner.selection_muted = false;
            inner.last_selection = next.clone();
        }
        if let Some(msg) = self
            .events
            .borrow()
            .on_select
            .as_ref()
            .and_then(|f| f(&next))
        {
            self.sink.emit(msg);
        }
    }

    /// Selects and focuses `row`, deselecting everything else.
    pub fn select(&self, row: usize) {
        self.set_selection(&[row]);
    }

    /// The focused row, if any. Focus follows the selection for mouse and
    /// programmatic changes, and moves on its own with Ctrl+arrow keys.
    pub fn focused(&self) -> Option<usize> {
        sys::listview::lv_focused(self.control.hwnd())
    }

    /// Scrolls `row` into view, fully if it is not visible at all.
    pub fn ensure_visible(&self, row: usize) {
        sys::listview::lv_ensure_visible(self.control.hwnd(), row);
    }

    /// Sets one column's width. `Fill` columns give up their fixed width and
    /// rejoin sharing the leftover client space.
    pub fn set_column_width(&self, column: usize, width: impl Into<ColumnWidth>) {
        let width = width.into();
        let view = self.control.hwnd();
        let mut inner = self.inner.borrow_mut();
        let Some(spec) = inner.columns.get_mut(column) else {
            return;
        };
        spec.width = width;
        match width {
            ColumnWidth::Fixed(design) => {
                let px = design.to_px(inner.dpi).value();
                sys::listview::lv_set_column_width(view, column, px);
            }
            ColumnWidth::Fill => inner.restretch(view),
        }
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

    /// The number of columns.
    pub fn column_count(&self) -> usize {
        self.inner.borrow().columns.len()
    }

    /// The control's background colour.
    pub fn background_color(&self) -> crate::Color {
        sys::listview::lv_background(self.control.hwnd())
    }

    /// Reads back a cell's text (which re-enters the owner-data path). Useful
    /// for tests and for accessibility.
    pub fn cell_text(&self, item: usize, column: usize) -> String {
        sys::listview::lv_item_text(self.control.hwnd(), item as i32, column as i32)
    }
}
