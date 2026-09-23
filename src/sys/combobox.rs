//! Raw `COMBOBOX` messages.
//!
//! The system combo class is `COMBOBOX`; its messages are the `CB_*` family
//! (`WinUser.h`) plus `CB_SETMINVISIBLE` (`CommCtrl.h`), sent through
//! `SendMessageW` like the other `sys` control helpers.

use windows::Win32::UI::WindowsAndMessaging::{
    CB_ADDSTRING, CB_GETCURSEL, CB_GETITEMHEIGHT, CB_GETLBTEXT, CB_GETLBTEXTLEN, CB_RESETCONTENT,
    CB_SETCURSEL, CB_SHOWDROPDOWN, SendMessageW,
};

use crate::hwnd::Hwnd;

use super::raw_hwnd;

/// `CB_SETMINVISIBLE`: the minimum number of visible items in a dropped list.
/// Mirrored from `CommCtrl.h` (`0x1701`), which the `windows` crate does not
/// expose.
const CB_SETMINVISIBLE: u32 = 0x1701;

/// `CB_ERR`: the sentinel a combo box returns for an out-of-range request.
const CB_ERR: isize = -1;

fn send(hwnd: Hwnd, msg: u32, wparam: usize, lparam: isize) -> isize {
    // SAFETY: only integer values are forwarded; the caller guarantees any
    // pointer in `lparam` points at a valid buffer for the duration.
    unsafe {
        SendMessageW(
            raw_hwnd(hwnd),
            msg,
            Some(windows::Win32::Foundation::WPARAM(wparam)),
            Some(windows::Win32::Foundation::LPARAM(lparam)),
        )
        .0
    }
}

/// Empties the combo box's item list.
pub(crate) fn cb_reset_content(hwnd: Hwnd) {
    send(hwnd, CB_RESETCONTENT, 0, 0);
}

/// Appends `text` to the item list, returning its index or `None` on failure.
pub(crate) fn cb_add_string(hwnd: Hwnd, text: &str) -> Option<usize> {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `wide` is a nul-terminated UTF-16 buffer alive across the call.
    let result = send(hwnd, CB_ADDSTRING, 0, wide.as_ptr() as isize);
    if result == CB_ERR {
        None
    } else {
        Some(result as usize)
    }
}

/// The index of the current selection, or `None` when nothing is selected.
pub(crate) fn cb_get_cur_sel(hwnd: Hwnd) -> Option<usize> {
    let index = send(hwnd, CB_GETCURSEL, 0, 0);
    if index == CB_ERR || index < 0 {
        None
    } else {
        Some(index as usize)
    }
}

/// Selects `index`, or clears the selection with `None`.
pub(crate) fn cb_set_cur_sel(hwnd: Hwnd, index: Option<usize>) {
    let value = index.unwrap_or(CB_ERR as usize);
    send(hwnd, CB_SETCURSEL, value, 0);
}

/// Sets the minimum number of items shown in the dropped list, so its height
/// is sensible at any DPI rather than relying on a fixed pixel count.
pub(crate) fn cb_set_min_visible(hwnd: Hwnd, count: usize) {
    send(hwnd, CB_SETMINVISIBLE, count, 0);
}

/// The height of the item at `index`, or of the selection field for a negative
/// `index`. Used to size the closed control to its font.
pub(crate) fn cb_item_height(hwnd: Hwnd, index: i32) -> i32 {
    send(hwnd, CB_GETITEMHEIGHT, index as usize, 0) as i32
}

/// Shows or hides the dropped list (used by the demo to screenshot it open).
pub(crate) fn cb_show_drop_down(hwnd: Hwnd, show: bool) {
    send(hwnd, CB_SHOWDROPDOWN, usize::from(show), 0);
}

/// Reads back the text of the item at `index` (which re-enters the combo's
/// storage). Useful for tests and accessibility.
pub(crate) fn cb_item_text(hwnd: Hwnd, index: usize) -> String {
    let length = send(hwnd, CB_GETLBTEXTLEN, index, 0);
    if length == CB_ERR || length < 0 {
        return String::new();
    }
    let mut buffer = vec![0u16; length as usize + 1];
    // SAFETY: `buffer` is large enough for `length` UTF-16 code units plus the
    // terminator the combo writes.
    let copied = send(hwnd, CB_GETLBTEXT, index, buffer.as_mut_ptr() as isize);
    if copied == CB_ERR {
        return String::new();
    }
    let length = (copied as usize).min(buffer.len());
    String::from_utf16_lossy(&buffer[..length])
}
