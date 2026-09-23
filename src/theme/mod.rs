#![forbid(unsafe_code)]

//! Theming foundation: semantic [`Theme`] tokens, the [`Themed`] trait every
//! widget implements, the per-window theme store and the central
//! `WM_CTLCOLOR*` answers. See the README's *Theming* section for the model.

mod ctlcolor;
mod registry;
mod themed;
mod tokens;

pub use themed::Themed;
pub use tokens::Theme;

pub(crate) use ctlcolor::{answer as ctlcolor_answer, is_ctlcolor};
pub(crate) use registry::{
    backdrop_active, forget_window as forget_window_theme, register_child as register_themed,
    retheme_children, set_backdrop_active, set_window_theme, unregister_child as unregister_themed,
    window_background, window_theme,
};

/// The theming types a frontend usually needs.
pub mod prelude {
    pub use super::{Theme, Themed};
}
