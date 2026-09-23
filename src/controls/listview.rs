#![forbid(unsafe_code)]

//! A virtual, owner-drawn report [`ListView`].
//!
//! The control is created with `LVS_OWNERDATA`, so it never stores the rows
//! itself: cell text is requested lazily through [`ListSource`], and row
//! colours/backgrounds are supplied through `NM_CUSTOMDRAW`. Both hooks are
//! handled inside [`ListViewInner`] and never reach the application. Only the
//! meaningful events surface, as [`ListViewEvent`]s.

use std::cell::RefCell;
use std::rc::Rc;

use windows::Win32::UI::Controls::{LVN_GETDISPINFO, NM_CUSTOMDRAW};

use crate::controls::registry::{self, ControlEvents, ControlKind};
use crate::controls::{create_child, style};
use crate::error::Result;
use crate::gdi::{Brush, Canvas, Font, TextFormat};
use crate::geometry::Rect;
use crate::hwnd::Hwnd;
use crate::sys;
use crate::theme::Theme;
use crate::window::dpi_scale;

const LVS_REPORT: u32 = 0x0000_0001;
const LVS_SHOWSELALWAYS: u32 = 0x0000_0008;
const LVS_OWNERDATA: u32 = 0x0000_1000;
const LVS_EX_FULLROWSELECT: u32 = 0x0000_0020;
const LVS_EX_DOUBLEBUFFER: u32 = 0x0001_0000;

const CDDS_PREPAINT: u32 = 0x0000_0001;
const CDDS_ITEMPREPAINT: u32 = 0x0001_0001;

/// A report-mode column.
#[derive(Clone, Debug)]
pub struct Column {
    /// Header label.
    pub title: String,
    /// Initial width at 96 DPI.
    pub width: i32,
    /// Whether the column's cells are right-aligned.
    pub align_right: bool,
}

impl Column {
    /// A left-aligned column.
    pub fn new(title: impl Into<String>, width: i32) -> Column {
        Column {
            title: title.into(),
            width,
            align_right: false,
        }
    }

    /// A right-aligned column (numbers, durations).
    pub fn right(title: impl Into<String>, width: i32) -> Column {
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
    },
}

