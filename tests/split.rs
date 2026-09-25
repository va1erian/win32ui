//! Split layout node: the divider reserves space, the panes tile the parent,
//! and the divider child window is rebuilt when a layout is reinstalled.
//!
//! The pure arithmetic lives in `src/app/layout/tests.rs`; this exercises the
//! whole path through a real window, including the divider child window and its
//! mouse-driven position.

#![cfg(windows)]

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Mutex;

use common::run_app_with_watchdog;
use win32ui::prelude::*;
use win32ui::{column, split_col, split_row};
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    SendMessageW, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
};

/// The divider child window class prefix (see `split::build_divider`; the
/// registration appends a unique `::<sequence>`).
const DIVIDER_CLASS: &str = "win32ui::win32ui.split";

fn divider(root: HWND) -> Option<HWND> {
    let mut found = None;
    // SAFETY: enumeration of a live window's children; the callback only reads.
    let _ = unsafe {
        windows::Win32::UI::WindowsAndMessaging::EnumChildWindows(
            Some(root),
            Some(find_proc),
            LPARAM(&mut found as *mut Option<HWND> as isize),
        )
    };
    found
}

unsafe extern "system" fn find_proc(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
    if is_divider(hwnd) {
        // SAFETY: the caller passed a `*mut Option<HWND>` through `lparam`.
        unsafe { *(lparam.0 as *mut Option<HWND>) = Some(hwnd) };
        return windows::core::BOOL(0);
    }
    windows::core::BOOL(1)
}

fn is_divider(hwnd: HWND) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::GetClassNameW;
    let mut buffer = [0u16; 64];
    // SAFETY: `hwnd` is a live child and `buffer` is a valid out slice.
    let len = unsafe { GetClassNameW(hwnd, &mut buffer) };
    len > 0 && String::from_utf16_lossy(&buffer[..len as usize]).starts_with(DIVIDER_CLASS)
}

fn divider_count(root: HWND) -> usize {
    let mut count = 0usize;
    // SAFETY: enumeration of a live window's children; the callback only reads.
    let _ = unsafe {
        windows::Win32::UI::WindowsAndMessaging::EnumChildWindows(
            Some(root),
            Some(count_proc),
            LPARAM(&mut count as *mut usize as isize),
        )
    };
    count
}

unsafe extern "system" fn count_proc(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
    if is_divider(hwnd) {
        // SAFETY: the caller passed a `*mut usize` through `lparam`.
        unsafe { *(lparam.0 as *mut usize) += 1 };
    }
    windows::core::BOOL(1)
}

fn raw(hwnd: Hwnd) -> HWND {
    HWND(hwnd.raw() as *mut core::ffi::c_void)
}

struct SplitApp {
    left: Label,
    right: Label,
    geometry_ok: Rc<Cell<bool>>,
}

impl App for SplitApp {
    type Msg = ();

    fn update(&mut self, _msg: (), ui: &mut Ui<()>) {
        let dpi = ui.dpi();
        let divider = dip(5.0).to_px(dpi).value();
        let expected = dip(150.0).to_px(dpi).value();
        let left = self.left.bounds();
        let right = self.right.bounds();
        let client = ui.client_rect();
        self.geometry_ok.set(
            left.width() == expected
                && right.left == left.right + divider
                && right.right == client.width(),
        );
        ui.quit();
    }
}

