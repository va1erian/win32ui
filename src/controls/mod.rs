#![forbid(unsafe_code)]

//! Child controls built on Win32 common controls, plus the notification
//! registry that keeps their owner-data/custom-draw plumbing out of the
//! application's window procedure.

pub mod button;
pub mod checkbox;
pub mod combobox;
pub(crate) mod combobox_events;
pub(crate) mod combobox_model;
pub mod control;
pub mod custom;
pub(crate) mod custom_inner;
pub mod edit;
pub(crate) mod edit_events;
pub(crate) mod edit_text;
pub mod flow_text;
pub mod grid_view;
pub mod groupbox;
pub mod label;
pub mod listview;
pub mod menu;
pub mod progressbar;
pub mod progressbar_theme;
pub mod radio;
pub(crate) mod registry;
pub mod scrollview;
pub mod slider;
pub mod statusbar;
pub mod taskdialog;
pub mod toolbar;
pub(crate) mod toolbar_icon;
pub(crate) mod tooltip;
pub mod treeview;

pub use control::{AsControl, Control, ControlExt, HasText};
pub use custom::{Custom, CustomWidget, Input, KeyResult, Renderer, WidgetCx};
pub use listview::ListViewTheme;
pub use progressbar_theme::ProgressBarTheme;
pub use toolbar_icon::ToolbarIcon;

/// The control types a frontend usually needs.
pub mod prelude {
    pub use super::button::Button;
    pub use super::checkbox::CheckBox;
    pub use super::combobox::ComboBox;
    pub use super::control::{AsControl, Control, ControlExt, HasText};
    pub use super::custom::{Custom, CustomWidget, Input, KeyResult, Renderer, WidgetCx};
    pub use super::edit::Edit;
    pub use super::flow_text::{FlowText, Run, RunStyle};
    pub use super::grid_view::{GridModel, GridView, GridViewTheme, TileSizeSpec, TileState};
    pub use super::groupbox::GroupBox;
    pub use super::label::Label;
    pub use super::listview::{
        Column, ColumnWidth, Fill, ListModel, ListView, ListViewEvent, ListViewTheme, RowState,
        RowStyle, SortDirection,
    };
    pub use super::menu::Menu;
    pub use super::progressbar::{ProgressBar, ProgressState};
    pub use super::progressbar_theme::ProgressBarTheme;
    pub use super::radio::{RadioGroup, RadioOption};
    pub use super::scrollview::ScrollView;
    pub use super::slider::Slider;
    pub use super::statusbar::{StatusBar, StatusBarTheme};
    pub use super::taskdialog::{TaskDialog, TaskDialogIcon};
    pub use super::toolbar::{Toolbar, ToolbarItem, ToolbarTheme};
    pub use super::toolbar_icon::ToolbarIcon;
    pub use super::treeview::{ImageList, Node, NodeStyle, TreeModel, TreeView, TreeViewEvent};
}

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
    let hwnd = sys::window::create_control(class, style, ex_style, parent, id, bounds)
        .map(sys::hwnd_from)
        .map_err(|_| Error::CreateControl(name))?;
    sys::control::apply_ui_font(hwnd, sys::dpi::window_dpi(hwnd));
    Ok(hwnd)
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
