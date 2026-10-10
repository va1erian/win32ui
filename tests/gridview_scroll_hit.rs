//! Scrolled `GridView` hit-testing: a mouse click's client coordinates must
//! map onto the content the scroll host drew at the offset, so the click lands
//! on the tile under the cursor rather than one higher every scroll stride.
//!
//! Regression: `GridWidget::input` hit-tested the client-space `y` without
//! adding the scroll host's offset, so the selection climbed one row per
//! scroll stride as the grid scrolled down.

#![cfg(windows)]

mod common;

use std::cell::Cell;
use std::ffi::c_void;
use std::rc::Rc;

use common::run_app_with_watchdog;
use win32ui::prelude::*;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    GetScrollInfo, SB_VERT, SCROLLINFO, SIF_PAGE, SIF_POS, SendMessageW, WM_VSCROLL,
};

/// Enough rows to scroll deep past the first page.
const ROWS: usize = 40;
/// `SB_BOTTOM`, from `winuser.h`.
const SB_BOTTOM: u32 = 7;
/// `WM_LBUTTONDOWN`, from `winuser.h`.
const WM_LBUTTONDOWN: u32 = 0x0201;

struct Tile(u32);

#[derive(Clone, Copy)]
enum Msg {
    ScrollAndClick,
    /// The click selected this index.
    Selected(usize),
}

struct Harness {
    grid: GridView<Tile, Msg>,
    selected: Rc<Cell<Option<usize>>>,
}

impl App for Harness {
    type Msg = Msg;

    fn update(&mut self, msg: Msg, ui: &mut Ui<Msg>) {
        match msg {
            Msg::ScrollAndClick => {
                let hwnd = HWND(self.grid.hwnd().raw() as *mut c_void);
                // SAFETY: `hwnd` is a live scrollbar-bearing window; the
                // message takes no pointers.
                unsafe {
                    let _ = SendMessageW(hwnd, WM_VSCROLL, Some(WPARAM(SB_BOTTOM as usize)), None);
                }
                let (offset, page) = scroll_pos_page(self.grid.hwnd());
                let tile = dip(80.0).to_px(ui.dpi()).value();
                let spacing = dip(8.0).to_px(ui.dpi()).value();
                let stride = tile + spacing;
                let last_row = ROWS as i32 - 1;
                // The content-space centre of the last row's tile, mapped back
                // into client space by the offset the host is scrolled to.
                let content_y = last_row * stride + tile / 2;
                let click_y = content_y - offset;
                assert!(
                    click_y >= 0 && click_y < page,
                    "the last row should be on the bottom page: click_y {click_y} page {page}"
                );
                let click_x = tile / 2; // inside the first column's tile
                let lparam =
                    LPARAM((((click_y as i16) as isize) << 16) | (click_x as i16 as isize));
                // SAFETY: `hwnd` is a live window; the message takes no pointers.
                unsafe {
                    let _ = SendMessageW(hwnd, WM_LBUTTONDOWN, None, Some(lparam));
                }
            }
            Msg::Selected(index) => {
                let got = self.selected.replace(Some(index));
                let _ = got;
                ui.quit();
            }
        }
    }
}

/// `(position, page)` of `hwnd`'s vertical scroll bar, in device pixels.
fn scroll_pos_page(hwnd: Hwnd) -> (i32, i32) {
    let mut info = SCROLLINFO {
        cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
        fMask: SIF_PAGE | SIF_POS,
        ..Default::default()
    };
    // SAFETY: `hwnd` is a live scrollbar-bearing window; `info` matches `cbSize`.
    unsafe {
        let _ = GetScrollInfo(HWND(hwnd.raw() as *mut c_void), SB_VERT, &mut info);
    }
    (info.nPos, info.nPage as i32)
}

/// Scrolled to the bottom, a click on the last visible row selects that row —
/// not a row a page above it.
#[test]
fn scrolled_click_hits_the_tile_under_the_cursor() {
    let selected = Rc::new(Cell::new(None));
    let selected_for_make = Rc::clone(&selected);

    let Some(run) = run_app_with_watchdog("win32ui.grid_view.scroll-hit", move |ui| {
        let grid = GridView::<Tile, Msg>::new(ui)
            .expect("grid")
            .tile_size(dip(80.0))
            .content(|tile: &Tile, _canvas, _rect, _state| {
                let _ = tile.0;
            })
            .on_select(|index| Some(Msg::Selected(index)));
        grid.set_model((0..ROWS).map(|n| Tile(n as u32)).collect::<Vec<_>>());
        // One column: narrower than one tile+spacing stride, so the row under
        // the click is exactly one index regardless of the host's DPI.
        grid.set_bounds(Rect::new(0, 0, 100, 300));
        ui.emit(Msg::ScrollAndClick);
        Harness {
            grid,
            selected: selected_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    let selected = selected.get();
    assert_eq!(
        selected,
        Some(ROWS - 1),
        "the click should have selected the bottom row"
    );
}
