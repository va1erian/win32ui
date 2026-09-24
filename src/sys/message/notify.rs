//! Decoding of `WM_NOTIFY` notifications.

use windows::Win32::Foundation::LPARAM;
use windows::Win32::UI::Controls::{
    LVIS_SELECTED, LVN_COLUMNCLICK, LVN_ITEMCHANGED, LVN_KEYDOWN, LVN_ODSTATECHANGED, NM_CLICK,
    NM_DBLCLK, NM_RCLICK, NM_RETURN, NMITEMACTIVATE, NMLISTVIEW, NMLVKEYDOWN, NMLVODSTATECHANGE,
    NMTREEVIEWW, TVIS_EXPANDED, TVN_ITEMEXPANDED, TVN_SELCHANGED,
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
    // Owner-data lists report selection changes as ranges, not per item.
    // Focus-only moves leave the selected bit untouched and are skipped here;
    // the widget reads the whole selection when the bit does change, so one
    // user gesture becomes one selection event.
    if code == LVN_ODSTATECHANGED {
        let info = read::<NMLVODSTATECHANGE>(lparam);
        let changed = (info.uNewState.0 ^ info.uOldState.0) & LVIS_SELECTED.0 != 0;
        if changed {
            return Notify::ListView {
                id,
                event: ListViewEvent::SelectionChanged {
                    from: info.iFrom,
                    to: info.iTo,
                },
            };
        }
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
            event: ListViewEvent::KeyDown {
                key: info.wVKey,
                modifiers: super::keyboard_modifiers(),
            },
        };
    }
    if code == NM_DBLCLK || code == NM_CLICK || code == NM_RCLICK {
        // Only these codes are shared between control kinds, so the registry
        // lookup is deferred to here: a nested notification (custom draw during
        // an owner-draw callback) must not re-borrow a control that is busy.
        if registry::kind(hwnd_from(from)) == Some(ControlKind::TreeView) {
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
                item: info.itemNew.lParam.0 as i64,
                expanded: info.itemNew.state.0 & TVIS_EXPANDED.0 != 0,
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
