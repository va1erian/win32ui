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
mod app;
mod capture;
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
pub mod looper;
mod sys;

pub use accel::{Shortcut, ShortcutParseError};
pub use app::{
    App, IntoLayoutItem, Layout, LayoutExt, LayoutItem, Proxy, Split, Tabs, Ui, WindowHandle,
    WindowSpec, run_app,
};
pub use capture::RgbaImage;
pub use color::Color;
pub use error::{Error, Result, Win32Error};
pub use geometry::{Point, Rect, Size};
pub use hwnd::Hwnd;
pub use layout::{Dock, DockLayout, Insets, Stack, StackDirection, StackSlot};
pub use message::{
    Command, CommandNotification, HitTest, Key, LResult, Message, MinMaxInfo, Modifiers,
    MouseButton, Notify, TimerId,
};
pub use theme::{Theme, Themed};
pub use units::{Dip, Px, dip};
pub use window::{
    Backdrop, CursorShape, Icon, Placement, ShowState, TitleBar, Window, WindowClass,
    WindowExStyle, WindowHandler, WindowStyle, monitor_work_areas,
};

pub use controls::button::Button;
pub use controls::checkbox::CheckBox;
pub use controls::combobox::ComboBox;
pub use controls::control::{AsControl, Control, ControlExt, HasText};
pub use controls::custom::{Custom, CustomWidget, Input, Renderer, WidgetCx};
pub use controls::edit::Edit;
pub use controls::groupbox::GroupBox;
pub use controls::label::Label;
pub use controls::listview::{
    Column, ColumnWidth, Fill, ListModel, ListView, ListViewEvent, ListViewTheme, SortDirection,
};
pub use controls::menu::Menu;
pub use controls::progressbar::{ProgressBar, ProgressState};
pub use controls::progressbar_theme::ProgressBarTheme;
pub use controls::radio::{RadioGroup, RadioOption};
pub use controls::scrollview::ScrollView;
pub use controls::statusbar::{StatusBar, StatusBarTheme};
pub use controls::taskdialog::{TaskDialog, TaskDialogIcon};
pub use controls::toolbar::{Toolbar, ToolbarItem, ToolbarTheme};
pub use controls::treeview::{TreeEntry, TreeSource, TreeView, TreeViewEvent};

pub use looper::{quit, run, run_modal};

/// Everything a frontend typically needs, in one `use`.
pub mod prelude {
    pub use crate::{
        App, AsControl, Backdrop, Button, CheckBox, Color, Column, ColumnWidth, ComboBox, Command,
        CommandNotification, Control, ControlExt, CursorShape, Custom, CustomWidget, Dip, Dock,
        DockLayout, Edit, Error, Fill, GroupBox, HasText, HitTest, Hwnd, Icon, Input, Insets,
        IntoLayoutItem, Key, LResult, Label, Layout, LayoutExt, LayoutItem, ListModel, ListView,
        ListViewEvent, ListViewTheme, Menu, Message, MinMaxInfo, Modifiers, MouseButton, Notify,
        Placement, Point, ProgressBar, ProgressBarTheme, ProgressState, Proxy, Px, RadioGroup,
        RadioOption, Rect, Renderer, Result, RgbaImage, ScrollView, Shortcut, ShortcutParseError,
        ShowState, SortDirection, Split, Stack, StackDirection, StackSlot, StatusBar,
        StatusBarTheme, Tabs, TaskDialog, TaskDialogIcon, Theme, Themed, TimerId, TitleBar,
        Toolbar, ToolbarItem, ToolbarTheme, TreeEntry, TreeSource, TreeView, TreeViewEvent, Ui,
        WidgetCx, Win32Error, Window, WindowClass, WindowExStyle, WindowHandle, WindowHandler,
        WindowSpec, WindowStyle, column, dip, monitor_work_areas, row, run_app, split_col,
        split_row, tabs,
    };
    pub use crate::{clipboard, gdi, looper, quit, run, run_modal};
}

/// Performs one-time process initialisation: per-monitor-v2 DPI awareness and
/// the common-controls classes. Idempotent; safe to call before creating any
/// window. Failures are non-fatal (the controls init is best-effort).
pub fn init() {
    sys::dpi::set_per_monitor_v2();
    let _ = sys::control::init_common_controls();
}