#[test]
fn split_layout_positions_both_panes_around_the_divider() {
    let geometry_ok = Rc::new(Cell::new(false));
    let ok_for_make = Rc::clone(&geometry_ok);

    let Some(run) = run_app_with_watchdog("win32ui.split", move |ui| {
        let left = Label::new(ui, Rect::default(), "left").expect("left");
        let right = Label::new(ui, Rect::default(), "right").expect("right");
        ui.set_layout(column![
            split_row![left, right]
                .position(dip(150.0))
                .min(dip(40.0), dip(40.0))
        ]);
        ui.emit(());
        SplitApp {
            left,
            right,
            geometry_ok: ok_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    assert!(
        geometry_ok.get(),
        "the split did not tile the parent around the divider"
    );
}

/// An app that reinstalls its layout from `Msg::Reinstall` and counts the
/// divider windows each time.
struct ReinstallApp {
    left: Label,
    right: Label,
    counts: Rc<RefCell<Vec<usize>>>,
}

impl App for ReinstallApp {
    type Msg = ();

    fn update(&mut self, _msg: (), ui: &mut Ui<()>) {
        let root = raw(ui.hwnd());
        self.counts.borrow_mut().push(divider_count(root));
        // Rebuild the layout around the same widgets; the stale divider must
        // not survive alongside the new one.
        ui.set_layout(column![
            split_row![self.left, self.right].position(dip(120.0))
        ]);
        ui.relayout();
        self.counts.borrow_mut().push(divider_count(root));
        ui.quit();
    }
}

#[test]
fn reinstalling_a_layout_replaces_the_divider_window() {
    let counts = Rc::new(RefCell::new(Vec::new()));
    let counts_for_make = Rc::clone(&counts);

    let Some(run) = run_app_with_watchdog("win32ui.split.reinstall", move |ui| {
        let left = Label::new(ui, Rect::default(), "left").expect("left");
        let right = Label::new(ui, Rect::default(), "right").expect("right");
        ui.set_layout(column![split_row![left, right].position(dip(150.0))]);
        ui.emit(());
        ReinstallApp {
            left,
            right,
            counts: counts_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    let counts = counts.borrow();
    assert_eq!(counts.as_slice(), &[1, 1], "one divider before and after");
}

struct NestedApp {
    a: Label,
    b: Label,
    c: Label,
    count: Rc<Cell<usize>>,
}

impl App for NestedApp {
    type Msg = ();

    fn update(&mut self, _msg: (), ui: &mut Ui<()>) {
        let _ = (&self.a, &self.b, &self.c);
        self.count.set(divider_count(raw(ui.hwnd())));
        ui.quit();
    }
}

#[test]
fn a_split_nested_in_a_pane_binds_its_own_divider() {
    let count = Rc::new(Cell::new(0));
    let count_for_make = Rc::clone(&count);

    let Some(run) = run_app_with_watchdog("win32ui.split.nested", move |ui| {
        let a = Label::new(ui, Rect::default(), "a").expect("a");
        let b = Label::new(ui, Rect::default(), "b").expect("b");
        let c = Label::new(ui, Rect::default(), "c").expect("c");
        ui.set_layout(column![split_col![a, split_row![b, c].position(dip(80.0))]]);
        ui.emit(());
        NestedApp {
            a,
            b,
            c,
            count: count_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    assert_eq!(count.get(), 2, "both splits bound a divider");
}

/// Real input is global, so the drag test must not overlap another test.
static DRAG_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, PartialEq)]
enum DragMsg {
    Start,
    Moved(Dip),
}

struct DragApp {
    left: Label,
    right: Label,
    reported: Rc<RefCell<Vec<f32>>>,
    /// Whether the right pane's on-screen width matched the reported anchored
    /// extent (the end-anchoring check).
    end_anchor_ok: Rc<Cell<bool>>,
}

impl App for DragApp {
    type Msg = DragMsg;

    fn update(&mut self, msg: DragMsg, ui: &mut Ui<DragMsg>) {
        let _ = (&self.left, &self.right);
        match msg {
            DragMsg::Start => {
                let dpi = ui.dpi();
                let hwnd = divider(raw(ui.hwnd())).expect("bound divider");
                let mut client = RECT::default();
                // SAFETY: `hwnd` is live and `client` is a valid out pointer.
                unsafe {
                    windows::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut client)
                        .expect("divider client rect");
                }
                let y = (client.top + client.bottom) / 2;
                let x = client.right / 2;
                let delta = dip(40.0).to_px(dpi).value();
                let send = |message: u32, x: i32, wparam: usize| {
                    let lparam = ((y as i16 as u16 as isize) << 16) | (x as i16 as u16 as isize);
                    // SAFETY: a synchronous message to the divider's own window.
                    unsafe {
                        SendMessageW(hwnd, message, Some(WPARAM(wparam)), Some(LPARAM(lparam)));
                    }
                };
                send(WM_LBUTTONDOWN, x, 1);
                send(WM_MOUSEMOVE, x + delta, 1);
                send(WM_LBUTTONUP, x + delta, 0);
            }
            DragMsg::Moved(position) => {
                self.reported.borrow_mut().push(position.value());
                // The anchored pane is the second (right) one, so its on-screen
                // width must equal the reported extent.
                let expected = dip(position.value()).to_px(ui.dpi()).value();
                self.end_anchor_ok
                    .set((self.right.bounds().width() - expected).abs() <= 2);
                ui.quit();
            }
        }
    }
}

#[test]
fn dragging_the_divider_reports_the_anchored_pane_extent() {
    let _serial = DRAG_LOCK.lock().expect("drag lock");
    let reported = Rc::new(RefCell::new(Vec::new()));
    let reported_for_make = Rc::clone(&reported);
    let end_anchor_ok = Rc::new(Cell::new(false));
    let end_anchor_ok_for_make = Rc::clone(&end_anchor_ok);

    let Some(run) = run_app_with_watchdog("win32ui.split.drag", move |ui| {
        let left = Label::new(ui, Rect::default(), "left").expect("left");
        let right = Label::new(ui, Rect::default(), "right").expect("right");
        ui.set_layout(column![
            split_row![left, right]
                .position_b(dip(100.0))
                .min(dip(40.0), dip(40.0))
                .on_moved(|position| Some(DragMsg::Moved(position)))
        ]);
        ui.emit(DragMsg::Start);
        DragApp {
            left,
            right,
            reported: reported_for_make,
            end_anchor_ok: end_anchor_ok_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    let reported = reported.borrow();
    assert_eq!(reported.len(), 1, "one move was reported: {reported:?}");
    assert!(
        (reported[0] - 140.0).abs() < 2.0,
        "the anchored (second) pane's extent was reported: {reported:?}"
    );
    assert!(
        end_anchor_ok.get(),
        "the second pane kept the reported extent (end-anchored)"
    );
}
