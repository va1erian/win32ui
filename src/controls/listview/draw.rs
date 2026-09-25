#![forbid(unsafe_code)]

//! The owner-draw plumbing behind [`ListView`](super::ListView): the virtual
//! (owner-data) cell source and the row custom draw.
//!
//! Both hooks borrow from the model for the duration of the call — cell text
//! is a `&str` out of the row — so scrolling and repainting allocate nothing
//! per cell.

use std::cell::RefCell;
use std::rc::Rc;

use windows::Win32::UI::Controls::{HIMAGELIST, LVN_GETDISPINFO, NM_CUSTOMDRAW};

use crate::controls::listview::model::{Column, ColumnWidth, ListModel};
use crate::controls::listview::row_style::{RowState, RowStyle};
use crate::controls::listview::theme::ListViewTheme;
use crate::controls::registry::{ControlEvents, ControlKind};
use crate::gdi::{Brush, Canvas, Font, FontWeight, TextFormat};
use crate::geometry::Rect;
use crate::hwnd::Hwnd;
use crate::sys;
use crate::units::{Dip, Px};

// `CDDS_*` stage codes, from `commctrl.h`.
const CDDS_PREPAINT: u32 = 0x0000_0001;
const CDDS_ITEMPREPAINT: u32 = 0x0001_0001;

/// Width, in device pixels, of the [`RowStyle::accent_bar`].
const ACCENT_BAR_WIDTH: i32 = 3;

pub(crate) type RowStyleFn<T> = Box<dyn Fn(&T) -> RowStyle>;
pub(crate) type RowPainterFn<T> = Box<dyn Fn(&T, &Canvas, Rect, RowState) -> bool>;

/// The face and point size the rows are painted with, kept so the fonts can be
/// rebuilt at a new DPI. `None` means the system UI font (the default).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RowFontSpec {
    pub(crate) family: String,
    pub(crate) points: f32,
}

pub(crate) struct ListViewInner<T> {
    pub(crate) model: Option<Box<dyn ListModel<Item = T>>>,
    pub(crate) theme: ListViewTheme,
    pub(crate) columns: Vec<Column<T>>,
    pub(crate) font: Font,
    /// The bold variant of `font`, used for [`RowStyle::bold`] rows. Created
    /// at construction and rebuilt on a DPI change, never per paint.
    pub(crate) bold_font: Font,
    /// The app's row-font spec (see [`ListView::set_row_font`](
    /// super::ListView::set_row_font)); `None` uses the system UI font.
    pub(crate) font_spec: Option<RowFontSpec>,
    pub(crate) row_style: Option<RowStyleFn<T>>,
    pub(crate) row_painter: Option<RowPainterFn<T>>,
    /// The app's requested row height, kept so it can be reconverted to
    /// pixels on a DPI change.
    pub(crate) row_height: Option<Dip>,
    /// The image list backing [`row_height`](Self::row_height) (see
    /// `sys::listview::lv_set_row_height`); destroyed in `Drop`.
    pub(crate) row_image_list: Option<HIMAGELIST>,
    /// `(column, ascending)` for the header sort arrow.
    pub(crate) sort: Option<(usize, bool)>,
    pub(crate) dpi: u32,
    /// The selection last reported to the app; notifications that leave it
    /// unchanged emit nothing.
    pub(crate) last_selection: Vec<usize>,
    /// While set, selection notifications are swallowed: a programmatic
    /// change reports its own single event instead.
    pub(crate) selection_muted: bool,
}

impl<T: 'static> ControlEvents for ListViewInner<T> {
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
            let text = sys::listview::lv_disp_info_str(lparam, |item, sub| self.cell(item, sub));
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

impl<T> ListViewInner<T> {
    /// The cell's text borrowed from the row, or `None` for an empty cell.
    fn cell(&self, item: i32, column: i32) -> Option<&str> {
        if item < 0 || column < 0 {
            return None;
        }
        let row = self.model.as_ref()?.get(item as usize)?;
        let spec = self.columns.get(column as usize)?;
        Some((spec.text)(row))
    }

