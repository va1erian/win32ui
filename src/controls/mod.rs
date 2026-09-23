#![forbid(unsafe_code)]

//! Child controls built on Win32 common controls, plus the notification
//! registry that keeps their owner-data/custom-draw plumbing out of the
//! application's window procedure.

pub mod combobox;
pub(crate) mod combobox_events;
pub(crate) mod combobox_model;
pub mod control;
pub mod label;
pub mod listview;
pub(crate) mod listview_events;
pub(crate) mod listview_inner;
pub mod listview_theme;
pub mod progressbar;
pub mod progressbar_theme;
pub(crate) mod registry;
pub mod statusbar;
pub mod taskdialog;
pub mod toolbar;
pub mod treeview;

pub use control::{AsControl, Control, ControlExt, HasText};
pub use listview_theme::ListViewTheme;
pub use progressbar_theme::ProgressBarTheme;

use crate::error::{Error, Result};
use crate::geometry::Rect;
use crate::hwnd::Hwnd;
use crate::sys;

/// Creates a child control window from a system window class and returns its
/// safe handle.
pub(crate) fn create_child(
    name: &'static str,
    class: &str,
    parent: Hwnd,
    style: u32,
    ex_style: u32,
    id: usize,
    bounds: Rect,
) -> Result<Hwnd> {
    sys::control::init_common_controls()?;
    sys::window::create_control(class, style, ex_style, parent, id, bounds)
        .map(sys::hwnd_from)
        .map_err(|_| Error::CreateControl(name))
}

thread_local! {
    /// Source of unique, non-zero control ids for this thread. The ids are
    /// internal only: notifications are routed by `HWND`, never by id.
    static NEXT_CONTROL_ID: std::cell::Cell<usize> = const { std::cell::Cell::new(1) };
}

/// The next unique control id, used purely to satisfy the Win32 child-id slot.
pub(crate) fn next_id() -> usize {
    NEXT_CONTROL_ID.with(|next| {
        let id = next.get();
        next.set(id.wrapping_add(1).max(1));
        id
    })
}

/// Standard child-window style bits, as raw values so the safe modules don't
/// need the `windows` crate.
pub(crate) mod style {
    pub(crate) const WS_CHILD: u32 = 0x4000_0000;
    pub(crate) const WS_VISIBLE: u32 = 0x1000_0000;
    pub(crate) const WS_BORDER: u32 = 0x0080_0000;
    pub(crate) const WS_TABSTOP: u32 = 0x0001_0000;
    pub(crate) const WS_VSCROLL: u32 = 0x0020_0000;
    pub(crate) const WS_EX_CLIENTEDGE: u32 = 0x0000_0200;
}