struct ListViewInner {
    source: Box<dyn ListSource>,
    theme: ListViewTheme,
    columns: Vec<Column>,
    font: Font,
    playing: Option<usize>,
    /// `(column, ascending)` for the header sort arrow.
    sort: Option<(usize, bool)>,
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
struct HeaderDrawer {
    inner: Rc<RefCell<ListViewInner>>,
}

impl sys::control::HeaderPainter for HeaderDrawer {
    fn draw_header(&self, draw: &sys::control::HeaderDraw) -> Option<isize> {
        const CDDS_PREPAINT: u32 = 0x0000_0001;
        const CDDS_ITEMPREPAINT: u32 = 0x0001_0001;

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

/// A virtual report list view.
pub struct ListView {
    hwnd: Hwnd,
    header: Hwnd,
    inner: Rc<RefCell<ListViewInner>>,
    header_subclass: Option<sys::control::HeaderSubclass>,
}

impl ListView {
    /// Creates the control as a child of `parent`.
    pub fn new(
        parent: Hwnd,
        id: usize,
        bounds: Rect,
        columns: &[Column],
        source: Box<dyn ListSource>,
        theme: ListViewTheme,
        dpi: u32,
    ) -> Result<ListView> {
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
            parent,
            style,
            style::WS_EX_CLIENTEDGE,
            id,
            bounds,
        )?;

        sys::control::lv_set_extended_style(hwnd, LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER);
        sys::control::lv_set_colors(hwnd, theme.background, theme.text);
        for (index, column) in columns.iter().enumerate() {
            sys::control::lv_insert_column(
                hwnd,
                index as i32,
                &column.title,
                dpi_scale(column.width, dpi),
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
        let events: Rc<RefCell<dyn ControlEvents>> = inner.clone();
        registry::register(hwnd, events);

        let header_subclass = if header.is_null() {
            None
        } else {
            sys::control::HeaderSubclass::install(
                hwnd,
                header,
                Box::new(HeaderDrawer {
                    inner: Rc::clone(&inner),
                }),
            )
        };

        Ok(ListView {
            hwnd,
            header,
            inner,
            header_subclass,
        })
    }

    /// The control handle.
    pub fn hwnd(&self) -> Hwnd {
        self.hwnd
    }

    /// Moves/resizes the control.
    pub fn set_bounds(&self, bounds: Rect) {
        sys::window::move_window(self.hwnd, bounds);
    }

    /// Updates the number of virtual rows after the source changed.
    pub fn set_item_count(&self, count: usize) {
        sys::control::lv_set_item_count(self.hwnd, count);
    }

    /// Replaces the data source and refreshes the view.
    pub fn set_source(&self, source: Box<dyn ListSource>) {
        self.inner.borrow_mut().source = source;
        sys::control::lv_set_item_count(self.hwnd, self.inner.borrow().source.item_count());
        sys::window::invalidate(self.hwnd);
    }

    /// Marks `row` as the now-playing row (highlighted during custom draw).
    pub fn set_playing(&self, row: Option<usize>) {
        self.inner.borrow_mut().playing = row;
        sys::window::invalidate(self.hwnd);
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
        sys::control::lv_selected(self.hwnd).map(|index| index as usize)
    }

    /// The control's background colour.
    pub fn background_color(&self) -> crate::Color {
        sys::control::lv_background(self.hwnd)
    }

    /// Reads back a cell's text (which re-enters the owner-data path). Useful
    /// for tests and for accessibility.
    pub fn cell_text(&self, item: usize, column: usize) -> String {
        sys::control::lv_item_text(self.hwnd, item as i32, column as i32)
    }

    /// Selects and focuses `row`.
    pub fn select(&self, row: usize) {
        sys::control::lv_select(self.hwnd, row as i32);
    }
}

impl Drop for ListView {
    fn drop(&mut self) {
        // Remove the header subclass before the window (and its header) go away.
        self.header_subclass = None;
        registry::unregister(self.hwnd);
        sys::window::destroy(self.hwnd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controls::listview::ListViewTheme;
    use crate::geometry::Rect;
    use crate::message::{LResult, Message};
    use crate::theme::Theme;
    use crate::window::{Window, WindowClass, WindowExStyle, WindowHandler, WindowStyle};

    struct NullHandler;

    impl WindowHandler for NullHandler {
        fn message(&mut self, _window: &Window, _message: Message) -> Option<LResult> {
            None
        }
    }

    struct Empty;

    impl ListSource for Empty {
        fn item_count(&self) -> usize {
            0
        }

        fn text(&self, _item: usize, _column: usize) -> String {
            String::new()
        }
    }

    /// Assorted text that has historically tripped up the ANSI/UTF-16 boundary:
    /// CJK, emoji beyond the BMP, combining marks, RTL, flags and a long run.
    const WEIRD: &[&str] = &[
        "日本語のアルバム",
        "🎵 Émoji 🎶 𝄞",
        "Ω≈ç√∫˜µ≤≥÷",
        "العربية − Ελληνικά − עברית",
        "e\u{301}\u{327} combining",
        "𝔘𝔫𝔦𝔠𝔬𝔡𝔢 𝕗𝕒𝕟𝕔𝕪",
        "🇫🇷🇯🇵 flags",
        "NUL-free\u{200b}zero-width",
    ];

    struct UnicodeSource {
        long: String,
    }

    impl ListSource for UnicodeSource {
        fn item_count(&self) -> usize {
            WEIRD.len() + 1
        }

        fn text(&self, item: usize, column: usize) -> String {
            if item == WEIRD.len() {
                return if column == 0 {
                    self.long.clone()
                } else {
                    String::new()
                };
            }
            if column == 0 {
                WEIRD[item].to_string()
            } else {
                format!("{} / {column}", WEIRD[item])
            }
        }
    }

    fn make_list(source: Box<dyn ListSource>) -> Option<(Window, ListView)> {
        let theme = Theme::dark();
        let class = WindowClass::register("win32ui.unicodetest", theme.background).ok()?;
        let window = Window::create(
            class,
            None,
            WindowStyle::overlapped(),
            WindowExStyle::new(),
            Rect::new(0, 0, 800, 600),
            "unicode test",
            NullHandler,
        )
        .ok()?;
        let list = ListView::new(
            window.hwnd(),
            1,
            Rect::new(0, 0, 700, 500),
            &[Column::new("Title", 300), Column::new("Artist", 200)],
            source,
            ListViewTheme::from_theme(&theme),
            96,
        )
        .ok()?;
        Some((window, list))
    }

    #[test]
    fn unicode_cell_text_round_trips() {
        let long = "長".repeat(400);
        let Some((window, list)) = make_list(Box::new(UnicodeSource { long: long.clone() })) else {
            return;
        };
        for (index, expected) in WEIRD.iter().enumerate() {
            assert_eq!(&list.cell_text(index, 0), expected, "row {index}");
        }
        assert_eq!(list.cell_text(0, 1), format!("{} / 1", WEIRD[0]));
        assert_eq!(list.cell_text(WEIRD.len(), 0), long);
        window.destroy();
    }

    #[test]
    fn background_colour_is_applied() {
        let theme = Theme::dark();
        let Ok(class) = WindowClass::register("win32ui.listtest", theme.background) else {
            return;
        };
        let Ok(window) = Window::create(
            class,
            None,
            WindowStyle::overlapped(),
            WindowExStyle::new(),
            Rect::new(0, 0, 400, 300),
            "list test",
            NullHandler,
        ) else {
            return;
        };
        let Ok(list) = ListView::new(
            window.hwnd(),
            1,
            Rect::new(0, 0, 200, 200),
            &[Column::new("A", 80)],
            Box::new(Empty),
            ListViewTheme::from_theme(&theme),
            96,
        ) else {
            return;
        };
        assert_eq!(list.background_color(), theme.background);
        window.destroy();
    }
}
