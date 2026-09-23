//! A small, idiomatic Rust wrapper over the slice of Win32 that the native
//! emusic frontend (#106) needs: custom windows, the message loop, GDI
//! painting, and a handful of common controls.
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
//!     fn message(&mut self, window: &Window, message: Message) -> Option<LResult> {
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
//! The full working program lives in `examples/demo.rs`.

mod color;
mod error;
mod geometry;
mod hwnd;
mod message;
mod theme;
mod window;

pub mod controls;
pub mod gdi;
pub mod looper;
mod sys;

pub use color::Color;
pub use error::{Error, Result};
pub use geometry::{Point, Rect, Size};
pub use hwnd::Hwnd;
pub use message::{Command, CommandNotification, LResult, Message, MouseButton, Notify, TimerId};
pub use theme::Theme;
pub use window::{Window, WindowClass, WindowExStyle, WindowHandler, WindowStyle, dpi_scale};

pub use controls::label::Label;
pub use controls::listview::{
    Column, ListSource, ListView, ListViewEvent, ListViewTheme, SortDirection,
};
pub use controls::statusbar::{StatusBar, StatusBarTheme};
pub use controls::toolbar::{Toolbar, ToolbarItem, ToolbarTheme};
pub use controls::treeview::{TreeEntry, TreeSource, TreeView, TreeViewEvent};

pub use looper::{quit, run};

/// Everything a frontend typically needs, in one `use`.
pub mod prelude {
    pub use crate::{
        Color, Column, Command, CommandNotification, Error, Hwnd, LResult, Label, ListSource,
        ListView, ListViewEvent, ListViewTheme, Message, MouseButton, Notify, Point, Rect, Result,
        SortDirection, StatusBar, StatusBarTheme, Theme, TimerId, Toolbar, ToolbarItem,
        ToolbarTheme, TreeEntry, TreeSource, TreeView, TreeViewEvent, Window, WindowClass,
        WindowExStyle, WindowHandler, WindowStyle, dpi_scale,
    };
    pub use crate::{gdi, looper, quit, run};
}

/// Performs one-time process initialisation: per-monitor-v2 DPI awareness and
/// the common-controls classes. Idempotent; safe to call before creating any
/// window. Failures are non-fatal (the controls init is best-effort).
pub fn init() {
    sys::dpi::set_per_monitor_v2();
    let _ = sys::control::init_common_controls();
}