    /// Rebuilds the regular and bold row fonts at `dpi`, from the app's
    /// [`RowFontSpec`] when one was set or from the system UI font otherwise.
    /// New fonts are built before the old handles are replaced, so a paint in
    /// flight can never touch a deleted `HFONT`.
    pub(crate) fn rebuild_fonts(&mut self, dpi: u32) {
        let (regular, bold) = match &self.font_spec {
            Some(spec) => (
                Font::new(&spec.family, spec.points, FontWeight::Regular, dpi),
                Font::new(&spec.family, spec.points, FontWeight::Bold, dpi),
            ),
            None => (
                Font::system_ui(dpi),
                Font::system_ui_weight(dpi, FontWeight::Bold),
            ),
        };
        if let Ok(regular) = regular {
            self.font = regular;
        }
        if let Ok(bold) = bold {
            self.bold_font = bold;
        }
    }

    /// Honours the width the user just dragged column `item` to: a `Fill`
    /// column stops sharing and becomes a fixed width, so it never snaps back;
    /// the remaining `Fill` columns then take the leftover space.
    pub(crate) fn end_track(&mut self, view: Hwnd, item: i32) {
        if item >= 0 {
            let index = item as usize;
            if let Some(column) = self.columns.get_mut(index)
                && column.width == ColumnWidth::Fill
            {
                let px = sys::listview::lv_column_width(view, index).max(0);
                column.width = ColumnWidth::Fixed(Px(px).to_dip(self.dpi));
            }
        }
        self.restretch(view);
    }

