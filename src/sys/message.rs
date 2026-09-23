//! Message pump and raw-message decoding.

use std::sync::OnceLock;

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Controls::{
    LVN_COLUMNCLICK, LVN_ITEMCHANGED, LVN_KEYDOWN, NM_CLICK, NM_DBLCLK, NM_RCLICK, NM_RETURN,
    NMHDR, NMITEMACTIVATE, NMLISTVIEW, NMLVKEYDOWN, NMTREEVIEWW, TVN_ITEMEXPANDED, TVN_SELCHANGED,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, MSG, PostQuitMessage, RegisterWindowMessageW, TranslateMessage,
    WM_CLOSE, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_DPICHANGED, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEMOVE, WM_NOTIFY, WM_PAINT, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SIZE, WM_TIMER,
};
use windows::core::{PCWSTR, w};

use crate::controls::listview::ListViewEvent;
use crate::controls::registry::{self, ControlKind};
use crate::controls::treeview::TreeViewEvent;
use crate::geometry::Rect;
use crate::message::{Command, CommandNotification, Message, MouseButton, Notify, TimerId};

use super::hwnd_from;

/// Name of the message a worker thread posts to wake the UI.
const WAKE_MESSAGE_NAME: PCWSTR = w!("emusic.win32ui.wake");

/// The outcome of pumping one message.
pub(crate) enum Pumped {
    /// A message was retrieved (and dispatched).
    Message,
    /// `WM_QUIT` was received; carries the exit code.
    Quit(i32),
    /// `GetMessageW` failed.
    Error,
}

