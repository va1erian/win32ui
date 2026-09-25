#![forbid(unsafe_code)]

//! The list view's owner-drawn header: background, labels, separators and the
//! sort arrow, all painted in the app's colours.

use std::cell::RefCell;
use std::rc::Rc;

use crate::controls::listview::draw::ListViewInner;
use crate::gdi::{Brush, Canvas, TextFormat};
use crate::geometry::Rect;
use crate::hwnd::Hwnd;
use crate::sys;

// `CDDS_*` stage codes, from `commctrl.h`.
const CDDS_PREPAINT: u32 = 0x0000_0001;
const CDDS_ITEMPREPAINT: u32 = 0x0001_0001;

/// Paints the list view's header in the app's colours.
pub(crate) struct HeaderDrawer<T> {
    view: Hwnd,
    inner: Rc<RefCell<ListViewInner<T>>>,
}

impl<T> HeaderDrawer<T> {
    pub(crate) fn new(view: Hwnd, inner: Rc<RefCell<ListViewInner<T>>>) -> HeaderDrawer<T> {
        HeaderDrawer { view, inner }
    }
}

impl<T> sys::listview_header::HeaderPainter for HeaderDrawer<T> {
    fn draw_header(&self, draw: &sys::listview_header::HeaderDraw) -> Option<isize> {
        let inner = self.inner.borrow();
        if draw.stage == CDDS_PREPAINT {
            Canvas::new(draw.hdc).fill_rect(draw.rect, inner.theme.header_background);
            // Ask for a notification per header item.
            return Some(32);
        }
        if draw.stage != CDDS_ITEMPREPAINT {
            return None;
        }

        let canvas = Canvas::new(draw.hdc);
        canvas.fill_rect(draw.rect, inner.theme.header_background);
        let item = draw.item.max(0) as usize;

        if let Some(column) = inner.columns.get(item) {
            let format = if column.align_right {
                TextFormat::left().right()
            } else {
                TextFormat::left()
            };
            let text_rect = Rect::new(
                draw.rect.left + 6,
                draw.rect.top,
                draw.rect.right - 6,
                draw.rect.bottom,
            );
            canvas.with_font(&inner.font, |canvas| {
                canvas.draw_text(
                    text_rect,
                    &column.title,
                    inner.theme.header_text,
                    format.single_line().vcenter().no_prefix(),
                );
            });
        }

        if let Ok(brush) = Brush::solid(inner.theme.border)
            && item > 0
        {
            canvas.fill_rect_brush(
                Rect::new(
                    draw.rect.left,
                    draw.rect.top,
                    draw.rect.left + 1,
                    draw.rect.bottom,
                ),
                &brush,
            );
        }

        if let Some((sort_column, ascending)) = inner.sort
            && sort_column == item
        {
            let size = 4;
            let middle = (draw.rect.top + draw.rect.bottom) / 2;
            let arrow = Rect::new(
                draw.rect.right - 16,
                middle - size,
                draw.rect.right - 8,
                middle + size,
            );
            canvas.triangle(arrow, inner.theme.header_text, ascending);
        }

        // We painted the whole item.
        Some(4)
    }

    fn allow_resize(&self, item: i32) -> bool {
        if item < 0 {
            return true;
        }
        self.inner
            .borrow()
            .columns
            .get(item as usize)
            .map(|column| column.resizable)
            .unwrap_or(true)
    }

    fn end_track(&self, item: i32) {
        // The drag is applied; honour the dragged column's new width, then give
        // the freed or claimed space to the `Fill` columns, if any.
        if let Ok(mut inner) = self.inner.try_borrow_mut() {
            inner.end_track(self.view, item);
        }
    }
}
