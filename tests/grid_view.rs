//! `GridView` behaviour that needs a real window: the model/selection round
//! trip and tile-size clamping. The virtualization-range and keyboard
//! wrap-around arithmetic are pure functions, unit-tested alongside them in
//! `src/controls/grid_view/layout.rs`.

#![cfg(windows)]

mod common;

use std::cell::Cell;
use std::ffi::c_void;
use std::rc::Rc;
use std::time::Instant;

use common::run_app_with_watchdog;
use win32ui::prelude::*;
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{InvalidateRect, UpdateWindow};
use windows::Win32::UI::WindowsAndMessaging::{
    GW_CHILD, GetClientRect, GetScrollInfo, GetWindow, SB_LINEDOWN, SB_VERT, SCROLLINFO, SIF_POS,
    SIF_RANGE, SendMessageW, WM_VSCROLL,
};

struct Tile(u32);

enum Msg {
    Start,
}

struct GridApp;

impl App for GridApp {
    type Msg = Msg;

    fn update(&mut self, msg: Msg, ui: &mut Ui<Msg>) {
        let Msg::Start = msg;
        ui.quit();
    }
}

#[test]
fn selection_round_trips_and_clears_out_of_range() {
    let checks = Rc::new(Cell::new((None::<Option<usize>>, None::<Option<usize>>)));
    let checks_for_make = Rc::clone(&checks);

    let Some(run) = run_app_with_watchdog("win32ui.grid_view.selection", move |ui| {
        let grid = GridView::<Tile, Msg>::new(ui).expect("grid").content(
            |tile: &Tile, _canvas, _rect, _state| {
                let _ = tile.0;
            },
        );
        grid.set_model(vec![Tile(0), Tile(1), Tile(2)]);

        grid.set_selected(Some(1));
        let selected = grid.selected();

        grid.set_model(vec![Tile(0)]); // shrinks past the old selection
        let after_shrink = grid.selected();

        checks_for_make.set((Some(selected), Some(after_shrink)));
        ui.emit(Msg::Start);
        GridApp
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    let (selected, after_shrink) = checks.get();
    assert_eq!(selected, Some(Some(1)));
    assert_eq!(after_shrink, Some(None));
}

/// The grid's viewport scrollbar range (`SCROLLINFO.nMax`), read from the live
/// `HWND`, so the test can prove the extent followed a resize.
fn scroll_max(hwnd: Hwnd) -> i32 {
    let mut info = SCROLLINFO {
        cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
        fMask: SIF_RANGE,
        ..Default::default()
    };
    // SAFETY: `hwnd` is a live scrollbar-bearing window for the duration of the
    // call, and `info` is a valid `SCROLLINFO` with the matching `cbSize`.
    unsafe {
        let _ = GetScrollInfo(HWND(hwnd.raw() as *mut c_void), SB_VERT, &mut info);
    }
    info.nMax
}

/// Resizing the viewport must resize the scrollable extent through the resize
/// callback, with no intervening `set_model`/`set_tile_size`.
#[test]
fn resizing_the_viewport_resyncs_the_scroll_range() {
    let observed = Rc::new(Cell::new((None::<i32>, None::<i32>)));
    let observed_for_make = Rc::clone(&observed);

    let Some(run) = run_app_with_watchdog("win32ui.grid_view.resize", move |ui| {
        let grid = GridView::<Tile, Msg>::new(ui).expect("grid").content(
            |tile: &Tile, _canvas, _rect, _state| {
                let _ = tile.0;
            },
        );
        grid.set_model((0..60).map(|_| Tile(0)).collect::<Vec<_>>());

        // Wide viewport: five columns, twelve rows.
        grid.set_bounds(Rect::new(0, 0, 900, 200));
        let wide = scroll_max(grid.hwnd());

        // Narrow viewport: one column, sixty rows.
        grid.set_bounds(Rect::new(0, 0, 200, 200));
        let narrow = scroll_max(grid.hwnd());

        observed_for_make.set((Some(wide), Some(narrow)));
        ui.emit(Msg::Start);
        GridApp
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    let (wide, narrow) = observed.get();
    let (wide, narrow) = (wide.expect("wide range"), narrow.expect("narrow range"));
    assert!(
        narrow > wide,
        "a narrower viewport needs more rows, so the range should grow (wide {wide}, narrow {narrow})"
    );
}

/// Reads the viewport's current vertical scroll position, so the 30k-tile test
/// can invalidate the strip that is actually visible.
fn scroll_pos(hwnd: Hwnd) -> i32 {
    let mut info = SCROLLINFO {
        cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
        fMask: SIF_POS,
        ..Default::default()
    };
    // SAFETY: `hwnd` is live and `info` is a valid `SCROLLINFO` with matching
    // `cbSize`.
    unsafe {
        let _ = GetScrollInfo(HWND(hwnd.raw() as *mut c_void), SB_VERT, &mut info);
    }
    info.nPos
}

/// The 30k-tile scroll check from the issue: only the visible strip repaints,
/// so a synthetic scroll+paint frame stays well under a 16.7 ms (60 Hz) budget.
/// Ignored by default (timing on a shared CI box is noisy); run explicitly with
/// `cargo test --test grid_view -- --ignored --nocapture` and read the printed
/// p95.
#[test]
#[ignore = "timing-sensitive; run explicitly for the 30k-tile p95 check"]
fn scrolling_30k_tiles_keeps_p95_frame_time_low() {
    // A 32-bit window's height is a `WORD`, so the grid's single tall content
    // window caps the model at roughly `32767 / row-stride` rows; the test
    // widens the viewport to fit this many tiles underneath that cap.
    const TILES: usize = 30_000;
    const STEPS: usize = 200;
    const VIEWPORT_HEIGHT: i32 = 600;
    /// A deliberately loose ceiling, so the debug build passes while a real
    /// regression (painting off-screen tiles) fails loudly.
    const BUDGET_MS: f64 = 50.0;

    let observed = Rc::new(Cell::new(None::<(f64, f64)>));
    let observed_for_make = Rc::clone(&observed);

    let Some(run) = run_app_with_watchdog("win32ui.grid_view.30k", move |ui| {
        let grid = GridView::<Tile, Msg>::new(ui)
            .expect("grid")
            .tile_size(dip(20.0))
            .content(|tile: &Tile, canvas, rect, _state| {
                canvas.fill_rect(rect, Color::rgb((tile.0 & 0xFF) as u8, 0x40, 0x80));
            });
        grid.set_model(
            (0..TILES)
                .map(|index| Tile(index as u32))
                .collect::<Vec<_>>(),
        );
        let height = VIEWPORT_HEIGHT;
        // Pick enough columns that `TILES` tiles fit under the ~32767 px window
        // height cap (device-pixel arithmetic, so it holds at any DPI).
        let dpi = ui.dpi();
        let stride = grid.current_tile_size().to_px(dpi).value() + dip(8.0).to_px(dpi).value();
        let columns = (TILES * stride as usize).div_ceil(31_000).max(1);
        let width = columns as i32 * stride;
        grid.set_bounds(Rect::new(0, 0, width, height));

        let viewport = grid.hwnd();
        // SAFETY: `viewport` is live; `GW_CHILD` is a documented flag and the
        // content window is its only child.
        let content =
            unsafe { GetWindow(HWND(viewport.raw() as *mut c_void), GW_CHILD) }.unwrap_or_default();

        // The whole 30k-tile extent lives in one tall content window; prove the
        // OS accepted it rather than clamping to the viewport (a clamp would
        // make the "virtualized" test paint nothing but the first screenful).
        let mut client = RECT::default();
        // SAFETY: `content` is the live content window; `client` is a valid
        // out-parameter.
        let _ = unsafe { GetClientRect(content, &mut client) };
        assert!(
            client.bottom > 30_000,
            "the content window height was {} px; 30k tiles cannot virtualize",
            client.bottom
        );

        // Warm up: the first frames build the GDI brush cache and upload data.
        for _ in 0..3 {
            let _ = unsafe {
                SendMessageW(
                    HWND(viewport.raw() as *mut c_void),
                    WM_VSCROLL,
                    Some(WPARAM(SB_LINEDOWN.0 as usize)),
                    Some(LPARAM(0)),
                )
            };
        }

        let mut frames = Vec::with_capacity(STEPS);
        for _ in 0..STEPS {
            let start = Instant::now();
            // SAFETY: live handles; the strip is the visible part of the
            // content, so the paint exercises exactly one viewport of tiles.
            unsafe {
                let _ = SendMessageW(
                    HWND(viewport.raw() as *mut c_void),
                    WM_VSCROLL,
                    Some(WPARAM(SB_LINEDOWN.0 as usize)),
                    Some(LPARAM(0)),
                );
                let pos = scroll_pos(viewport);
                if !content.0.is_null() {
                    let strip = RECT {
                        left: 0,
                        top: pos,
                        right: width,
                        bottom: pos + height,
                    };
                    let _ = InvalidateRect(Some(content), Some(&strip), false);
                    let _ = UpdateWindow(content);
                }
            }
            frames.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        let max = frames.iter().copied().fold(0.0f64, f64::max);
        frames.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p95 = frames[(STEPS as f64 * 0.95) as usize];
        eprintln!("30k tiles: p95 frame {p95:.3} ms, max {max:.3} ms");
        observed_for_make.set(Some((p95, max)));

        ui.emit(Msg::Start);
        GridApp
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    let (p95, max) = observed.get().expect("p95 not recorded");
    assert!(
        p95 < BUDGET_MS,
        "p95 frame time {p95:.3} ms exceeds the {BUDGET_MS} ms budget (max {max:.3} ms)"
    );
}

#[test]
fn tile_size_clamps_to_the_given_range() {
    let observed = Rc::new(Cell::new(None::<f32>));
    let observed_for_make = Rc::clone(&observed);

    let Some(run) = run_app_with_watchdog("win32ui.grid_view.tile_size", move |ui| {
        let grid = GridView::<Tile, Msg>::new(ui)
            .expect("grid")
            .tile_size(dip(100.0)..dip(200.0));

        grid.set_tile_size(dip(1000.0)); // above the range
        observed_for_make.set(Some(grid.current_tile_size().value()));

        ui.emit(Msg::Start);
        GridApp
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    assert_eq!(observed.get(), Some(200.0));
}
