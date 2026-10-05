//! A list view scrolled far down keeps a valid scroll origin when its model
//! shrinks enough to drop the vertical scroll bar.
//!
//! Regression: dropping the scroll bar resizes the client area, and the `Fill`
//! column restretch used to run inside that `WM_SIZE`, nested in the list
//! view's own scroll-bar update. The control then applied its scroll
//! correction twice, leaving the top row as far above row 0 as it had been
//! below it (`LVM_GETTOPINDEX` negative), so every row painted off screen.

#![cfg(windows)]

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::run_app_with_watchdog;
use win32ui::prelude::*;
use win32ui::{ColumnWidth, ListView, column, dip};
use windows::Win32::Foundation::{HWND, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{SB_BOTTOM, SendMessageW, WM_VSCROLL};

/// `LVM_GETTOPINDEX`, from `commctrl.h`.
const LVM_GETTOPINDEX: u32 = 0x1027;

/// Enough rows to scroll far past the first page.
const LONG: usize = 3000;
/// Few enough rows to fit without a vertical scroll bar.
const SHORT: usize = 3;

struct Row {
    text: String,
}

fn rows(count: usize) -> Vec<Row> {
    (0..count)
        .map(|index| Row {
            text: format!("row {index}"),
        })
        .collect()
}

fn top_index(view: Hwnd) -> isize {
    let hwnd = HWND(view.raw() as *mut core::ffi::c_void);
    // SAFETY: `view` is a live list view; the message takes no pointers.
    unsafe { SendMessageW(hwnd, LVM_GETTOPINDEX, None, None).0 }
}

enum Msg {
    ScrollToBottom,
    Shrink,
    Regrow,
}

/// The top row index after each step: scrolled, shrunk, regrown.
#[derive(Default)]
struct Tops {
    scrolled: Cell<isize>,
    shrunk: Cell<isize>,
    regrown: Cell<isize>,
}

struct Harness {
    list: ListView<Row, Msg>,
    tops: Rc<Tops>,
}

impl App for Harness {
    type Msg = Msg;

    fn update(&mut self, msg: Msg, ui: &mut Ui<Msg>) {
        let view = self.list.control().hwnd();
        match msg {
            Msg::ScrollToBottom => {
                let hwnd = HWND(view.raw() as *mut core::ffi::c_void);
                // SAFETY: `view` is a live list view; the message takes no pointers.
                unsafe { SendMessageW(hwnd, WM_VSCROLL, Some(WPARAM(SB_BOTTOM.0 as usize)), None) };
                self.tops.scrolled.set(top_index(view));
                ui.emit(Msg::Shrink);
            }
            Msg::Shrink => {
                self.list.set_model(rows(SHORT));
                self.tops.shrunk.set(top_index(view));
                ui.emit(Msg::Regrow);
            }
            Msg::Regrow => {
                self.list.set_model(rows(LONG));
                self.tops.regrown.set(top_index(view));
                ui.quit();
            }
        }
    }
}

#[test]
fn shrinking_a_scrolled_model_keeps_the_top_row_in_range() {
    let tops = Rc::new(Tops::default());
    let tops_for_make = Rc::clone(&tops);
    let Some(run) = run_app_with_watchdog("win32ui.listview.scroll-origin", move |ui| {
        let list = ListView::new(ui)
            .expect("list")
            .column("Fixed", dip(120.0), |row: &Row| row.text.as_str())
            .column("Fill", ColumnWidth::Fill, |row: &Row| row.text.as_str())
            .column("Tail", dip(120.0), |row: &Row| row.text.as_str());
        list.set_model(rows(LONG));
        ui.set_layout(column![list.fill(1)]);
        ui.emit(Msg::ScrollToBottom);
        Harness {
            list,
            tops: tops_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    assert!(
        tops.scrolled.get() > 0,
        "the list did not scroll down: top {}",
        tops.scrolled.get()
    );
    assert_eq!(tops.shrunk.get(), 0, "top row after shrinking");
    assert_eq!(tops.regrown.get(), 0, "top row after regrowing");
}
