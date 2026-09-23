//! Decoding of `WM_MEASUREITEM` requests for owner-drawn controls and menus.

use std::cell::Cell;

use windows::Win32::Foundation::LPARAM;
use windows::Win32::UI::Controls::{MEASUREITEMSTRUCT, ODT_MENU};
use windows::Win32::UI::WindowsAndMessaging::WM_MEASUREITEM;

use crate::geometry::Size;
use crate::message::Message;

thread_local! {
    /// The `MEASUREITEMSTRUCT*` of the `WM_MEASUREITEM` being handled, if any.
    /// Like `MINMAXINFO`, it is only valid for the duration of its message.
    static MEASURE: Cell<isize> = const { Cell::new(0) };
}

/// The raw id of `WM_MEASUREITEM` (from `WinUser.h` via the `windows` crate).
pub(crate) fn message_id() -> u32 {
    WM_MEASUREITEM
}

/// Clears the cached `MEASUREITEMSTRUCT*` at the start of every decode, so a
/// stale pointer can never be written through.
pub(crate) fn reset() {
    MEASURE.with(|slot| slot.set(0));
}

/// Decodes a `WM_MEASUREITEM` into [`Message::MeasureItem`], or `None` for a
/// null payload.
pub(crate) fn decode(lparam: LPARAM) -> Option<Message> {
    if lparam.0 == 0 {
        return None;
    }
    // SAFETY: for `WM_MEASUREITEM`, `lparam` points at a `MEASUREITEMSTRUCT`
    // owned by the system for the duration of the message.
    let info = unsafe { &*(lparam.0 as *const MEASUREITEMSTRUCT) };
    MEASURE.with(|slot| slot.set(lparam.0));
    Some(Message::MeasureItem {
        id: info.CtlID as usize,
        item: info.itemID as usize,
        data: info.itemData,
        menu: info.CtlType == ODT_MENU,
    })
}

/// Records the size the current `WM_MEASUREITEM` should report. A no-op outside
/// that message.
pub(crate) fn set_size(size: Size) {
    let pointer = MEASURE.with(|slot| slot.get());
    if pointer == 0 {
        return;
    }
    // SAFETY: set from a WM_MEASUREITEM lparam and valid until that message
    // returns; only the width/height fields we own are written.
    let info = unsafe { &mut *(pointer as *mut MEASUREITEMSTRUCT) };
    info.itemWidth = size.width.max(0) as u32;
    info.itemHeight = size.height.max(0) as u32;
}
