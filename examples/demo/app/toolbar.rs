//! The demo's toolbar, mapped to `Msg`.

use win32ui::prelude::*;

use super::Msg;

/// Builds the window's toolbar.
pub(super) fn build(ui: &mut Ui<Msg>, _theme: Theme) -> Toolbar<Msg> {
    Toolbar::new(
        ui,
        vec![
            ToolbarItem::new("Scan")
                .with_icon(ToolbarIcon::Circle)
                .on_click(|| Some(Msg::Scan)),
            ToolbarItem::new("Shuffle")
                .with_icon(ToolbarIcon::Chevron)
                .on_click(|| Some(Msg::Shuffle)),
            ToolbarItem::new("Refresh")
                .with_icon(ToolbarIcon::Arrow)
                .on_click(|| Some(Msg::Refresh)),
            ToolbarItem::new("Theme")
                .with_icon(ToolbarIcon::Check)
                .on_click(|| Some(Msg::ToggleTheme)),
            ToolbarItem::new("Clear")
                .with_icon(ToolbarIcon::Close)
                .on_click(|| Some(Msg::Clear)),
            ToolbarItem::new("Prefs").on_click(|| Some(Msg::OpenPrefs)),
            ToolbarItem::new("Confirm").on_click(|| Some(Msg::OpenConfirm)),
        ],
    )
    .expect("toolbar")
}
