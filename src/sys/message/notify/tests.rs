//! Regression coverage for the ListView-only guard around `NMITEMACTIVATE`.
//!
//! `NM_CLICK`/`NM_DBLCLK`/`NM_RCLICK`/`NM_RETURN` are generic `NM_*` codes
//! every common control sends via a plain `NMHDR`; only a `ListView` sends the
//! larger `NMITEMACTIVATE`. These tests build an `NMHDR`-only payload (as a
//! header, tab control, or any unregistered window would) and check that
//! `decode_notify` never reads it as an `NMITEMACTIVATE` unless the sender is
//! registered as a `ListView` — reading it for anything else is the
//! out-of-bounds read fixed here.

use core::ffi::c_void;
use std::cell::RefCell;
use std::rc::Rc;

use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::Controls::{NM_CLICK, NM_RETURN, NMHDR};

use super::*;
use crate::controls::registry::{ControlEvents, ControlKind};
use crate::hwnd::Hwnd;

struct Stub(ControlKind);

impl ControlEvents for Stub {
    fn kind(&self) -> ControlKind {
        self.0
    }

    fn on_notification(
        &mut self,
        _hwnd: Hwnd,
        _code: u32,
        _wparam: usize,
        _lparam: isize,
    ) -> Option<isize> {
        None
    }
}

/// A bare `NMHDR`, as a control that only ever sends `NMHDR` (not
/// `NMITEMACTIVATE`) would. Callers keep this alive for as long as the
/// `LPARAM` they build from its address is used.
fn nmhdr(hwnd: Hwnd, code: u32) -> NMHDR {
    NMHDR {
        hwndFrom: HWND(hwnd.raw() as *mut c_void),
        idFrom: 0,
        code,
    }
}

#[test]
fn unregistered_sender_does_not_read_nmitemactivate() {
    let hwnd = Hwnd::from_raw(0xC1A1);
    // Simulates a ListView column header forwarding its own NM_CLICK to the
    // parent: never registered via `registry::register`.
    let header = nmhdr(hwnd, NM_CLICK);
    let lparam = LPARAM(&header as *const NMHDR as isize);
    assert!(matches!(decode_notify(lparam), Notify::Other { .. }));
}

#[test]
fn other_control_kind_does_not_read_nmitemactivate() {
    let hwnd = Hwnd::from_raw(0xC1A2);
    registry::register(hwnd, Rc::new(RefCell::new(Stub(ControlKind::Tooltip))));
    let header = nmhdr(hwnd, NM_CLICK);
    let lparam = LPARAM(&header as *const NMHDR as isize);
    let result = decode_notify(lparam);
    registry::unregister(hwnd);
    assert!(matches!(result, Notify::Other { .. }));
}

#[test]
fn nm_return_from_non_listview_does_not_read_nmitemactivate() {
    let hwnd = Hwnd::from_raw(0xC1A3);
    // No registration: e.g. a tab control sending NM_RETURN.
    let header = nmhdr(hwnd, NM_RETURN);
    let lparam = LPARAM(&header as *const NMHDR as isize);
    assert!(matches!(decode_notify(lparam), Notify::Other { .. }));
}

#[test]
fn listview_click_still_decodes() {
    let hwnd = Hwnd::from_raw(0xC1A4);
    registry::register(hwnd, Rc::new(RefCell::new(Stub(ControlKind::ListView))));
    let info = NMITEMACTIVATE {
        hdr: NMHDR {
            hwndFrom: HWND(hwnd.raw() as *mut c_void),
            idFrom: 7,
            code: NM_CLICK,
        },
        iItem: 3,
        ..Default::default()
    };
    let lparam = LPARAM(&info as *const NMITEMACTIVATE as isize);
    let result = decode_notify(lparam);
    registry::unregister(hwnd);
    assert!(matches!(
        result,
        Notify::ListView {
            id: 7,
            event: ListViewEvent::Click { item: 3 }
        }
    ));
}

#[test]
fn treeview_click_still_decodes() {
    let hwnd = Hwnd::from_raw(0xC1A5);
    registry::register(hwnd, Rc::new(RefCell::new(Stub(ControlKind::TreeView))));
    let header = nmhdr(hwnd, NM_CLICK);
    let lparam = LPARAM(&header as *const NMHDR as isize);
    let result = decode_notify(lparam);
    registry::unregister(hwnd);
    assert!(matches!(
        result,
        Notify::TreeView {
            event: TreeViewEvent::Click,
            ..
        }
    ));
}
