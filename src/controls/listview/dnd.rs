#![forbid(unsafe_code)]

//! Drag and drop for [`ListView`]: the drag source hooks, the list as a drop
//! target with an insertion marker, and edge auto-scroll.
//!
//! `LVM_SETINSERTMARK` only exists for icon and tile views, so the marker is
//! drawn by the list's own row painting (see [`ListViewInner::draw_insert_mark`])
//! instead: a thin accent line on the top or bottom edge of the marked row.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::{Rc, Weak};

use crate::app::Ui;
use crate::controls::listview::ListView;
use crate::controls::listview::draw::ListViewInner;
use crate::controls::listview::events::ListViewEvents;
use crate::dnd::{DragData, DragImage, DragInfo, DropEffect, DropEffects};
use crate::error::Result;
use crate::gdi::Canvas;
use crate::geometry::Rect;
use crate::hwnd::Hwnd;
use crate::message::{Modifiers, MouseButton};
use crate::sys;
use crate::sys::dnd::{DataObj, DragPoint, TargetSink};
use crate::units::Dip;

/// How close to the top or bottom edge a drag starts auto-scrolling, in design
/// units.
const EDGE_ZONE_DIP: f32 = 24.0;
/// Thickness of the insertion marker, in design units.
const MARK_THICKNESS_DIP: f32 = 2.0;

/// A drop on a [`ListView`], as [`on_drop`](ListView::on_drop) reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListDrop {
    /// Where the dropped data goes: `0..=len`, the row index it should end up
    /// at. For an in-list reorder, remove the dragged rows first and adjust for
    /// the ones that were above this index.
    pub index: usize,
    /// The row under the pointer, if any.
    pub row: Option<usize>,
    /// The app payload of the drag ([`begin_drag`](ListView::begin_drag)), or
    /// `None` when the drag came from elsewhere.
    pub payload: Option<Vec<u8>>,
    /// Files dropped from Explorer; empty for an app drag.
    pub files: Vec<PathBuf>,
    /// The effect the drop was accepted with.
    pub effect: DropEffect,
    /// The modifier keys held at the drop.
    pub modifiers: Modifiers,
}

pub(crate) type DropMapper<M> = Box<dyn Fn(ListDrop) -> Option<M>>;
pub(crate) type DropFilter = Box<dyn Fn(&DragData<'_>) -> bool>;

/// The drop-target hooks of a list, kept with its other event mappers.
pub(crate) struct DropHooks<M> {
    pub(crate) on_drop: Option<DropMapper<M>>,
    pub(crate) accepts: Option<DropFilter>,
}

impl<M> DropHooks<M> {
    pub(crate) fn new() -> DropHooks<M> {
        DropHooks {
            on_drop: None,
            accepts: None,
        }
    }
}

/// Where a drag at some height would insert.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Insertion {
    /// The insertion index, `0..=len`.
    pub(super) index: usize,
    /// The row under the pointer.
    pub(super) row: Option<usize>,
    /// The marker: `(row, after)` — a line above or below that row.
    pub(super) mark: Option<(usize, bool)>,
}

/// Resolves the insertion point for a pointer at `y`. `hit` is the row under
/// it as `(row, top, bottom)`; `None` means past the last row (append).
pub(super) fn resolve(len: usize, hit: Option<(usize, i32, i32)>, y: i32) -> Insertion {
    if len == 0 {
        return Insertion {
            index: 0,
            row: None,
            mark: None,
        };
    }
    let (index, row) = match hit {
        Some((row, top, bottom)) => (row + usize::from(y >= (top + bottom) / 2), Some(row)),
        None => (len, None),
    };
    let index = index.min(len);
    let mark = if index < len {
        (index, false)
    } else {
        (len - 1, true)
    };
    Insertion {
        index,
        row,
        mark: Some(mark),
    }
}

impl<T> ListViewInner<T> {
    /// Draws the insertion marker into `row` when `item` carries it.
    pub(super) fn draw_insert_mark(&self, canvas: &Canvas, row: Rect, item: i32) {
        let Some((marked, after)) = self.insert_mark else {
            return;
        };
        if marked as i32 != item {
            return;
        }
        let thickness = Dip::new(MARK_THICKNESS_DIP).to_px(self.dpi).value().max(1);
        let line = if after {
            Rect::new(row.left, row.bottom - thickness, row.right, row.bottom)
        } else {
            Rect::new(row.left, row.top, row.right, row.top + thickness)
        };
        canvas.fill_rect(line, self.theme.accent);
    }
}

