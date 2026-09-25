//! Raw `EDIT` control messages and its Enter-submit subclass.
//!
//! The system edit class is `EDIT`; its messages are the `EM_*` family
//! (`WinUser.h`), sent through `SendMessageW` like the other `sys` control
//! helpers. An edit reports its changes as `WM_COMMAND` notifications to its
//! parent (`EN_CHANGE`, `EN_SETFOCUS`, `EN_KILLFOCUS`); only the Enter key
//! needs a subclass, because a single-line edit does not announce it as a
//! notification.

use std::ops::Range;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Controls::{
    EM_GETSEL, EM_LIMITTEXT, EM_REPLACESEL, EM_SETCUEBANNER, EM_SETREADONLY, EM_SETSEL,
};
use windows::Win32::UI::Input::KeyboardAndMouse::VK_RETURN;
use windows::Win32::UI::Shell::DefSubclassProc;
use windows::Win32::UI::WindowsAndMessaging::{
    ES_AUTOHSCROLL, ES_AUTOVSCROLL, ES_MULTILINE, ES_NUMBER, ES_PASSWORD, GWL_STYLE,
    GetWindowLongPtrW, SWP_FRAMECHANGED, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SendMessageW,
    SetWindowLongPtrW, SetWindowPos, WM_GETTEXT, WM_GETTEXTLENGTH, WM_KEYDOWN, WM_SETTEXT,
};

use crate::hwnd::Hwnd;

use super::{raw_hwnd, window};

// `ES_*` style bits, from `WinUser.h` (via the `windows` crate). Re-exported so
// the widget layer composes the creation style without naming `windows` types.
pub(crate) const STYLE_MULTILINE: u32 = ES_MULTILINE as u32;
pub(crate) const STYLE_PASSWORD: u32 = ES_PASSWORD as u32;
pub(crate) const STYLE_AUTOHSCROLL: u32 = ES_AUTOHSCROLL as u32;
pub(crate) const STYLE_AUTOVSCROLL: u32 = ES_AUTOVSCROLL as u32;
pub(crate) const STYLE_NUMBER: u32 = ES_NUMBER as u32;

fn send(hwnd: Hwnd, msg: u32, wparam: usize, lparam: isize) -> isize {
    // SAFETY: only integer values are forwarded; the caller guarantees any
    // pointer in `lparam` points at a valid buffer for the duration.
    unsafe {
        SendMessageW(
            raw_hwnd(hwnd),
            msg,
            Some(WPARAM(wparam)),
            Some(LPARAM(lparam)),
        )
        .0
    }
}

/// The edit's whole text, as Win32 stores it (multi-line text keeps its `CRLF`
/// line endings; normalising those is the widget layer's job).
pub(crate) fn text(hwnd: Hwnd) -> String {
    let length = send(hwnd, WM_GETTEXTLENGTH, 0, 0).max(0) as usize;
    let mut buffer = vec![0u16; length + 1];
    // SAFETY: `buffer` holds `length + 1` writable u16s, room for the text and
    // the terminator the edit writes.
    let copied = send(hwnd, WM_GETTEXT, buffer.len(), buffer.as_mut_ptr() as isize);
    if copied <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buffer[..(copied as usize).min(buffer.len())])
}

/// Replaces the edit's text.
pub(crate) fn set_text(hwnd: Hwnd, text: &str) {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `wide` is a nul-terminated UTF-16 buffer alive across the call.
    send(hwnd, WM_SETTEXT, 0, wide.as_ptr() as isize);
}

/// The cue banner set with [`set_cue`], or empty (`EM_GETCUEBANNER`,
/// `CommCtrl.h`: `ECM_FIRST + 2`).
pub(crate) fn cue(hwnd: Hwnd) -> String {
    const EM_GETCUEBANNER: u32 = 0x1500 + 2;
    let mut buffer = vec![0u16; 256];
    // SAFETY: `buffer` holds 256 writable u16s; `EM_GETCUEBANNER` takes the
    // buffer in `wparam` and its length in characters in `lparam`.
    let ok = send(
        hwnd,
        EM_GETCUEBANNER,
        buffer.as_mut_ptr() as usize,
        buffer.len() as isize,
    );
    if ok == 0 {
        return String::new();
    }
    let end = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}

/// Shows `text` as grey placeholder text while the edit is empty.
pub(crate) fn set_cue(hwnd: Hwnd, text: &str, show_when_focused: bool) {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `wide` is a nul-terminated UTF-16 buffer alive across the call.
    send(
        hwnd,
        EM_SETCUEBANNER,
        usize::from(show_when_focused),
        wide.as_ptr() as isize,
    );
}

/// Turns read-only mode on or off (`EM_SETREADONLY`).
pub(crate) fn set_read_only(hwnd: Hwnd, read_only: bool) {
    send(hwnd, EM_SETREADONLY, usize::from(read_only), 0);
}

/// Limits the edit's text to `max` characters (`EM_LIMITTEXT`).
pub(crate) fn limit_text(hwnd: Hwnd, max: usize) {
    send(hwnd, EM_LIMITTEXT, max, 0);
}

/// Selects the whole text (`EM_SETSEL` with a `-1` end).
pub(crate) fn select_all(hwnd: Hwnd) {
    send(hwnd, EM_SETSEL, 0, -1);
}

