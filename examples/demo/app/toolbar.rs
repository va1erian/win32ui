//! The demo's toolbar, mapped to `Msg`.

use win32ui::prelude::*;

use super::Msg;

/// Builds the window's toolbar. Each button carries a tooltip; the ones that
/// have a shortcut show the same text as their menu item.
pub(super) fn build(ui: &mut Ui<Msg>, _theme: Theme) -> Toolbar<Msg> {
    Toolbar::new(
        ui,
        vec![
            ToolbarItem::new("Scan")
                .with_icon(ToolbarIcon::Circle)
                .tooltip("Scan the library")
                .shortcut(Shortcut::ctrl(Key::S))
                .on_click(|| Some(Msg::Scan)),
            ToolbarItem::new("Shuffle")
                .with_icon(ToolbarIcon::Chevron)
                .tooltip("Shuffle the rows")
                .on_click(|| Some(Msg::Shuffle)),
            ToolbarItem::new("Refresh")
                .with_icon(ToolbarIcon::Arrow)
                .tooltip("Refresh")
                .shortcut(Shortcut::ctrl(Key::R))
                .on_click(|| Some(Msg::Refresh)),
            ToolbarItem::new("Theme")
                .with_icon(ToolbarIcon::Check)
                .tooltip("Toggle the theme")
                .shortcut(Shortcut::ctrl(Key::T))
                .on_click(|| Some(Msg::ToggleTheme)),
            ToolbarItem::new("Clear")
                .with_icon(ToolbarIcon::Close)
                .tooltip("Clear")
                .shortcut(Shortcut::key(Key::DELETE))
                .on_click(|| Some(Msg::Clear)),
            ToolbarItem::new("Prefs")
                .tooltip("Preferences")
                .on_click(|| Some(Msg::OpenPrefs)),
            ToolbarItem::new("Confirm")
                .tooltip("Open a confirmation dialog")
                .on_click(|| Some(Msg::OpenConfirm)),
        ],
    )
    .expect("toolbar")
}