/// Moves the marker to `mark`, repainting the rows it leaves and reaches.
fn apply_mark<T>(inner: &RefCell<ListViewInner<T>>, view: Hwnd, mark: Option<(usize, bool)>) {
    let (previous, len) = {
        let mut state = inner.borrow_mut();
        let previous = std::mem::replace(&mut state.insert_mark, mark);
        let len = state.model.as_ref().map_or(0, |model| model.len());
        (previous, len)
    };
    if previous == mark {
        return;
    }
    for (row, _) in [previous, mark].into_iter().flatten() {
        if row < len {
            sys::listview::lv_redraw_items(view, row, row);
        }
    }
}

/// The list as an OLE drop target.
struct ListDropSink<T, M> {
    view: Hwnd,
    inner: Weak<RefCell<ListViewInner<T>>>,
    events: Weak<RefCell<ListViewEvents<M>>>,
    ui: Ui<M>,
}

impl<T: 'static, M: 'static> ListDropSink<T, M> {
    fn len(inner: &RefCell<ListViewInner<T>>) -> usize {
        inner.borrow().model.as_ref().map_or(0, |model| model.len())
    }

    /// The raw effect bits this list applies to `data` at `point`.
    fn effect(&self, point: DragPoint, data: &DataObj) -> u32 {
        let info = DragInfo::new(point, data);
        let Some(events) = self.events.upgrade() else {
            return 0;
        };
        let Ok(events) = events.try_borrow() else {
            return 0;
        };
        let accepted = match &events.drop.accepts {
            Some(filter) => filter(&info.data),
            None => info.data.has_payload() || info.data.has_files(),
        };
        if accepted {
            info.preferred_effect().bits()
        } else {
            0
        }
    }

    /// Where a drag at client height `y` would insert.
    fn locate(&self, len: usize, y: i32) -> Insertion {
        let visible = sys::listview::lv_visible_rows(self.view, len);
        let bounds = |row: usize| sys::listview::lv_subitem_rect(self.view, row as i32, 0);
        let Some(first) = visible.clone().next() else {
            return resolve(len, None, y);
        };
        // The pointer over the header counts as the top of the first row.
        let y = y.max(bounds(first).top);
        let hit = visible
            .map(|row| (row, bounds(row)))
            .find_map(|(row, rect)| {
                (y >= rect.top && y < rect.bottom).then_some((row, rect.top, rect.bottom))
            });
        resolve(len, hit, y)
    }

    /// Scrolls one row when the pointer is in the top or bottom edge zone.
    fn auto_scroll(&self, inner: &RefCell<ListViewInner<T>>, len: usize, y: i32) {
        let dpi = inner.borrow().dpi;
        let zone = Dip::new(EDGE_ZONE_DIP).to_px(dpi).value();
        let visible = sys::listview::lv_visible_rows(self.view, len);
        let content_top = if visible.start < len {
            sys::listview::lv_subitem_rect(self.view, visible.start as i32, 0).top
        } else {
            0
        };
        let height = sys::window::client_rect(self.view).height();
        if y < content_top + zone && visible.start > 0 {
            sys::listview::lv_ensure_visible(self.view, visible.start - 1);
        } else if y > height - zone && visible.end < len {
            sys::listview::lv_ensure_visible(self.view, visible.end);
        }
    }

    fn track(&self, point: DragPoint, data: &DataObj) -> u32 {
        let Some(inner) = self.inner.upgrade() else {
            return 0;
        };
        let effect = self.effect(point, data);
        let len = Self::len(&inner);
        if effect == 0 {
            apply_mark(&inner, self.view, None);
        } else {
            apply_mark(&inner, self.view, self.locate(len, point.y).mark);
        }
        self.auto_scroll(&inner, len, point.y);
        effect
    }
}

impl<T: 'static, M: 'static> TargetSink for ListDropSink<T, M> {
    fn enter(&self, point: DragPoint, data: &DataObj) -> u32 {
        self.track(point, data)
    }

    fn over(&self, point: DragPoint, data: &DataObj) -> u32 {
        self.track(point, data)
    }

    fn leave(&self) {
        if let Some(inner) = self.inner.upgrade() {
            apply_mark(&inner, self.view, None);
        }
    }

