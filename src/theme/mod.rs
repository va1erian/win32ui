#![forbid(unsafe_code)]

//! Theming foundation: semantic [`Theme`] tokens, the [`Themed`] trait every
//! widget implements, the per-window theme store and the central
//! `WM_CTLCOLOR*` answers. See the README's *Theming* section for the model.

mod ctlcolor;
mod registry;
mod system;
mod themed;
mod tokens;

pub use system::is_theme_change;
pub use themed::Themed;
pub use tokens::Theme;

pub(crate) use ctlcolor::{answer as ctlcolor_answer, is_ctlcolor};
pub(crate) use registry::{
    ApplyTheme, backdrop_active, forget_window as forget_window_theme,
    register_child as register_themed, retheme_children, set_backdrop_active,
    set_follow_system as set_window_follow_system, set_window_theme,
    unregister_child as unregister_themed, window_background, window_theme,
};

/// Re-reads [`Theme::system`] and applies it to `window`, if it opted in with
/// [`Window::follow_system_theme`](crate::Window::follow_system_theme) or
/// `Ui::follow_system_theme`.
pub(crate) fn notify_theme_change(window: crate::hwnd::Hwnd) {
    if let Some(apply) = registry::follow_system_apply(window) {
        apply(&Theme::system());
    }
}

/// The theming types a frontend usually needs.
pub mod prelude {
    pub use super::{Theme, Themed, is_theme_change};
}
