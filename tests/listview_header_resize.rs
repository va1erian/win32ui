//! Dragging a list-view header divider with real mouse input, so the default
//! header tracking loop, the `HDN_BEGINTRACK`/`HDN_ENDTRACK` notifications and
//! the `Fill`-column restretch are all exercised.
//!
//! Regression: a `Fill` column used to snap back to its computed width the
//! instant the drag ended, because `end_track` restretched every `Fill` column
//! without knowing which divider the user had just dragged.

#![cfg(windows)]

mod common;

use common::run_app_with_watchdog;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use win32ui::prelude::*;
use win32ui::{ColumnWidth, ListView, column, dip};
use windows::Win32::Foundation::{HWND, RECT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, mouse_event,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GA_ROOT, GetAncestor, GetSystemMetrics, GetWindowRect, SM_CXSCREEN, SM_CYSCREEN, SendMessageW,
    SetCursorPos, SetForegroundWindow,
};

/// `LVM_GETHEADER`.
const LVM_GETHEADER: u32 = 0x101F;
/// `LVM_GETCOLUMNWIDTH`.
const LVM_GETCOLUMNWIDTH: u32 = 0x101D;
/// `HDM_GETITEMRECT`.
const HDM_GETITEMRECT: u32 = 0x1207;

/// Real input is global (cursor, foreground), so the two tests must not run at
/// the same time even though libtest runs them on parallel threads.
static DRAG_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, PartialEq)]
enum Msg {
    Late,
}

struct Row {
    text: String,
}

/// Which divider to drag: the one after `Fixed` (column 0) or the one after
/// `Fill` (column 1, with another fixed column after it so it is not the
/// clipped trailing edge).
#[derive(Clone, Copy, PartialEq)]
enum Target {
    Fixed,
    Fill,
}

/// The widths captured around one drag: `(before, after)` for the fixed, fill
/// and trailing fixed columns.
#[derive(Clone, Copy, Debug)]
struct Widths {
    fixed_before: i32,
    fixed_after: i32,
    fill_before: i32,
    fill_after: i32,
    tail_before: i32,
    tail_after: i32,
}

/// Parks the real pointer in the far corner of the primary screen.
fn park_real_pointer() {
    // SAFETY: plain integer arguments; a failure (no interactive desktop) is ignored.
    unsafe {
        let _ = SetCursorPos(
            GetSystemMetrics(SM_CXSCREEN) - 1,
            GetSystemMetrics(SM_CYSCREEN) - 1,
        );
    }
}

fn raw_hwnd(hwnd: Hwnd) -> HWND {
    HWND(hwnd.raw() as *mut core::ffi::c_void)
}

fn header_hwnd(view: Hwnd) -> HWND {
    // SAFETY: `view` is a live list view; LVM_GETHEADER returns its header.
    unsafe {
        HWND(SendMessageW(raw_hwnd(view), LVM_GETHEADER, None, None).0 as *mut core::ffi::c_void)
    }
}

fn column_width(view: Hwnd, column: usize) -> i32 {
    // SAFETY: `view` is a live list view and `column` is in range.
    unsafe {
        SendMessageW(
            raw_hwnd(view),
            LVM_GETCOLUMNWIDTH,
            Some(WPARAM(column)),
            None,
        )
        .0 as i32
    }
}

fn rect(hwnd: HWND) -> RECT {
    let mut rect = RECT::default();
    // SAFETY: `hwnd` is live and `rect` is a valid out pointer.
    unsafe { GetWindowRect(hwnd, &mut rect) }.expect("window rect");
    rect
}

/// The header item's rectangle, in the header's client coordinates.
fn header_item_rect(header: HWND, item: usize) -> RECT {
    let mut rect = RECT::default();
    // SAFETY: `header` is a live header and `rect` is a valid out pointer.
    unsafe {
        SendMessageW(
            header,
            HDM_GETITEMRECT,
            Some(WPARAM(item)),
            Some(windows::Win32::Foundation::LPARAM(
                &mut rect as *mut RECT as isize,
            )),
        )
    };
    rect
}

