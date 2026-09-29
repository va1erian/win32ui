//! Drag and drop with real mouse input: a list row dragged onto a `Custom`
//! drop target, and a row dragged to a new position in its own list.
//!
//! The drag itself is OLE's modal `DoDragDrop` loop, so a background thread
//! presses, moves and releases the real pointer while the UI thread sits in
//! the loop. Real input is global, hence the serialising lock.

#![cfg(windows)]

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::run_app_with_watchdog;
use win32ui::prelude::*;
use win32ui::{Renderer, column, row};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, mouse_event,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetSystemMetrics, GetWindowRect, GetWindowThreadProcessId, SM_CXSCREEN,
    SM_CYSCREEN, SendMessageW, SetCursorPos, WindowFromPoint,
};

/// `LVM_GETITEMRECT`.
const LVM_GETITEMRECT: u32 = 0x100E;

/// Real input is global, so the drag tests must not overlap.
static DRAG_LOCK: Mutex<()> = Mutex::new(());

struct Row {
    text: String,
}

fn rows() -> Vec<Row> {
    (0..6)
        .map(|index| Row {
            text: format!("row {index}"),
        })
        .collect()
}

#[derive(Debug)]
enum Msg {
    Begin(Vec<usize>),
    TargetDrop(Vec<u8>),
    ListDrop(ListDrop),
    Done,
}

/// What one drag observed.
#[derive(Default)]
struct Outcome {
    effect: Option<DropEffect>,
    payload: Option<Vec<u8>>,
    list_drop: Option<ListDrop>,
    target_events: Vec<String>,
}

/// A drop target that logs the drag events it sees and accepts app payloads.
struct Target {
    log: Rc<RefCell<Outcome>>,
}

impl CustomWidget for Target {
    type Event = Vec<u8>;

    fn renderer(&self) -> Renderer {
        Renderer::Gdi
    }

    fn paint(&self, canvas: &win32ui::gdi::Canvas, bounds: Rect, theme: &Theme) {
        canvas.fill_rect(bounds, theme.surface);
    }

    fn drag(&self, event: DragEvent<'_>, cx: &mut WidgetCx<Vec<u8>>) -> DropEffect {
        let mut log = self.log.borrow_mut();
        match event {
            DragEvent::Enter(info) => {
                log.target_events
                    .push(format!("enter {:?}", info.position()));
                accept(&info)
            }
            DragEvent::Over(info) => {
                log.target_events
                    .push(format!("over {:?} {:?}", info.position(), info.allowed));
                accept(&info)
            }
            DragEvent::Leave => {
                log.target_events.push("leave".to_string());
                DropEffect::None
            }
            DragEvent::Drop(info) => {
                log.target_events.push("drop".to_string());
                if let Some(payload) = info.data.payload() {
                    cx.emit(payload);
                }
                accept(&info)
            }
        }
    }
}

fn accept(info: &DragInfo<'_>) -> DropEffect {
    if info.data.has_payload() {
        info.preferred_effect()
    } else {
        DropEffect::None
    }
}

struct Harness {
    source: ListView<Row, Msg>,
    _target: Option<Custom<Target, Msg>>,
    outcome: Rc<RefCell<Outcome>>,
}

impl App for Harness {
    type Msg = Msg;

    fn update(&mut self, msg: Msg, ui: &mut Ui<Msg>) {
        match msg {
            Msg::Begin(rows) => {
                let payload = format!("rows:{rows:?}");
                let effect = self.source.begin_drag(
                    payload.as_bytes(),
                    DropEffects::COPY | DropEffects::MOVE,
                    None,
                );
                let effect = effect.ok();
                self.outcome.borrow_mut().effect = effect;
                // The drop message is queued behind this one; quit here only
                // when nothing was dropped.
                if !matches!(effect, Some(DropEffect::Copy | DropEffect::Move)) {
                    ui.quit();
                }
            }
            Msg::TargetDrop(payload) => {
                self.outcome.borrow_mut().payload = Some(payload);
                ui.quit();
            }
            Msg::ListDrop(drop) => {
                self.outcome.borrow_mut().list_drop = Some(drop);
                ui.quit();
            }
            Msg::Done => ui.quit(),
        }
    }
}

fn screen_point(view: Hwnd, x: i32, y: i32) -> (i32, i32) {
    let mut point = POINT { x, y };
    // SAFETY: `view` is a live window and `point` a valid in/out pointer.
    unsafe {
        let _ = ClientToScreen(HWND(view.raw() as *mut core::ffi::c_void), &mut point);
    }
    (point.x, point.y)
}

