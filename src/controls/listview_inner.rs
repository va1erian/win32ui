#![forbid(unsafe_code)]

//! The owner-draw plumbing behind [`ListView`](super::ListView): the virtual
//! (owner-data) cell source and the dark header painting.

use std::cell::RefCell;
use std::rc::Rc;

use windows::Win32::UI::Controls::{LVN_GETDISPINFO, NM_CUSTOMDRAW};

use crate::controls::listview::{Column, ListSource};
use crate::controls::listview_theme::ListViewTheme;
use crate::controls::registry::{ControlEvents, ControlKind};
use crate::gdi::{Brush, Canvas, Font, TextFormat};
use crate::geometry::Rect;
use crate::hwnd::Hwnd;
use crate::sys;

const CDDS_PREPAINT: u32 = 0x0000_0001;
const CDDS_ITEMPREPAINT: u32 = 0x0001_0001;

pub(crate) struct ListViewInner {
    pub(crate) source: Box<dyn ListSource>,
    pub(crate) theme: ListViewTheme,
    pub(crate) columns: Vec<Column>,
    pub(crate) font: Font,
    pub(crate) playing: Option<usize>,
    /// `(column, ascending)` for the header sort arrow.
    pub(crate) sort: Option<(usize, bool)>,
}

impl ControlEvents for ListViewInner {
    fn kind(&self) -> ControlKind {
        ControlKind::ListView
    }

    fn on_notification(
        &mut self,
        hwnd: Hwnd,
        code: u32,
        _wparam: usize,
        lparam: isize,
    ) -> Option<isize> {
        if code == LVN_GETDISPINFO {
            let text = sys::control::lv_disp_info(lparam, |item, sub| self.cell(item, sub));
            return Some(text);
        }
        if code == NM_CUSTOMDRAW {
            return Some(sys::control::lv_custom_draw(lparam, |ctx| {
                self.custom_draw(hwnd, ctx)
            }));
        }
        None
    }
}

impl ListViewInner {
    fn cell(&self, item: i32, column: i32) -> String {
        if item < 0 || column < 0 {
            return String::new();
        }
        self.source.text(item as usize, column as usize)
    }

    fn custom_draw(
        &self,
        hwnd: Hwnd,
        ctx: &sys::control::CustomDraw,
    ) -> sys::control::CustomDrawResult {
        if ctx.stage == CDDS_PREPAINT {
            return sys::control::CustomDrawResult::NotifyItemDraw;
        }
        if ctx.stage != CDDS_ITEMPREPAINT || ctx.item < 0 {
            return sys::control::CustomDrawResult::Default;
        }

        let item = ctx.item;
        let row = sys::control::lv_subitem_rect(hwnd, item, 0);
        if row.is_empty() {
            return sys::control::CustomDrawResult::SkipDefault;
        }

        let selected = sys::control::lv_is_selected(hwnd, item);
        let playing = self.playing == Some(item as usize);
        let highlight = playing || selected;
        let background = if highlight {
            self.theme.selection
        } else if item % 2 == 1 {
            self.theme.alternate
        } else {
            self.theme.background
        };
        let text_color = if highlight {
            self.theme.on_playing
        } else {
            self.theme.text
        };

        // Paint the row ourselves: this is a real owner-drawn list, which also
        // lets us suppress the system's (focus-dependent) selection colour.
        let canvas = Canvas::new(ctx.hdc);
        canvas.fill_rect(row, background);
        canvas.with_font(&self.font, |canvas| {
            for (column, spec) in self.columns.iter().enumerate() {
                let cell = sys::control::lv_subitem_rect(hwnd, item, column as i32);
                if cell.is_empty() {
                    continue;
                }
                let text = self.source.text(item as usize, column);
                let format = if spec.align_right {
                    TextFormat::left().right()
                } else {
                    TextFormat::left()
                };
                let text_rect = Rect::new(cell.left + 4, cell.top, cell.right - 4, cell.bottom);
                canvas.draw_text(
                    text_rect,
                    &text,
                    text_color,
                    format.single_line().vcenter().end_ellipsis().no_prefix(),
                );
            }
        });

        // Thin vertical separators between columns.
        if let Ok(brush) = Brush::solid(self.theme.border) {
            for column in 1..self.columns.len() {
                let cell = sys::control::lv_subitem_rect(hwnd, item, column as i32);
                if cell.height() > 0 && cell.left > 0 {
                    canvas.fill_rect_brush(
                        Rect::new(cell.left, cell.top, cell.left + 1, cell.bottom),
                        &brush,
                    );
                }
            }
        }

        sys::control::CustomDrawResult::SkipDefault
    }
}

/// Paints the list view's header in the app's colours.
pub(crate) struct HeaderDrawer {
    inner: Rc<RefCell<ListViewInner>>,
}

impl HeaderDrawer {
    pub(crate) fn new(inner: Rc<RefCell<ListViewInner>>) -> HeaderDrawer {
        HeaderDrawer { inner }
    }
}

impl sys::control::HeaderPainter for HeaderDrawer {
    fn draw_header(&self, draw: &sys::control::HeaderDraw) -> Option<isize> {
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
}
