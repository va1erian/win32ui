#![forbid(unsafe_code)]

//! The owner-draw plumbing behind [`ListView`](super::ListView): the virtual
//! (owner-data) cell source and the row custom draw.

use windows::Win32::UI::Controls::{LVN_GETDISPINFO, NM_CUSTOMDRAW};

use crate::controls::listview::model::{Column, ListSource};
use crate::controls::listview::theme::ListViewTheme;
use crate::controls::registry::{ControlEvents, ControlKind};
use crate::gdi::{Brush, Canvas, Font, TextFormat};
use crate::geometry::Rect;
use crate::hwnd::Hwnd;
use crate::sys;

// `CDDS_*` stage codes, from `commctrl.h`.
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
            let text = sys::listview::lv_disp_info(lparam, |item, sub| self.cell(item, sub));
            return Some(text);
        }
        if code == NM_CUSTOMDRAW {
            return Some(sys::listview::lv_custom_draw(lparam, |ctx| {
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
        ctx: &sys::listview::CustomDraw,
    ) -> sys::listview::CustomDrawResult {
        if ctx.stage == CDDS_PREPAINT {
            return sys::listview::CustomDrawResult::NotifyItemDraw;
        }
        if ctx.stage != CDDS_ITEMPREPAINT || ctx.item < 0 {
            return sys::listview::CustomDrawResult::Default;
        }

        let item = ctx.item;
        let row = sys::listview::lv_subitem_rect(hwnd, item, 0);
        if row.is_empty() {
            return sys::listview::CustomDrawResult::SkipDefault;
        }

        let selected = sys::listview::lv_is_selected(hwnd, item);
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
                let cell = sys::listview::lv_subitem_rect(hwnd, item, column as i32);
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
                let cell = sys::listview::lv_subitem_rect(hwnd, item, column as i32);
                if cell.height() > 0 && cell.left > 0 {
                    canvas.fill_rect_brush(
                        Rect::new(cell.left, cell.top, cell.left + 1, cell.bottom),
                        &brush,
                    );
                }
            }
        }

        sys::listview::CustomDrawResult::SkipDefault
    }
}
