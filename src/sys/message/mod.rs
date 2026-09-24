//! Raw-message decoding: the typed [`Message`] behind each Win32 message.

mod draw;
mod input;
mod measure;
mod notify;

use std::cell::Cell;
use std::sync::OnceLock;

use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::UI::Controls::NMHDR;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    MINMAXINFO, RegisterWindowMessageW, WM_CHAR, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_DESTROY,
    WM_DPICHANGED, WM_GETMINMAXINFO, WM_NOTIFY, WM_PAINT, WM_SETTINGCHANGE, WM_SIZE, WM_SYSCHAR,
    WM_SYSCOLORCHANGE, WM_THEMECHANGED, WM_TIMER,
};
use windows::core::{PCWSTR, w};

use crate::geometry::{Point, Size};
use crate::message::{Command, CommandNotification, Message, MinMaxInfo, Modifiers, TimerId};

use super::hwnd_from;

use notify::decode_notify;

pub(crate) use draw::{decode_draw, message_id as draw_message_id};
pub(crate) use measure::{message_id as measure_message_id, set_size as set_measured_size};

/// Name of the message a worker thread posts to wake the UI.
const WAKE_MESSAGE_NAME: PCWSTR = w!("emusic.win32ui.wake");

/// The process-wide id of the registered "wake" message (0 if unavailable).
pub(crate) fn wake_message() -> u32 {
    static ID: OnceLock<u32> = OnceLock::new();
    *ID.get_or_init(|| {
        // SAFETY: the string is a static, nul-terminated wide literal.
        unsafe { RegisterWindowMessageW(WAKE_MESSAGE_NAME) }
    })
}

/// Name of the private message that nudges the widget layer to drain its queue.
const DRAIN_MESSAGE_NAME: PCWSTR = w!("emusic.win32ui.drain");

/// The process-wide id of the registered "drain" message (0 if unavailable).
pub(crate) fn drain_message() -> u32 {
    static ID: OnceLock<u32> = OnceLock::new();
    *ID.get_or_init(|| {
        // SAFETY: the string is a static, nul-terminated wide literal.
        unsafe { RegisterWindowMessageW(DRAIN_MESSAGE_NAME) }
    })
}

thread_local! {
    /// High half of a `WM_CHAR` surrogate pair, waiting for its low half.
    static PENDING_HIGH_SURROGATE: Cell<Option<u16>> = const { Cell::new(None) };
    /// The `MINMAXINFO*` of the `WM_GETMINMAXINFO` being handled, if any.
    static MIN_MAX: Cell<isize> = const { Cell::new(0) };
}

/// Reads the `NMHDR` at the head of a `WM_NOTIFY` `lparam`.
pub(crate) fn notify_header(lparam: LPARAM) -> Option<(HWND, usize, u32)> {
    if lparam.0 == 0 {
        return None;
    }
    // SAFETY: for WM_NOTIFY, lparam points to an NMHDR (or a struct beginning
    // with one) owned by the sender for the duration of the message.
    let header = read::<NMHDR>(lparam);
    Some((header.hwndFrom, header.idFrom, header.code))
}