/// The current selection, in UTF-16 code units.
pub(crate) fn selection(hwnd: Hwnd) -> Range<usize> {
    let mut start = 0u32;
    let mut end = 0u32;
    // SAFETY: both out-pointers are valid `u32`s alive across the call;
    // `EM_GETSEL` only writes them.
    send(
        hwnd,
        EM_GETSEL,
        &mut start as *mut u32 as usize,
        &mut end as *mut u32 as isize,
    );
    start as usize..end as usize
}

/// Replaces the selection with `range` (`EM_SETSEL`). An end of `-1` is the
/// documented "to the end of the text" sentinel.
pub(crate) fn set_selection(hwnd: Hwnd, selection: Range<usize>) {
    send(hwnd, EM_SETSEL, selection.start, selection.end as isize);
}

/// Replaces the current selection (or inserts at the caret) with `text`,
/// recording an undo action.
pub(crate) fn replace_selection(hwnd: Hwnd, text: &str) {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `wide` is a nul-terminated UTF-16 buffer alive across the call.
    send(hwnd, EM_REPLACESEL, 1, wide.as_ptr() as isize);
}

/// The window's cached style bits (`GWL_STYLE`).
fn style_bits(hwnd: Hwnd) -> u32 {
    // SAFETY: `GWL_STYLE` only reads the window's own style bits.
    unsafe { GetWindowLongPtrW(raw_hwnd(hwnd), GWL_STYLE) as u32 }
}

/// Flips one style bit and refreshes the non-client frame, so a change made
/// after creation (e.g. `ES_NUMBER`) takes effect.
fn set_style_bit(hwnd: Hwnd, bit: u32, on: bool) {
    let current = style_bits(hwnd);
    let next = if on { current | bit } else { current & !bit };
    if next == current {
        return;
    }
    // SAFETY: `GWL_STYLE` writes the window's own style bits; a stale handle is
    // a documented no-op.
    unsafe { SetWindowLongPtrW(raw_hwnd(hwnd), GWL_STYLE, next as isize) };
    // SAFETY: only the frame is recomputed; the position, size and z-order
    // flags keep the window otherwise unchanged.
    unsafe {
        let _ = SetWindowPos(
            raw_hwnd(hwnd),
            None,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
        );
    }
}

/// Turns `ES_NUMBER` on or off: the edit rejects non-digits typed by the user.
pub(crate) fn set_number_only(hwnd: Hwnd, number_only: bool) {
    set_style_bit(hwnd, STYLE_NUMBER, number_only);
}

/// Turns word wrap on or off for a multi-line edit. Wrapping means the edit
/// has no horizontal scroll bar, i.e. `ES_AUTOHSCROLL` is *cleared*.
pub(crate) fn set_word_wrap(hwnd: Hwnd, wrap: bool) {
    set_style_bit(hwnd, STYLE_AUTOHSCROLL, !wrap);
}

/// State of an installed [`ReturnSubclass`].
struct ReturnData {
    on_return: Box<dyn Fn()>,
}

/// Watches a single-line edit for Enter and calls `on_return` when it is
/// pressed, consuming the key so the edit does not beep.
///
/// A single-line edit has no `EN_*` code for Enter, so the widget layer needs
/// this subclass; it is only installed on single-line edits, where Enter means
/// "submit" rather than "new line".
pub(crate) struct ReturnSubclass {
    hwnd: Hwnd,
    raw: *mut ReturnData,
}

const RETURN_SUBCLASS_ID: usize = 0x7765_6474; // "wedt"

impl ReturnSubclass {
    /// Subclasses `hwnd` so Enter calls `on_return`. Returns `None` when the
    /// subclass could not be installed.
    pub(crate) fn install(hwnd: Hwnd, on_return: Box<dyn Fn()>) -> Option<ReturnSubclass> {
        let raw = Box::into_raw(Box::new(ReturnData { on_return }));
        if !window::set_subclass(hwnd, Some(return_proc), RETURN_SUBCLASS_ID, raw as usize) {
            // SAFETY: install failed before the subclass could adopt the state,
            // so this is the only reference to it.
            unsafe { drop(Box::from_raw(raw)) };
            return None;
        }
        Some(ReturnSubclass { hwnd, raw })
    }
}

impl Drop for ReturnSubclass {
    fn drop(&mut self) {
        window::remove_subclass(self.hwnd, Some(return_proc), RETURN_SUBCLASS_ID);
        // SAFETY: `raw` came from `Box::into_raw` in `install`; removing the
        // subclass means no further callback can reach it, so this is the only
        // reference and it is reclaimed exactly once.
        unsafe { drop(Box::from_raw(self.raw)) };
    }
}

/// The subclass procedure installed by [`ReturnSubclass::install`].
///
/// # Safety
/// Called by Windows for the subclass installed by `install`; `refdata` is the
/// `ReturnData` pointer it installed, valid until `remove_subclass`.
unsafe extern "system" fn return_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    refdata: usize,
) -> LRESULT {
    if msg == WM_KEYDOWN && wparam.0 as u16 == VK_RETURN.0 {
        // SAFETY: `refdata` is the live `ReturnData` installed by `install`; it
        // is freed only after `remove_subclass` in `ReturnSubclass::drop`.
        let data = unsafe { &*(refdata as *const ReturnData) };
        // A panic unwinding across this `extern "system"` boundary is undefined
        // behaviour; isolate it instead of letting it propagate.
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (data.on_return)())).ok();
        return LRESULT(0);
    }
    // SAFETY: forward to the subclass chain's original window procedure.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}
