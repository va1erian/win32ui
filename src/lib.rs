//! Native Windows UI for Rust: small, fast, idiomatic, and themed. Dark mode is
//! first-class. Two layers: a safe platform layer over Win32 (windows, typed
//! messages, GDI) and a widget layer where widget events are mapped to the
//! application's own message type (see the README's *Architecture* section).
//!
//! The crate is deliberately split so that `unsafe` is confined to [`sys`]:
//! every other module starts with `#![forbid(unsafe_code)]` and talks to
//! Win32 only through the safe functions that [`sys`] exposes.
//!
//! # Shape of the API
//!
//! * [`Window`] wraps an `HWND` and a [`WindowHandler`]; messages arrive as a
//!   typed [`Message`] instead of raw `(u32, WPARAM, LPARAM)` triples.
//! * Controls ([`ListView`], [`TreeView`], [`Toolbar`], …) are Rust structs
//!   that own a child `HWND`. Their self-contained notifications (owner-data
//!   requests, custom draw, lazy tree expansion) never reach the application;
//!   only meaningful events do, decoded into per-control enums.
//! * [`gdi`] provides RAII handles ([`gdi::Font`], [`gdi::Brush`],
//!   [`gdi::Pen`], [`gdi::Bitmap`]) and a double-buffered [`gdi::Paint`]
//!   context, so no manual `DeleteObject` bookkeeping is required.
//!
//! # Getting started
//!
//! ```
//! use win32ui::prelude::*;
//!
//! struct Main;
//!
//! impl WindowHandler for Main {
//!     fn message(&self, window: &Window, message: Message) -> Option<LResult> {
//!         if let Message::Close = message {
//!             window.destroy();
//!             win32ui::quit(0);
//!             return Some(0);
//!         }
//!         None
//!     }
//! }
//! ```
//!
//! The full working program lives in `examples/demo/`.

// The crate is Win32-only. On other targets it compiles to an empty crate so
// that dependants (e.g. a cross-platform workspace) can still `cargo check`.
#![cfg(windows)]

mod accel;
pub mod accessibility;
mod app;
pub mod capture;
mod color;
mod error;
mod geometry;
mod hwnd;
mod layout;
mod message;
mod theme;
mod units;
mod window;

pub mod clipboard;
pub mod controls;
pub mod d2d;
pub mod gdi;
pub mod gl;
pub mod imaging;
pub mod looper;
mod sys;

pub use accel::{Shortcut, ShortcutParseError};
pub use app::{
    App, DEFAULT_ACCENT_TINT_STRENGTH, Fluent, IntoLayoutItem, Layout, LayoutExt, LayoutItem,
    MaterialStatusBar, MaterialTopBar, MenuStripPlacement, Proxy, Split, Tabs, TopBarEvent,
    TopBarId, TopBarItem, Ui, WindowHandle, WindowSpec, run_app,
};
pub use capture::RgbaImage;
pub use color::Color;
pub use error::{CaptureError, Error, ImagingError, Result, Win32Error};
pub use geometry::{Point, Rect, Size};
/// The OpenGL binding [`Renderer::Gl`] widgets draw with, re-exported so an
/// implementor names the exact version this crate links against.
pub use glow;
pub use hwnd::Hwnd;
pub use layout::{Dock, DockLayout, Insets, Stack, StackDirection, StackSlot};
pub use message::{
    Command, CommandNotification, HitTest, Key, LResult, Message, MinMaxInfo, Modifiers,
    MouseButton, Notify, TimerId,
};
pub use theme::{Theme, Themed, is_theme_change};
pub use units::{Dip, Px, dip};
pub use window::{
    Backdrop, CursorShape, Icon, MonitorInfo, Placement, ShowState, TitleBar, Window, WindowClass,
    WindowExStyle, WindowHandler, WindowStyle, monitor_of, monitor_work_areas, monitors,
};

pub use controls::button::Button;
pub use controls::checkbox::CheckBox;
pub use controls::color_picker::ColorPicker;
pub use controls::combobox::ComboBox;
pub use controls::control::{AsControl, Control, ControlExt, HasText};
pub use controls::custom::{Custom, CustomWidget, Input, KeyResult, Renderer, WidgetCx};
pub use controls::edit::Edit;
pub use controls::flow_text::{FlowText, Run, RunStyle};
pub use controls::grid_view::{GridModel, GridView, GridViewTheme, TileSizeSpec, TileState};
pub use controls::groupbox::GroupBox;
pub use controls::label::Label;
pub use controls::listview::{
    Column, ColumnWidth, Fill, ListModel, ListView, ListViewEvent, ListViewTheme, RowState,
    RowStyle, SortDirection,
};
pub use controls::menu::Menu;
pub use controls::panel::Panel;
pub use controls::progressbar::{ProgressBar, ProgressState};
pub use controls::progressbar_theme::ProgressBarTheme;
pub use controls::radio::{RadioGroup, RadioOption};
pub use controls::scrollview::ScrollView;
pub use controls::statusbar::{StatusBar, StatusBarTheme};
pub use controls::taskdialog::{TaskDialog, TaskDialogIcon};
pub use controls::toolbar::{LabelMode, Toolbar, ToolbarItem, ToolbarItemId, ToolbarTheme};
pub use controls::treeview::{ImageList, Node, NodeStyle, TreeModel, TreeView, TreeViewEvent};

pub use looper::{quit, run, run_modal};

/// Everything a frontend typically needs, in one `use`.
///
/// Each module owns its own list in `prelude`, so adding a public item is a
/// one-line change in the module that defines it.
pub mod prelude {
    pub use crate::accel::prelude::*;
    pub use crate::app::prelude::*;
    pub use crate::capture::prelude::*;
    pub use crate::color::prelude::*;
    pub use crate::controls::prelude::*;
    pub use crate::error::prelude::*;
    pub use crate::geometry::prelude::*;
    pub use crate::hwnd::prelude::*;
    pub use crate::imaging::prelude::*;
    pub use crate::layout::prelude::*;
    pub use crate::looper::prelude::*;
    pub use crate::message::prelude::*;
    pub use crate::theme::prelude::*;
    pub use crate::units::prelude::*;
    pub use crate::window::prelude::*;

    pub use crate::{clipboard, gdi, looper};
}

/// Performs one-time process initialisation: per-monitor-v2 DPI awareness and
/// the common-controls classes. Idempotent; safe to call before creating any
/// window. Failures are non-fatal (the controls init is best-effort).
pub fn init() {
    sys::dpi::set_per_monitor_v2();
    let _ = sys::control::init_common_controls();
}