/// Decodes a raw message into the typed [`Message`] enum.
///
/// Returns `None` when the message carries no complete message (the first half
/// of a `WM_CHAR` surrogate pair), so the caller skips it.
pub(crate) fn decode(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<Message> {
    // A `MINMAXINFO*` is only valid for the duration of its own message.
    MIN_MAX.with(|slot| slot.set(0));
    measure::reset();

    if msg == WM_GETMINMAXINFO {
        if lparam.0 != 0 {
            MIN_MAX.with(|slot| slot.set(lparam.0));
        }
        return Some(Message::GetMinMaxInfo);
    }
    if msg == measure_message_id() {
        return measure::decode(lparam);
    }
    if msg == WM_SETTINGCHANGE {
        return Some(Message::SettingChange {
            section: read_wide_string(lparam),
        });
    }
    if msg == WM_SYSCOLORCHANGE {
        return Some(Message::SysColorChange);
    }
    if msg == WM_THEMECHANGED {
        return Some(Message::ThemeChanged);
    }
    if msg == WM_CHAR || msg == WM_SYSCHAR {
        let mut pending = PENDING_HIGH_SURROGATE.with(|slot| slot.get());
        let decoded = input::decode_char(wparam.0 as u16, &mut pending);
        PENDING_HIGH_SURROGATE.with(|slot| slot.set(pending));
        return decoded.map(Message::Char);
    }
    if let Some(message) = input::decode_input(msg, wparam.0, lparam.0, keyboard_modifiers()) {
        // Wheel coordinates arrive in screen space; the public message is in
        // client coordinates.
        if let Message::MouseWheel {
            delta,
            horizontal,
            x,
            y,
            modifiers,
        } = message
        {
            let client = screen_to_client(hwnd, x, y);
            return Some(Message::MouseWheel {
                delta,
                horizontal,
                x: client.x,
                y: client.y,
                modifiers,
            });
        }
        return Some(message);
    }

    let lo = |value: isize| (value & 0xffff) as i16 as i32;
    let hi = |value: isize| ((value >> 16) & 0xffff) as i16 as i32;

    if msg == WM_CREATE {
        Some(Message::Create)
    } else if msg == WM_DESTROY {
        Some(Message::Destroy)
    } else if msg == WM_CLOSE {
        Some(Message::Close)
    } else if msg == WM_PAINT {
        Some(Message::Paint)
    } else if msg == WM_SIZE {
        Some(Message::Size {
            width: lo(lparam.0),
            height: hi(lparam.0),
        })
    } else if msg == WM_TIMER {
        Some(Message::Timer {
            id: TimerId(wparam.0),
        })
    } else if msg == wake_message() {
        Some(Message::Wake)
    } else if msg == WM_COMMAND {
        let control = if lparam.0 == 0 {
            None
        } else {
            Some(hwnd_from(HWND(lparam.0 as *mut core::ffi::c_void)))
        };
        Some(Message::Command(Command {
            id: (wparam.0 & 0xffff) as u16,
            control,
            notification: CommandNotification::from_code(((wparam.0 >> 16) & 0xffff) as u16),
        }))
    } else if msg == WM_NOTIFY {
        Some(Message::Notify(decode_notify(lparam)))
    } else if msg == WM_DPICHANGED {
        Some(Message::DpiChanged {
            dpi: (wparam.0 & 0xffff) as u32,
            suggested: if lparam.0 == 0 {
                crate::geometry::Rect::default()
            } else {
                let rect = read::<windows::Win32::Foundation::RECT>(lparam);
                crate::geometry::Rect::new(rect.left, rect.top, rect.right, rect.bottom)
            },
        })
    } else {
        Some(Message::Other {
            code: msg,
            wparam: wparam.0,
            lparam: lparam.0,
        })
    }
}

/// The modifier keys currently held, read from the thread's key state.
fn keyboard_modifiers() -> Modifiers {
    // SAFETY: `GetKeyState` only reads the calling thread's keyboard state and
    // takes no pointer.
    let down = |key: i32| unsafe { GetKeyState(key) < 0 };
    Modifiers {
        ctrl: down(VK_CONTROL.0 as i32),
        shift: down(VK_SHIFT.0 as i32),
        alt: down(VK_MENU.0 as i32),
        win: down(VK_LWIN.0 as i32) || down(VK_RWIN.0 as i32),
    }
}

/// Converts a screen-space point to the client area of `hwnd`.
fn screen_to_client(hwnd: HWND, x: i32, y: i32) -> Point {
    let mut point = POINT { x, y };
    // SAFETY: `point` is a valid in/out POINT; the call only rewrites it.
    unsafe {
        let _ = ScreenToClient(hwnd, &mut point);
    }
    Point::new(point.x, point.y)
}

/// Reads a `WM_SETTINGCHANGE` `lparam` wide string, if present.
fn read_wide_string(lparam: LPARAM) -> Option<String> {
    if lparam.0 == 0 {
        return None;
    }
    // SAFETY: for WM_SETTINGCHANGE, lparam is a nul-terminated wide string
    // owned by the system for the duration of the message. It is scanned only
    // up to (and excluding) the nul terminator.
    unsafe {
        let start = lparam.0 as *const u16;
        let mut length = 0usize;
        while *start.add(length) != 0 {
            length += 1;
        }
        if length == 0 {
            None
        } else {
            Some(String::from_utf16_lossy(std::slice::from_raw_parts(
                start, length,
            )))
        }
    }
}

/// The size limits reported by the `WM_GETMINMAXINFO` currently being handled,
/// if any.
pub(crate) fn read_min_max_info() -> Option<MinMaxInfo> {
    let pointer = MIN_MAX.with(|slot| slot.get());
    if pointer == 0 {
        return None;
    }
    // SAFETY: set from a WM_GETMINMAXINFO lparam and valid until that message
    // returns; MINMAXINFO is exactly the struct it points at.
    let info = unsafe { &*(pointer as *const MINMAXINFO) };
    Some(MinMaxInfo {
        max_size: Size::new(info.ptMaxSize.x, info.ptMaxSize.y),
        max_position: Point::new(info.ptMaxPosition.x, info.ptMaxPosition.y),
        min_track_size: Size::new(info.ptMinTrackSize.x, info.ptMinTrackSize.y),
        max_track_size: Size::new(info.ptMaxTrackSize.x, info.ptMaxTrackSize.y),
    })
}

/// Writes the size limits back to the `WM_GETMINMAXINFO` currently being
/// handled, if any.
pub(crate) fn write_min_max_info(value: MinMaxInfo) {
    let pointer = MIN_MAX.with(|slot| slot.get());
    if pointer == 0 {
        return;
    }
    // SAFETY: as in `read_min_max_info`; only the fields we expose are written.
    let info = unsafe { &mut *(pointer as *mut MINMAXINFO) };
    info.ptMaxSize = POINT {
        x: value.max_size.width,
        y: value.max_size.height,
    };
    info.ptMaxPosition = POINT {
        x: value.max_position.x,
        y: value.max_position.y,
    };
    info.ptMinTrackSize = POINT {
        x: value.min_track_size.width,
        y: value.min_track_size.height,
    };
    info.ptMaxTrackSize = POINT {
        x: value.max_track_size.width,
        y: value.max_track_size.height,
    };
}

/// Copies the notification struct `lparam` points at. The caller must know
/// which struct matches the `NMHDR.code` it just checked.
fn read<T: Copy>(lparam: LPARAM) -> T {
    // SAFETY: callers only use this for the struct matching the message they
    // are handling; the pointer is valid for the duration of the call.
    unsafe { *(lparam.0 as *const T) }
}