/// The screen rectangle of list row `item`.
fn row_rect(view: Hwnd, item: usize) -> (i32, i32, i32, i32) {
    let mut rect = RECT::default();
    // SAFETY: `view` is a live list view; `LVIR_BOUNDS` (0) is passed in `left`.
    unsafe {
        SendMessageW(
            HWND(view.raw() as *mut core::ffi::c_void),
            LVM_GETITEMRECT,
            Some(WPARAM(item)),
            Some(LPARAM(&mut rect as *mut RECT as isize)),
        );
    }
    let (left, top) = screen_point(view, rect.left, rect.top);
    let (right, bottom) = screen_point(view, rect.right, rect.bottom);
    (left, top, right, bottom)
}

fn center_of(hwnd: Hwnd) -> (i32, i32) {
    let mut rect = RECT::default();
    // SAFETY: `hwnd` is a live window; `rect` is a valid out pointer.
    unsafe {
        let _ = GetWindowRect(HWND(hwnd.raw() as *mut core::ffi::c_void), &mut rect);
    }
    ((rect.left + rect.right) / 2, (rect.top + rect.bottom) / 2)
}

fn park_real_pointer() {
    // SAFETY: plain integers; a failure (no interactive desktop) is ignored.
    unsafe {
        let _ = SetCursorPos(
            GetSystemMetrics(SM_CXSCREEN) - 1,
            GetSystemMetrics(SM_CYSCREEN) - 1,
        );
    }
}

/// Whether real pointer input can drive the test.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pointer {
    /// The cursor moved to the point and the list is the window under it.
    Reaches,
    /// The cursor could not be moved there, or the window under it is not ours
    /// (a headless CI runner: the cursor is settable but nothing of ours is
    /// hit-testable), so the drag cannot happen and the test skips.
    Unavailable,
    /// The cursor moved but another window of this process covers the list:
    /// a real failure.
    Missed,
}

const POINTER_REACHES: u8 = 0;
const POINTER_UNAVAILABLE: u8 = 1;
const POINTER_MISSED: u8 = 2;

impl Pointer {
    fn code(self) -> u8 {
        match self {
            Pointer::Reaches => POINTER_REACHES,
            Pointer::Unavailable => POINTER_UNAVAILABLE,
            Pointer::Missed => POINTER_MISSED,
        }
    }
}

/// Whether `hwnd` belongs to this process.
fn window_is_ours(hwnd: HWND) -> bool {
    if hwnd.0.is_null() {
        return false;
    }
    let mut pid = 0u32;
    // SAFETY: `pid` is a valid out pointer; a stale handle just yields 0.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid == std::process::id()
}

/// Moves the cursor to `at` and reports whether `expect` is under it.
fn pointer_reaches(at: (i32, i32), expect: Hwnd) -> Pointer {
    let mut cursor = POINT::default();
    // SAFETY: plain integers and a valid out pointer.
    unsafe {
        let _ = SetCursorPos(at.0, at.1);
        std::thread::sleep(Duration::from_millis(150));
        let moved = GetCursorPos(&mut cursor).is_ok() && (cursor.x, cursor.y) == at;
        let under = WindowFromPoint(POINT { x: at.0, y: at.1 });
        if !moved {
            Pointer::Unavailable
        } else if under.0 as usize == expect.raw() {
            Pointer::Reaches
        } else if window_is_ours(under) {
            Pointer::Missed
        } else {
            Pointer::Unavailable
        }
    }
}

/// Presses at `from`, drags to `to` in small steps and releases.
fn drag_with_mouse(from: (i32, i32), to: (i32, i32)) {
    // SAFETY: plain integer arguments to the input APIs.
    unsafe {
        let _ = SetCursorPos(from.0, from.1);
        mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0);
        std::thread::sleep(Duration::from_millis(150));
        // Cross the drag threshold so the list reports `LVN_BEGINDRAG`.
        for step in 1..=6 {
            let _ = SetCursorPos(from.0 + step * 4, from.1);
            std::thread::sleep(Duration::from_millis(30));
        }
        std::thread::sleep(Duration::from_millis(250));
        for step in 1..=20 {
            let x = from.0 + (to.0 - from.0) * step / 20;
            let y = from.1 + (to.1 - from.1) * step / 20;
            let _ = SetCursorPos(x, y);
            std::thread::sleep(Duration::from_millis(30));
        }
        std::thread::sleep(Duration::from_millis(500));
        mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0);
        std::thread::sleep(Duration::from_millis(300));
    }
    park_real_pointer();
}

/// Which drop target the run drags onto.
#[derive(Clone, Copy)]
enum Goal {
    /// Row 1 onto the `Custom` target.
    CustomTarget,
    /// Row 0 to the lower half of row 3 in the same list.
    ReorderInList,
}

