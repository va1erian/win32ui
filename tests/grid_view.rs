//! `GridView` behaviour that needs a real window: the model/selection round
//! trip and tile-size clamping. The virtualization-range and keyboard
//! wrap-around arithmetic are pure functions, unit-tested alongside them in
//! `src/controls/grid_view/layout.rs`.

#![cfg(windows)]

mod common;

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::rc::Rc;
use std::time::Instant;

use common::run_app_with_watchdog;
use win32ui::d2d::Interpolation;
use win32ui::prelude::*;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::UpdateWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    GetScrollInfo, SB_LINEDOWN, SB_PAGEDOWN, SB_VERT, SCROLLINFO, SIF_RANGE, SendMessageW,
    WM_VSCROLL,
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

/// The 30k-tile scroll check from the issue: only the visible tiles repaint,
/// so a synthetic scroll+paint frame stays well under a 16.7 ms (60 Hz) budget.
/// Ignored by default (timing on a shared CI box is noisy); run explicitly with
/// `cargo test --test grid_view -- --ignored --nocapture` and read the printed
/// p95.
#[test]
#[ignore = "timing-sensitive; run explicitly for the 30k-tile p95 check"]
fn scrolling_30k_tiles_keeps_p95_frame_time_low() {
    const TILES: usize = 30_000;
    const STEPS: usize = 200;
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
        // The widget is the viewport: a 30k-tile document lives entirely in the
        // scroll extent, never in a window taller than the screen.
        grid.set_bounds(Rect::new(0, 0, 900, 600));

        let viewport = grid.hwnd();

        // Warm up: the first frames build the GDI brush cache and upload data.
        for _ in 0..3 {
            // SAFETY: `viewport` is live; a plain line-down scroll message.
            unsafe {
                let _ = SendMessageW(
                    HWND(viewport.raw() as *mut c_void),
                    WM_VSCROLL,
                    Some(WPARAM(SB_LINEDOWN.0 as usize)),
                    Some(LPARAM(0)),
                );
            }
        }

        let mut frames = Vec::with_capacity(STEPS);
        for _ in 0..STEPS {
            let start = Instant::now();
            // SAFETY: `viewport` is live. `scroll_to_px` invalidates the widget,
            // so `UpdateWindow` paints exactly one viewport of tiles.
            unsafe {
                let _ = SendMessageW(
                    HWND(viewport.raw() as *mut c_void),
                    WM_VSCROLL,
                    Some(WPARAM(SB_LINEDOWN.0 as usize)),
                    Some(LPARAM(0)),
                );
                let _ = UpdateWindow(HWND(viewport.raw() as *mut c_void));
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

/// A Direct2D grid in the built-in scroll host must actually scroll: a page
/// down changes which tile indices the painter is called for (the canvas is
/// translated and only the new visible band is painted).
struct ScrollPaintApp {
    grid: GridView<Tile, Msg>,
    last: Rc<RefCell<Vec<usize>>>,
    first_min: Rc<Cell<Option<usize>>>,
    second_min: Rc<Cell<Option<usize>>>,
}

impl App for ScrollPaintApp {
    type Msg = Msg;

    fn update(&mut self, _msg: Msg, ui: &mut Ui<Msg>) {
        let viewport = HWND(self.grid.hwnd().raw() as *mut c_void);
        self.grid.invalidate();
        self.last.borrow_mut().clear();
        // SAFETY: `viewport` is the live grid window.
        unsafe {
            let _ = UpdateWindow(viewport);
        }
        self.first_min.set(self.last.borrow().iter().copied().min());

        self.last.borrow_mut().clear();
        // SAFETY: a page-down scroll on the live grid, then a synchronous paint.
        unsafe {
            let _ = SendMessageW(
                viewport,
                WM_VSCROLL,
                Some(WPARAM(SB_PAGEDOWN.0 as usize)),
                Some(LPARAM(0)),
            );
            let _ = UpdateWindow(viewport);
        }
        self.second_min
            .set(self.last.borrow().iter().copied().min());
        ui.quit();
    }
}

#[test]
fn scrolling_paints_the_scrolled_rows() {
    let last = Rc::new(RefCell::new(Vec::new()));
    let first_min = Rc::new(Cell::new(None));
    let second_min = Rc::new(Cell::new(None));
    let last_for_make = Rc::clone(&last);
    let first_for_make = Rc::clone(&first_min);
    let second_for_make = Rc::clone(&second_min);

    let Some(run) = run_app_with_watchdog("win32ui.grid_view.scroll", move |ui| {
        let last_for_closure = Rc::clone(&last_for_make);
        let grid = GridView::<Tile, Msg>::new(ui)
            .expect("grid")
            .tile_size(dip(20.0))
            .content_d2d(move |tile: &Tile, canvas, rect, _state| {
                last_for_closure.borrow_mut().push(tile.0 as usize);
                canvas.fill_rect(rect, Color::rgb(0x40, 0x40, 0x80));
            });
        grid.set_model((0..5_000).map(|i| Tile(i as u32)).collect::<Vec<_>>());
        grid.set_bounds(Rect::new(0, 0, 900, 600));

        ui.emit(Msg::Start);
        ScrollPaintApp {
            grid,
            last: last_for_make,
            first_min: first_for_make,
            second_min: second_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    let first = first_min.get().expect("the first paint never ran");
    let second = second_min.get().expect("the scrolled paint never ran");
    assert_eq!(first, 0, "the unscrolled grid starts at tile 0");
    assert!(
        second > first,
        "a page down must paint later rows (first {first}, after {second})"
    );
}

/// A Direct2D grid inside a `ScrollView`, recording every tile the painter is
/// called for (`visited`, across all frames) and the count for one repaint
/// (`observed`).
struct VirtualPaintApp {
    grid: GridView<Tile, Msg>,
    visited: Rc<RefCell<std::collections::HashSet<usize>>>,
    observed: Rc<Cell<Option<usize>>>,
    first: Rc<Cell<Option<usize>>>,
}

impl App for VirtualPaintApp {
    type Msg = Msg;

    fn update(&mut self, _msg: Msg, ui: &mut Ui<Msg>) {
        // Everything painted before this message is the surface's first frames.
        self.first.set(Some(self.visited.borrow().len()));
        let viewport = self.grid.hwnd();
        let before = self.visited.borrow().len();
        // Changing the tile size resizes the scroll extent and invalidates the
        // grid; `UpdateWindow` paints one viewport of tiles synchronously.
        self.grid.set_tile_size(dip(21.0));
        // SAFETY: `viewport` is the live grid window.
        unsafe {
            let _ = UpdateWindow(HWND(viewport.raw() as *mut c_void));
        }
        self.observed
            .set(Some(self.visited.borrow().len() - before));
        ui.quit();
    }
}

/// A Direct2D grid must virtualize from its very first frame: creating the
/// render target must not paint — and so request cover art for — the whole
/// model.
#[test]
fn direct2d_grid_virtualizes_from_the_first_frame() {
    const TILES: usize = 30_000;
    let visited = Rc::new(RefCell::new(std::collections::HashSet::new()));
    let observed = Rc::new(Cell::new(None));
    let first = Rc::new(Cell::new(None));
    let visited_for_make = Rc::clone(&visited);
    let observed_for_make = Rc::clone(&observed);
    let first_for_make = Rc::clone(&first);

    let Some(run) = run_app_with_watchdog("win32ui.grid_view.d2d_virtual", move |ui| {
        let visited_for_closure = Rc::clone(&visited_for_make);
        let grid = GridView::<Tile, Msg>::new(ui)
            .expect("grid")
            .tile_size(dip(20.0))
            .content_d2d(move |tile: &Tile, canvas, rect, _state| {
                canvas.fill_rect(rect, Color::rgb((tile.0 & 0xFF) as u8, 0x40, 0x80));
                visited_for_closure.borrow_mut().insert(tile.0 as usize);
            });
        grid.set_model((0..TILES).map(|i| Tile(i as u32)).collect::<Vec<_>>());
        grid.set_bounds(Rect::new(0, 0, 900, 600));

        ui.emit(Msg::Start);
        VirtualPaintApp {
            grid,
            visited: visited_for_make,
            observed: observed_for_make,
            first: first_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    // 900 px wide at a 20 px tile + 8 px spacing is ~32 columns; a 600 px tall
    // viewport is ~22 rows, so well under 1,000 tiles are visible. Painting the
    // whole 30k model (23k+ before the fix) is the regression this guards.
    let first_count = first.get().expect("the first frames never painted");
    assert!(
        first_count > 100,
        "the first Direct2D frames painted only {first_count} tiles; the grid did not paint at all"
    );
    assert!(
        first_count < 2_000,
        "the first Direct2D frames painted {first_count} of {TILES} tiles"
    );
    let repaint_count = observed.get().expect("the repaint never ran");
    assert!(
        repaint_count < 2_000,
        "a full repaint painted {repaint_count} of {TILES} tiles"
    );
}

/// A Direct2D grid stays paintable after `release_images`: the uploaded covers
/// are dropped but the renderer surface is kept, so the next frame re-uploads
/// and paints without recreating (or blanking) the target.
struct ReleaseImagesApp {
    grid: GridView<Tile, Msg>,
    painted: Rc<Cell<usize>>,
    after_release: Rc<Cell<Option<usize>>>,
}

impl App for ReleaseImagesApp {
    type Msg = Msg;

    fn update(&mut self, _msg: Msg, ui: &mut Ui<Msg>) {
        let viewport = HWND(self.grid.hwnd().raw() as *mut c_void);
        // First frame: upload the cover and record that it painted.
        self.grid.invalidate();
        // SAFETY: `viewport` is the live grid window.
        unsafe {
            let _ = UpdateWindow(viewport);
        }
        assert!(
            self.painted.get() > 0,
            "the first frame never reached the painter"
        );

        // Release the uploaded images, then repaint: if the surface were
        // dropped this would still work, but a kept surface must repaint too.
        self.grid.release_images();
        self.painted.set(0);
        self.grid.invalidate();
        // SAFETY: as above.
        unsafe {
            let _ = UpdateWindow(viewport);
        }
        self.after_release.set(Some(self.painted.get()));
        ui.quit();
    }
}

#[test]
fn release_images_keeps_the_grid_paintable() {
    let painted = Rc::new(Cell::new(0));
    let after_release = Rc::new(Cell::new(None));
    let painted_for_make = Rc::clone(&painted);
    let after_for_make = Rc::clone(&after_release);

    let Some(run) = run_app_with_watchdog("win32ui.grid_view.release_images", move |ui| {
        let image = RgbaImage {
            width: 4,
            height: 4,
            pixels: vec![0x40; 4 * 4 * 4],
        };
        let painted_for_closure = Rc::clone(&painted_for_make);
        let grid = GridView::<Tile, Msg>::new(ui)
            .expect("grid")
            .tile_size(dip(20.0))
            .content_d2d(move |_tile: &Tile, canvas, rect, _state| {
                painted_for_closure.set(painted_for_closure.get() + 1);
                let id = canvas.image(&image);
                canvas.draw_image(id, rect, None, 1.0, Interpolation::Linear);
            });
        grid.set_model((0..200).map(|i| Tile(i as u32)).collect::<Vec<_>>());
        grid.set_bounds(Rect::new(0, 0, 900, 600));

        // Releasing before the first paint must be a no-op, not a panic.
        grid.release_images();

        ui.emit(Msg::Start);
        ReleaseImagesApp {
            grid,
            painted: painted_for_make,
            after_release: after_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    let after = after_release
        .get()
        .expect("the post-release frame never ran");
    assert!(
        after > 0,
        "the grid painted nothing after release_images; the surface was lost"
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
