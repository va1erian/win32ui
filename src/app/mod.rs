#![forbid(unsafe_code)]

//! The widget layer: [`App`] + [`Ui`], message-mapped events, and [`run_app`].
//!
//! Applications implement [`App`], build their widgets inside [`run_app`]'s
//! closure, and receive their own [`App::Msg`] type in [`App::update`] — never
//! raw window messages, and never re-entered.

mod child;
mod core;
mod layout;
mod proxy;
mod run;
mod spec;
mod title_menu;
mod ui;

pub use child::WindowHandle;
pub use layout::split::Split;
pub use layout::tabs::Tabs;
pub use layout::{IntoLayoutItem, Layout, LayoutExt, LayoutItem};
pub use proxy::Proxy;
pub use run::run_app;
pub use spec::{App, MenuStripPlacement, WindowSpec};
pub use ui::Ui;

/// The widget-layer types a frontend usually needs.
pub mod prelude {
    pub use super::{
        App, IntoLayoutItem, Layout, LayoutExt, LayoutItem, MenuStripPlacement, Proxy, Split, Tabs,
        Ui, WindowHandle, WindowSpec, run_app,
    };
    pub use crate::{column, row, split_col, split_row, tabs};
}
