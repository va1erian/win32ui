//! Decoding of `WM_DRAWITEM` requests for owner-drawn controls.

use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::Controls::DRAWITEMSTRUCT;
use windows::Win32::UI::WindowsAndMessaging::WM_DRAWITEM;

use crate::message::Message;

use super::hwnd_from;

/// The raw id of `WM_DRAWITEM` (from `WinUser.h` via the `windows` crate).
pub(crate) fn message_id() -> u32 {
    WM_DRAWITEM
}

/// Decodes a `WM_DRAWITEM` into [`Message::DrawItem`], or `None` for a null
/// payload. The device context stays valid only while handling the message.
pub(crate) fn decode_draw(wparam: WPARAM, lparam: LPARAM) -> Option<Message> {
    if lparam.0 == 0 {
        return None;
    }
    let request = crate::sys::button_draw::draw_request(lparam.0)?;
    // SAFETY: for `WM_DRAWITEM`, `lparam` points at a `DRAWITEMSTRUCT` owned
    // by the system for the duration of the message; only `itemAction` is
    // read here, the rest was copied by `draw_request`.
    let action = unsafe { (&*(lparam.0 as *const DRAWITEMSTRUCT)).itemAction.0 };
    Some(Message::DrawItem {
        control: hwnd_from(request.hwnd),
        id: wparam.0,
        item: request.item_id,
        data: request.item_data,
        menu: request.control_type == windows::Win32::UI::Controls::ODT_MENU.0,
        action,
        state: request.state,
        dc: request.hdc,
        area: request.rect,
    })
}