/// Retrieves and dispatches a single message. Blocks until one is available.
pub(crate) fn pump() -> Pumped {
    let mut msg = MSG::default();
    // SAFETY: `msg` is a valid, aligned out-pointer and `GetMessageW` fully
    // initialises it before returning a positive value.
    let result = unsafe { GetMessageW(&mut msg, None, 0, 0) };
    match result.0 {
        0 => return Pumped::Quit(msg.wParam.0 as i32),
        -1 => return Pumped::Error,
        _ => {}
    }
    // SAFETY: the message just retrieved is translated and dispatched under the
    // standard WndProc contract; both calls only read `msg`.
    unsafe {
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
    Pumped::Message
}

/// Ends the message loop with `code`.
pub(crate) fn post_quit(code: i32) {
    // SAFETY: `PostQuitMessage` takes no pointers and only touches the calling
    // thread's message queue.
    unsafe { PostQuitMessage(code) };
}

/// The process-wide id of the registered "wake" message (0 if unavailable).
pub(crate) fn wake_message() -> u32 {
    static ID: OnceLock<u32> = OnceLock::new();
    *ID.get_or_init(|| {
        // SAFETY: the string is a static, nul-terminated wide literal.
        unsafe { RegisterWindowMessageW(WAKE_MESSAGE_NAME) }
    })
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
pub(crate) fn decode(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Message {
    let _ = hwnd;
    let lo = |value: isize| (value & 0xffff) as i16 as i32;
    let hi = |value: isize| ((value >> 16) & 0xffff) as i16 as i32;

    if msg == WM_CREATE {
        Message::Create
    } else if msg == WM_DESTROY {
        Message::Destroy
    } else if msg == WM_CLOSE {
        Message::Close
    } else if msg == WM_PAINT {
        Message::Paint
    } else if msg == WM_SIZE {
        Message::Size {
            width: lo(lparam.0),
            height: hi(lparam.0),
        }
    } else if msg == WM_TIMER {
        Message::Timer {
            id: TimerId(wparam.0),
        }
    } else if msg == wake_message() {
        Message::Wake
    } else if msg == WM_COMMAND {
        let control = if lparam.0 == 0 {
            None
        } else {
            Some(hwnd_from(HWND(lparam.0 as *mut core::ffi::c_void)))
        };
        Message::Command(Command {
            id: (wparam.0 & 0xffff) as u16,
            control,
            notification: CommandNotification::from_code(((wparam.0 >> 16) & 0xffff) as u16),
        })
    } else if msg == WM_NOTIFY {
        Message::Notify(decode_notify(lparam))
    } else if msg == WM_DPICHANGED {
        Message::DpiChanged {
            dpi: (wparam.0 & 0xffff) as u32,
            suggested: if lparam.0 == 0 {
                Rect::default()
            } else {
                let rect = read::<windows::Win32::Foundation::RECT>(lparam);
                Rect::new(rect.left, rect.top, rect.right, rect.bottom)
            },
        }
    } else if msg == WM_LBUTTONDOWN || msg == WM_RBUTTONDOWN {
        Message::MouseDown {
            x: lo(lparam.0),
            y: hi(lparam.0),
            button: if msg == WM_LBUTTONDOWN {
                MouseButton::Left
            } else {
                MouseButton::Right
            },
        }
    } else if msg == WM_LBUTTONUP || msg == WM_RBUTTONUP {
        Message::MouseUp {
            x: lo(lparam.0),
            y: hi(lparam.0),
            button: if msg == WM_LBUTTONUP {
                MouseButton::Left
            } else {
                MouseButton::Right
            },
        }
    } else if msg == WM_MOUSEMOVE {
        Message::MouseMove {
            x: lo(lparam.0),
            y: hi(lparam.0),
        }
    } else {
        Message::Other {
            code: msg,
            wparam: wparam.0,
            lparam: lparam.0,
        }
    }
}

fn decode_notify(lparam: LPARAM) -> Notify {
    let (from, id, code) = match notify_header(lparam) {
        Some(parts) => parts,
        None => {
            return Notify::Other {
                id: 0,
                code: 0,
                hwnd: crate::Hwnd::NULL,
            };
        }
    };
    let kind = registry::kind(hwnd_from(from));

    if code == LVN_ITEMCHANGED {
        let info = read::<NMLISTVIEW>(lparam);
        return Notify::ListView {
            id,
            event: ListViewEvent::ItemChanged {
                item: info.iItem,
                selected: info.uNewState & 0x0002 != 0,
            },
        };
    }
    if code == LVN_COLUMNCLICK {
        let info = read::<NMLISTVIEW>(lparam);
        return Notify::ListView {
            id,
            event: ListViewEvent::ColumnClick {
                column: info.iSubItem,
            },
        };
    }
    if code == LVN_KEYDOWN {
        let info = read::<NMLVKEYDOWN>(lparam);
        return Notify::ListView {
            id,
            event: ListViewEvent::KeyDown { key: info.wVKey },
        };
    }
    if code == NM_DBLCLK || code == NM_CLICK || code == NM_RCLICK {
        if kind == Some(ControlKind::TreeView) {
            let event = if code == NM_DBLCLK {
                TreeViewEvent::DoubleClick
            } else if code == NM_RCLICK {
                TreeViewEvent::RightClick
            } else {
                TreeViewEvent::Click
            };
            return Notify::TreeView { id, event };
        }
        let info = read::<NMITEMACTIVATE>(lparam);
        let event = if code == NM_DBLCLK {
            ListViewEvent::DoubleClick { item: info.iItem }
        } else if code == NM_RCLICK {
            ListViewEvent::RightClick { item: info.iItem }
        } else {
            ListViewEvent::Click { item: info.iItem }
        };
        return Notify::ListView { id, event };
    }
    if code == NM_RETURN {
        let info = read::<NMITEMACTIVATE>(lparam);
        return Notify::ListView {
            id,
            event: ListViewEvent::ReturnKey { item: info.iItem },
        };
    }
    if code == TVN_SELCHANGED {
        let info = read::<NMTREEVIEWW>(lparam);
        return Notify::TreeView {
            id,
            event: TreeViewEvent::SelectionChanged {
                item: node_id(info.itemNew.lParam),
            },
        };
    }
    if code == TVN_ITEMEXPANDED {
        let info = read::<NMTREEVIEWW>(lparam);
        return Notify::TreeView {
            id,
            event: TreeViewEvent::Expanded {
                item: node_id(info.itemNew.lParam).unwrap_or(0),
            },
        };
    }

    Notify::Other {
        id,
        code,
        hwnd: hwnd_from(from),
    }
}

fn node_id(lparam: LPARAM) -> Option<i64> {
    if lparam.0 == 0 {
        None
    } else {
        Some(lparam.0 as i64)
    }
}

/// Copies the notification struct `lparam` points at. The caller must know
/// which struct matches the `NMHDR.code` it just checked.
fn read<T: Copy>(lparam: LPARAM) -> T {
    // SAFETY: callers only use this for the struct matching the message they
    // are handling; the pointer is valid for the duration of the call.
    unsafe { *(lparam.0 as *const T) }
}