fn run_case(name: &str, goal: Goal) -> Option<Outcome> {
    let _serial = DRAG_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    park_real_pointer();
    let outcome = Rc::new(RefCell::new(Outcome::default()));
    let outcome_for_app = Rc::clone(&outcome);
    let input_state = Arc::new(AtomicU8::new(POINTER_REACHES));
    let input_for_thread = Arc::clone(&input_state);
    let Some(run) = run_app_with_watchdog(name, move |ui| {
        let list = ListView::new(ui)
            .expect("list")
            .column("Row", Fill, |row: &Row| row.text.as_str())
            .on_begin_drag(|rows, _| Some(Msg::Begin(rows.to_vec())));
        let list = match goal {
            Goal::ReorderInList => list
                .on_drop(|drop| Some(Msg::ListDrop(drop)))
                .expect("list drop target"),
            Goal::CustomTarget => list,
        };
        list.set_model(rows());
        let target = Custom::new(
            ui,
            Target {
                log: Rc::clone(&outcome_for_app),
            },
        )
        .expect("target")
        .on_event(|payload| Some(Msg::TargetDrop(payload)))
        .accept_drops()
        .expect("drop target");
        ui.set_layout(row![list.fill(1), target.fill(1)]);

        let view = list.control().hwnd();
        let target_window = target.control().hwnd();
        let proxy = ui.proxy();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(700));
            let (from, to) = match goal {
                Goal::CustomTarget => {
                    let (left, top, _, bottom) = row_rect(view, 1);
                    ((left + 20, (top + bottom) / 2), center_of(target_window))
                }
                Goal::ReorderInList => {
                    let (left, top, _, bottom) = row_rect(view, 0);
                    let (_, third_top, _, third_bottom) = row_rect(view, 3);
                    let lower = third_bottom - (third_bottom - third_top) / 4;
                    ((left + 20, (top + bottom) / 2), (left + 20, lower))
                }
            };
            let pointer = pointer_reaches(from, view);
            if pointer == Pointer::Reaches {
                drag_with_mouse(from, to);
            } else {
                input_for_thread.store(pointer.code(), Ordering::SeqCst);
            }
            park_real_pointer();
            let _ = proxy.send(Msg::Done);
        });

        Harness {
            source: list,
            _target: Some(target),
            outcome: outcome_for_app,
        }
    }) else {
        panic!("{name}: the session could not create windows");
    };
    assert!(!run.timed_out, "{name}: the watchdog fired");
    match input_state.load(Ordering::SeqCst) {
        POINTER_UNAVAILABLE => {
            eprintln!("{name}: skipped, the cursor cannot be moved in this session");
            return None;
        }
        POINTER_MISSED => {
            panic!("{name}: the cursor moved but the list is not the window under it")
        }
        _ => {}
    }
    Some(outcome.take())
}

#[test]
fn list_row_dropped_on_a_custom_target() {
    let Some(outcome) = run_case("win32ui.dnd.custom", Goal::CustomTarget) else {
        return;
    };
    assert_eq!(
        outcome.payload.as_deref(),
        Some(&b"rows:[1]"[..]),
        "the payload arrived: {:?}",
        outcome.target_events
    );
    assert_eq!(outcome.effect, Some(DropEffect::Move));
    assert!(outcome.target_events[0].starts_with("enter"));
    assert_eq!(
        outcome.target_events.last().map(String::as_str),
        Some("drop")
    );
}

#[test]
fn list_row_reordered_within_its_list() {
    let Some(outcome) = run_case("win32ui.dnd.reorder", Goal::ReorderInList) else {
        return;
    };
    let drop = outcome.list_drop.expect("the list reported a drop");
    assert_eq!(drop.payload.as_deref(), Some(&b"rows:[0]"[..]));
    assert_eq!(drop.row, Some(3));
    assert_eq!(drop.index, 4, "the lower half of row 3 inserts after it");
    assert_eq!(outcome.effect, Some(DropEffect::Move));
}

#[test]
fn drop_targets_register_and_release_without_a_drag() {
    let _serial = DRAG_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let outcome = Rc::new(RefCell::new(Outcome::default()));
    let Some(_) = run_app_with_watchdog("win32ui.dnd.register", move |ui| {
        let list = ListView::new(ui)
            .expect("list")
            .column("Row", Fill, |row: &Row| row.text.as_str())
            .on_drop(|drop| Some(Msg::ListDrop(drop)))
            .expect("list drop target");
        list.set_model(rows());
        list.set_insert_mark(Some((2, false)));
        list.set_insert_mark(Some((5, true)));
        list.set_insert_mark(None);
        let target = Custom::new(
            ui,
            Target {
                log: Rc::clone(&outcome),
            },
        )
        .expect("target")
        .accept_drops()
        .expect("drop target");
        ui.set_layout(column![list.fill(1), target.fill(1)]);
        ui.emit(Msg::Done);
        Harness {
            source: list,
            _target: Some(target),
            outcome,
        }
    }) else {
        panic!("the session could not create windows");
    };
}

#[test]
fn apps_key_has_its_virtual_key_code() {
    assert_eq!(Key::APPS.code(), 0x5D);
}