    /// Stretches the `Fill` columns over whatever client width the fixed
    /// columns leave behind, sharing it evenly. Fixed columns keep their
    /// current width, so a fixed column the user just resized stays put; a
    /// `Fill` column the user resized is turned fixed first by
    /// [`Self::end_track`].
    pub(crate) fn restretch(&self, view: Hwnd) {
        let fills = self
            .columns
            .iter()
            .filter(|column| column.width == ColumnWidth::Fill)
            .count();
        if fills == 0 {
            return;
        }
        let client = sys::window::client_rect(view).width();
        let fixed: i32 = self
            .columns
            .iter()
            .enumerate()
            .filter(|(_, column)| column.width != ColumnWidth::Fill)
            .map(|(index, _)| sys::listview::lv_column_width(view, index))
            .sum();
        // A `Fill` column never collapses to zero under a narrow window; the
        // control scrolls horizontally instead.
        let minimum = Dip::new(48.0).to_px(self.dpi).value();
        let each = ((client - fixed) / fills as i32).max(minimum);
        for (index, column) in self.columns.iter().enumerate() {
            if column.width == ColumnWidth::Fill {
                sys::listview::lv_set_column_width(view, index, each);
            }
        }
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
        let focused = sys::listview::lv_has_focus(hwnd);
        let state = RowState {
            selected,
            focused,
            hot: ctx.hot,
            alternate: item % 2 == 1,
        };
        let canvas = Canvas::new(ctx.hdc);

        if let Some(painter) = &self.row_painter
            && let Some(data_row) = self
                .model
                .as_ref()
                .and_then(|model| model.get(item as usize))
            && painter(data_row, &canvas, row, state)
        {
            return sys::listview::CustomDrawResult::SkipDefault;
        }

        let data_row = self
            .model
            .as_ref()
            .and_then(|model| model.get(item as usize));
        let style = data_row.and_then(|data_row| self.row_style.as_ref().map(|f| f(data_row)));

        // Explorer keeps the normal text colour on selection and only swaps
        // the background (focused blue, unfocused grey). See
        // `ListViewTheme::row_colors`; `style` then overrides on top.
        let (mut background, mut text_color) =
            self.theme.row_colors(selected, focused, state.alternate);
        if let Some(style) = &style {
            if let Some(color) = style.background {
                background = color;
            }
            if let Some(color) = style.text {
                text_color = color;
            }
        }
        let bold = style.as_ref().is_some_and(|style| style.bold);
        let font = if bold { &self.bold_font } else { &self.font };

        // Paint the row ourselves: this is a real owner-drawn list, which also
        // lets us pick the focused/unfocused selection background ourselves
        // instead of taking the system's focus-dependent default.
        canvas.fill_rect(row, background);
        canvas.with_font(font, |canvas| {
            for (column, spec) in self.columns.iter().enumerate() {
                let cell = sys::listview::lv_cell_rect(hwnd, item, column as i32);
                if cell.is_empty() {
                    continue;
                }
                let text = self.cell(item, column as i32).unwrap_or("");
                let cell_color = spec
                    .color
                    .as_ref()
                    .and_then(|color| data_row.and_then(|row| color(row, &self.theme)))
                    .unwrap_or(text_color);
                let format = if spec.align_right {
                    TextFormat::left().right()
                } else if spec.centered {
                    TextFormat::left().center()
                } else {
                    TextFormat::left()
                };
                let text_rect = Rect::new(cell.left + 4, cell.top, cell.right - 4, cell.bottom);
                canvas.draw_text(
                    text_rect,
                    text,
                    cell_color,
                    format.single_line().vcenter().end_ellipsis().no_prefix(),
                );
            }
        });

        if let Some(color) = style.as_ref().and_then(|style| style.accent_bar) {
            canvas.fill_rect(
                Rect::new(row.left, row.top, row.left + ACCENT_BAR_WIDTH, row.bottom),
                color,
            );
        }

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

/// Restretches a list view's `Fill` columns whenever its client size changes.
pub(crate) struct StretchHandler<T> {
    pub(crate) view: Hwnd,
    pub(crate) inner: Rc<RefCell<ListViewInner<T>>>,
}

impl<T> sys::listview_header::SizeHandler for StretchHandler<T> {
    fn on_size(&self) {
        // A resize never nests inside another borrow of the same state, but a
        // `try_borrow` keeps a surprise nesting from panicking across the
        // subclass boundary; the next resize then repairs the widths.
        if let Ok(inner) = self.inner.try_borrow() {
            inner.restretch(self.view);
        }
    }

    fn on_dpi_changed(&self, dpi: u32) {
        // Same `try_borrow` caution as `on_size`; a missed DPI change is
        // repaired by the next resize or theme change, never a panic.
        //
        // The state is never borrowed mutably across a native call: the list
        // view repaints its header synchronously while the row height and the
        // column widths change, the header's custom draw needs the state, and
        // a failed borrow there makes the header fall back to its default
        // (light) drawing for the columns painted in that window.
        let (row_height, previous, previous_dpi, scalable) = {
            let Ok(mut inner) = self.inner.try_borrow_mut() else {
                return;
            };
            let previous_dpi = inner.dpi;
            inner.dpi = dpi;
            // The row text is painted with these, so they follow the DPI too,
            // honouring an app-set row font when there is one.
            inner.rebuild_fonts(dpi);
            let scalable: Vec<usize> = inner
                .columns
                .iter()
                .enumerate()
                .filter(|(_, column)| column.width != ColumnWidth::Fill)
                .map(|(index, _)| index)
                .collect();
            (
                inner.row_height,
                inner.row_image_list.take(),
                previous_dpi,
                scalable,
            )
        };
        // Column widths are device pixels the control never rescales; scale the
        // non-`Fill` ones (keeping any user resize) so the `Fill` columns
        // restretch against the right remainder.
        if previous_dpi != dpi && previous_dpi != 0 {
            for index in scalable {
                let width = sys::listview::lv_column_width(self.view, index);
                let scaled = (i64::from(width) * i64::from(dpi) / i64::from(previous_dpi)) as i32;
                sys::listview::lv_set_column_width(self.view, index, scaled);
            }
        }
        let list = match row_height {
            Some(height) => {
                sys::listview::lv_set_row_height(self.view, height.to_px(dpi).value(), previous)
            }
            None => previous,
        };
        if let Ok(mut inner) = self.inner.try_borrow_mut() {
            inner.row_image_list = list;
        }
        if let Ok(inner) = self.inner.try_borrow() {
            inner.restretch(self.view);
        }
        // Whatever the header painted mid-change, repaint it in the app's colours.
        sys::window::invalidate(sys::listview::lv_header(self.view));
    }
}