/// Drags one header divider by `delta` device pixels with real mouse input and
/// reports the column widths around the drag.
///
/// Runs on a background thread (the app's own window is kept alive by the
/// harness) because the header's tracking loop pumps messages; sending
/// synchronously from `update` could deadlock.
fn drag_divider(
    view: Hwnd,
    dpi: u32,
    target: Target,
    result: Arc<Mutex<Option<Widths>>>,
    quit: impl Fn() + Send + 'static,
) {
    let header = header_hwnd(view);
    let fixed_before = column_width(view, 0);
    let fill_before = column_width(view, 1);
    let tail_before = column_width(view, 2);

    // Divider after the column being resized: fixed -> column 0, fill -> column 1.
    // Use the header's own item rect, not a sum of column widths, so any inset
    // the header applies is accounted for.
    let divider_item = match target {
        Target::Fixed => 0,
        Target::Fill => 1,
    };
    let divider_left = header_item_rect(header, divider_item).right;
    let delta = (60.0 * f64::from(dpi) / 96.0).round() as i32;

    let header_rect = rect(header);
    let x = header_rect.left + divider_left;
    let y = (header_rect.top + header_rect.bottom) / 2;

    // SAFETY: all handles are live (the harness keeps the list view alive) for
    // the duration of the thread.
    unsafe {
        let root = GetAncestor(raw_hwnd(view), GA_ROOT);
        let _ = SetForegroundWindow(root);

        let _ = SetCursorPos(x, y);
        std::thread::sleep(Duration::from_millis(120));
        mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0);
        std::thread::sleep(Duration::from_millis(120));
        for step in 1..=8 {
            let _ = SetCursorPos(x + delta * step / 8, y);
            std::thread::sleep(Duration::from_millis(30));
        }
        std::thread::sleep(Duration::from_millis(120));
        mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0);
        std::thread::sleep(Duration::from_millis(300));
    }

    park_real_pointer();
    *result.lock().expect("result lock") = Some(Widths {
        fixed_before,
        fixed_after: column_width(view, 0),
        fill_before,
        fill_after: column_width(view, 1),
        tail_before,
        tail_after: column_width(view, 2),
    });
    quit();
}

fn run_case(name: &str, target: Target) -> Widths {
    let _serial = DRAG_LOCK.lock().expect("drag lock");
    park_real_pointer();
    let result: Arc<Mutex<Option<Widths>>> = Arc::new(Mutex::new(None));
    let result_for_make = Arc::clone(&result);
    let Some(run) = run_app_with_watchdog(name, move |ui| {
        let list = ListView::new(ui)
            .expect("list")
            .column("Fixed", dip(120.0), |row: &Row| row.text.as_str())
            .column("Fill", ColumnWidth::Fill, |row: &Row| row.text.as_str())
            .column("Tail", dip(120.0), |row: &Row| row.text.as_str());
        list.set_model(vec![Row {
            text: "row".to_string(),
        }]);
        ui.set_layout(column![list.fill(1)]);

        let view = list.control().hwnd();
        let dpi = ui.dpi();
        let result = Arc::clone(&result_for_make);
        let proxy = ui.proxy();
        let proxy_for_quit = proxy.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(700));
            drag_divider(view, dpi, target, result, move || {
                let _ = proxy_for_quit.send(Msg::Late);
            });
        });

        Harness {
            list,
            result: result_for_make,
        }
    }) else {
        panic!("{name}: the session could not create windows");
    };
    assert!(
        !run.timed_out,
        "{name}: the watchdog fired before the drag ended"
    );
    result
        .lock()
        .expect("result lock")
        .expect("the drag reported a result")
}

struct Harness {
    /// Kept alive so the list view's window (and header) outlives the drag.
    list: ListView<Row, Msg>,
    result: Arc<Mutex<Option<Widths>>>,
}

impl App for Harness {
    type Msg = Msg;

    fn update(&mut self, _msg: Msg, ui: &mut Ui<Msg>) {
        let _ = &self.list;
        if self.result.lock().expect("result lock").is_some() {
            ui.quit();
        }
    }
}

#[test]
fn fixed_column_resize_sticks() {
    let widths = run_case("win32ui.resize.fixed", Target::Fixed);
    assert!(
        widths.fixed_after > widths.fixed_before + 30,
        "the dragged fixed column kept its width: {widths:?}"
    );
    assert_eq!(
        widths.tail_after, widths.tail_before,
        "the trailing column was untouched: {widths:?}"
    );
}

#[test]
fn fill_column_resize_sticks() {
    let widths = run_case("win32ui.resize.fill", Target::Fill);
    assert!(
        widths.fill_after > widths.fill_before + 30,
        "the dragged fill column kept its width: {widths:?}"
    );
    assert_eq!(
        widths.tail_after, widths.tail_before,
        "the trailing column was untouched: {widths:?}"
    );
}
