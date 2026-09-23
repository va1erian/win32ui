//! Decoding of `WM_NOTIFY` notifications.

use windows::Win32::Foundation::LPARAM;
use windows::Win32::UI::Controls::{
    LVN_COLUMNCLICK, LVN_ITEMCHANGED, LVN_KEYDOWN, NM_CLICK, NM_DBLCLK, NM_RCLICK, NM_RETURN,
    NMITEMACTIVATE, NMLISTVIEW, NMLVKEYDOWN, NMTREEVIEWW, TVN_ITEMEXPANDED, TVN_SELCHANGED,
};

use crate::controls::listview::ListViewEvent;
use crate::controls::registry::{self, ControlKind};
use crate::controls::treeview::TreeViewEvent;
use crate::message::Notify;

use super::{hwnd_from, notify_header, read};

/// Decodes a `WM_NOTIFY` into the typed [`Notify`] enum.
pub(crate) fn decode_notify(lparam: LPARAM) -> Notify {
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
