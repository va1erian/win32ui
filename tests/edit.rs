//! Edit behaviour that needs a real window: the UTF-16 text round trip,
//! `CRLF`/`LF` normalisation for multi-line text, selection editing and the
//! size/limit builders.

#![cfg(windows)]

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::run_app_with_watchdog;
use win32ui::prelude::*;

// For testing WS_EX_CLIENTEDGE flag.
use core::ffi::c_void;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{GWL_EXSTYLE, GetWindowLongPtrW, WS_EX_CLIENTEDGE};

enum Msg {
    Start,
}

struct EditApp;

impl App for EditApp {
    type Msg = Msg;

    fn update(&mut self, _msg: Msg, ui: &mut Ui<Msg>) {
        ui.quit();
    }
}

/// Runs `f` against a fresh widget-layer window and reports whether it passed
/// its assertions. `None` when the session cannot create windows at all.
fn check(name: &str, f: impl FnOnce(&mut Ui<Msg>) -> bool + 'static) -> Option<bool> {
    let passed = Rc::new(Cell::new(false));
    let passed_for_make = Rc::clone(&passed);
    let run = run_app_with_watchdog(name, move |ui| {
        passed_for_make.set(f(ui));
        ui.emit(Msg::Start);
        EditApp
    })?;
    assert!(!run.timed_out, "the watchdog fired before the app quit");
    Some(passed.get())
}

/// A single-line edit round-trips CJK, astral-plane emoji and combining marks,
/// and supports selection editing.
#[test]
fn single_line_text_and_selection_round_trip() {
    let Some(passed) = check("win32ui.edit.text", |ui| {
        let Ok(edit) = Edit::single_line(ui) else {
            return false;
        };
        let mut ok = true;

        edit.set_text("日本語 🎵 e\u{301}");
        ok &= edit.text() == "日本語 🎵 e\u{301}";
        ok &= edit.selection() == (0..0);

        edit.set_text("hello world");
        edit.select_all();
        ok &= edit.selection() == (0..11);
        edit.set_selection(0..5);
        ok &= edit.selection() == (0..5);
        edit.replace_selection("bye");
        ok &= edit.text() == "bye world";

        ok
    }) else {
        return;
    };
    assert!(passed, "single-line text or selection round trip failed");
}

/// Multi-line text is normalised: the widget takes and returns `LF`, the native
/// control stores `CRLF`.
#[test]
fn multi_line_normalises_newlines() {
    let Some(passed) = check("win32ui.edit.multiline", |ui| {
        let Ok(edit) = Edit::multi_line(ui) else {
            return false;
        };
        let mut ok = true;
        ok &= edit.is_multiline();

        edit.set_text("a\nb\nc");
        ok &= edit.text() == "a\nb\nc";

        edit.set_text("日本語\n🎵\ne\u{301}\n");
        ok &= edit.text() == "日本語\n🎵\ne\u{301}\n";

        edit.set_word_wrap(false);
        edit.set_word_wrap(true);
        ok &= edit.text() == "日本語\n🎵\ne\u{301}\n";

        ok
    }) else {
        return;
    };
    assert!(passed, "multi-line newline normalisation failed");
}

/// `EM_LIMITTEXT` caps the text, and the width/read-only/number builders are
/// usable after construction.
#[test]
fn builders_limit_the_text() {
    let Some(passed) = check("win32ui.edit.builders", |ui| {
        let Ok(edit) = Edit::single_line(ui) else {
            return false;
        };
        let edit = edit
            .cue("Search")
            .max_length(3)
            .read_only(false)
            .number_only(true);
        // `EM_LIMITTEXT` caps text a user inserts; `WM_SETTEXT` (used by
        // `set_text`) bypasses it, so insert through the selection instead.
        edit.set_selection(0..0);
        edit.replace_selection("abcdef");
        edit.text().chars().count() == 3
    }) else {
        return;
    };
    assert!(passed, "the text limit or a builder did not apply");
}

/// A password edit is the native `EDIT` with `ES_PASSWORD`; `HasText` still
/// reads the real text.
#[test]
fn password_edit_holds_text() {
    let Some(passed) = check("win32ui.edit.password", |ui| {
        let Ok(edit) = Edit::password(ui) else {
            return false;
        };
        edit.set_text("hunter2");
        edit.text() == "hunter2"
    }) else {
        return;
    };
    assert!(passed, "the password edit did not keep its text");
}

/// Helper to check if a control has the WS_EX_CLIENTEDGE flag set.
fn has_client_edge(hwnd: win32ui::Hwnd) -> bool {
    // SAFETY: GetWindowLongPtrW only reads the window's extended style bits.
    let style = unsafe { GetWindowLongPtrW(HWND(hwnd.raw() as *mut c_void), GWL_EXSTYLE) } as u32;
    style & WS_EX_CLIENTEDGE.0 != 0
}

/// Multi-line edit has WS_EX_CLIENTEDGE set in light mode and cleared in dark
/// mode. Single-line edit never has it. Switching theme at runtime toggles it.
#[test]
fn multi_line_client_edge_follows_theme() {
    let Some(passed) = check("win32ui.edit.client_edge", |ui| {
        let Ok(single) = Edit::single_line(ui) else {
            return false;
        };
        let Ok(multi) = Edit::multi_line(ui) else {
            return false;
        };
        let mut ok = true;

        // Single-line edit never has client edge.
        ok &= !has_client_edge(single.control().hwnd());

        // Multi-line edit starts with light theme, so should have client edge.
        ok &= has_client_edge(multi.control().hwnd());

        // Applying dark theme removes client edge from multi-line edit.
        let dark = Theme::dark();
        multi.apply_theme(&dark);
        ok &= !has_client_edge(multi.control().hwnd());

        // Applying light theme restores client edge on multi-line edit.
        let light = Theme::light();
        multi.apply_theme(&light);
        ok &= has_client_edge(multi.control().hwnd());

        // Single-line edit stays without client edge after theme change.
        single.apply_theme(&dark);
        ok &= !has_client_edge(single.control().hwnd());

        ok
    }) else {
        return;
    };
    assert!(
        passed,
        "multi-line client edge did not follow theme correctly"
    );
}