    fn dropped(&self, point: DragPoint, data: &DataObj) -> u32 {
        let Some(inner) = self.inner.upgrade() else {
            return 0;
        };
        apply_mark(&inner, self.view, None);
        let effect = self.effect(point, data);
        if effect == 0 {
            return 0;
        }
        let insertion = self.locate(Self::len(&inner), point.y);
        let info = DragInfo::new(point, data);
        let drop = ListDrop {
            index: insertion.index,
            row: insertion.row,
            payload: info.data.payload(),
            files: info.data.files(),
            effect: DropEffect::from_bits(effect),
            modifiers: info.modifiers,
        };
        let message = self
            .events
            .upgrade()
            .and_then(|events| events.try_borrow().ok()?.drop.on_drop.as_ref()?(drop));
        if let Some(message) = message {
            self.ui.emit(message);
        }
        effect
    }
}

impl<T: 'static, M: 'static> ListView<T, M> {
    /// Maps the start of a row drag to a message: the closure receives the
    /// dragged rows (the selection, ascending) and the mouse button. Answer
    /// from `update` with [`begin_drag`](ListView::begin_drag) while the
    /// button is still down.
    pub fn on_begin_drag(
        self,
        f: impl Fn(&[usize], MouseButton) -> Option<M> + 'static,
    ) -> ListView<T, M> {
        self.events.borrow_mut().on_begin_drag = Some(Box::new(f));
        self
    }

    /// Restricts which drags the list accepts as a drop target (see
    /// [`on_drop`](ListView::on_drop)). Without a filter it accepts any drag
    /// carrying an app payload or files.
    pub fn drop_filter(self, f: impl Fn(&DragData<'_>) -> bool + 'static) -> ListView<T, M> {
        self.events.borrow_mut().drop.accepts = Some(Box::new(f));
        self
    }

    /// Makes the list a drop target and maps a drop on it to a message.
    ///
    /// While a drag is over the list it shows an insertion marker between the
    /// rows and scrolls near its top and bottom edges; [`ListDrop::index`] is
    /// the insertion index, so the same hook serves dropping in from another
    /// widget, reordering in place, and Explorer file drops.
    ///
    /// Fails when OLE cannot be initialised on this thread (see
    /// [`crate::dnd`]).
    pub fn on_drop(self, f: impl Fn(ListDrop) -> Option<M> + 'static) -> Result<ListView<T, M>> {
        self.events.borrow_mut().drop.on_drop = Some(Box::new(f));
        let sink: Rc<dyn TargetSink> = Rc::new(ListDropSink {
            view: self.control.hwnd(),
            inner: Rc::downgrade(&self.inner),
            events: Rc::downgrade(&self.events),
            ui: self.sink.clone(),
        });
        let registration = sys::dnd::register_target(self.control.hwnd(), sink)?;
        self.drop_target.replace(Some(registration));
        Ok(self)
    }

    /// Starts a drag of the list's rows carrying `payload`; see
    /// [`begin_drag`](crate::dnd::begin_drag). Without an `image` the list
    /// paints its own drag image.
    pub fn begin_drag(
        &self,
        payload: &[u8],
        allowed: DropEffects,
        image: Option<&DragImage>,
    ) -> Result<DropEffect> {
        crate::dnd::begin_drag(self.control.hwnd(), payload, allowed, image)
    }

    /// Shows an insertion marker: a line above (`after == false`) or below
    /// (`after == true`) `row`, or removes it with `None`. The list shows and
    /// clears its own marker while it is a drop target; use this for a marker
    /// driven by something else, such as a keyboard reorder.
    pub fn set_insert_mark(&self, mark: Option<(usize, bool)>) {
        apply_mark(&self.inner, self.control.hwnd(), mark);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upper_half_inserts_before_and_lower_half_after() {
        let hit = Some((3, 100, 120));
        assert_eq!(resolve(10, hit, 105).index, 3);
        assert_eq!(resolve(10, hit, 105).mark, Some((3, false)));
        assert_eq!(resolve(10, hit, 115).index, 4);
        assert_eq!(resolve(10, hit, 115).mark, Some((4, false)));
        assert_eq!(resolve(10, hit, 115).row, Some(3));
    }

    #[test]
    fn past_the_last_row_appends_with_a_marker_below_it() {
        let end = resolve(10, None, 900);
        assert_eq!(end.index, 10);
        assert_eq!(end.mark, Some((9, true)));
        let last_lower = resolve(10, Some((9, 200, 220)), 219);
        assert_eq!(last_lower.index, 10);
        assert_eq!(last_lower.mark, Some((9, true)));
    }

    #[test]
    fn an_empty_list_has_no_marker() {
        let empty = resolve(0, None, 5);
        assert_eq!((empty.index, empty.mark), (0, None));
    }
}
