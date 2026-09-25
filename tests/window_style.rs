//! `WindowSpec::resizable`/`minimizable`/`maximizable`: the requested frame
//! bits land on the native `HWND`, and a modal drops its minimize box by
//! default while a non-modal secondary window keeps one.

#![cfg(windows)]

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::run_app_spec_with_watchdog;
use win32ui::prelude::*;

/// `WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX` for `hwnd`.
fn frame_bits(hwnd: Hwnd) -> (bool, bool, bool) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_STYLE, GetWindowLongPtrW, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_THICKFRAME,
    };

    let raw = HWND(hwnd.raw() as *mut core::ffi::c_void);
    // SAFETY: `raw` names the live window created by this test; `GWL_STYLE`
    // only reads its style bits.
    let style = unsafe { GetWindowLongPtrW(raw, GWL_STYLE) } as u32;
    (
        style & WS_THICKFRAME.0 != 0,
        style & WS_MINIMIZEBOX.0 != 0,
        style & WS_MAXIMIZEBOX.0 != 0,
    )
}

enum Msg {
    Check,
}

struct RecordApp {
    bits: Rc<Cell<(bool, bool, bool)>>,
}

impl App for RecordApp {
    type Msg = Msg;

    fn update(&mut self, msg: Msg, ui: &mut Ui<Msg>) {
        match msg {
            Msg::Check => {
                self.bits.set(frame_bits(ui.hwnd()));
                ui.quit();
            }
        }
    }
}

/// A top-level window defaults to a resizable frame with both boxes.
#[test]
fn top_level_defaults_to_resizable_with_both_boxes() {
    let bits = Rc::new(Cell::new((false, false, false)));
    let bits_for_make = Rc::clone(&bits);
    let spec = WindowSpec::new("window_style.default").theme(Theme::light());
    let Some(run) = run_app_spec_with_watchdog(spec, move |ui| {
        ui.emit(Msg::Check);
        RecordApp {
            bits: bits_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    assert_eq!(bits.get(), (true, true, true));
}

/// `resizable(false)`/`minimizable(false)`/`maximizable(false)` each drop
/// their own frame bit and no other.
#[test]
fn spec_options_drop_the_matching_frame_bits() {
    let bits = Rc::new(Cell::new((true, true, true)));
    let bits_for_make = Rc::clone(&bits);
    let spec = WindowSpec::new("window_style.fixed")
        .theme(Theme::light())
        .resizable(false)
        .minimizable(false)
        .maximizable(false);
    let Some(run) = run_app_spec_with_watchdog(spec, move |ui| {
        ui.emit(Msg::Check);
        RecordApp {
            bits: bits_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    assert_eq!(bits.get(), (false, false, false));
}

enum OwnerMsg {
    Start,
}

struct OwnerApp {
    window_bits: Rc<Cell<(bool, bool, bool)>>,
    modal_bits: Rc<Cell<(bool, bool, bool)>>,
}

impl App for OwnerApp {
    type Msg = OwnerMsg;

    fn update(&mut self, msg: OwnerMsg, ui: &mut Ui<OwnerMsg>) {
        match msg {
            OwnerMsg::Start => {
                let window_bits = Rc::clone(&self.window_bits);
                let handle = ui
                    .open_window(
                        WindowSpec::new("window_style.child").size(dip(200.0), dip(120.0)),
                        move |ui| {
                            window_bits.set(frame_bits(ui.hwnd()));
                            RecordApp {
                                bits: Rc::new(Cell::new((false, false, false))),
                            }
                        },
                    )
                    .expect("child window");

                let modal_bits = Rc::clone(&self.modal_bits);
                let _: Option<()> = ui.open_modal(
                    WindowSpec::new("window_style.modal").size(dip(200.0), dip(120.0)),
                    move |ui| {
                        modal_bits.set(frame_bits(ui.hwnd()));
                        ui.close_with_result(());
                        RecordApp {
                            bits: Rc::new(Cell::new((false, false, false))),
                        }
                    },
                );

                handle.close();
                ui.quit();
            }
        }
    }
}

/// A non-modal secondary window keeps a minimize box by default; a modal
/// opened the same way drops it.
#[test]
fn modal_drops_the_minimize_box_by_default_but_open_window_keeps_it() {
    let window_bits = Rc::new(Cell::new((false, false, false)));
    let modal_bits = Rc::new(Cell::new((false, false, false)));
    let window_bits_for_make = Rc::clone(&window_bits);
    let modal_bits_for_make = Rc::clone(&modal_bits);
    let spec = WindowSpec::new("window_style.owner").theme(Theme::light());
    let Some(run) = run_app_spec_with_watchdog(spec, move |ui| {
        ui.emit(OwnerMsg::Start);
        OwnerApp {
            window_bits: window_bits_for_make,
            modal_bits: modal_bits_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    assert!(
        window_bits.get().1,
        "a non-modal open_window dropped the minimize box by default"
    );
    assert!(
        !modal_bits.get().1,
        "open_modal kept the minimize box by default"
    );
}
