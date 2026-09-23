//! The demo's toolbar, mapped to `Msg`.

use win32ui::prelude::*;

use super::Msg;
use super::icons::dot_icon;

/// Builds the window's toolbar.
pub(super) fn build(ui: &mut Ui<Msg>, theme: Theme) -> Toolbar<Msg> {
    Toolbar::new(
        ui,
        vec![
            ToolbarItem::new("Scan")
                .with_icon(dot_icon(theme.accent))
                .on_click(|| Some(Msg::Scan)),
            ToolbarItem::new("Shuffle").on_click(|| Some(Msg::Shuffle)),
            ToolbarItem::new("Refresh")
                .with_icon(dot_icon(theme.text_secondary))
                .on_click(|| Some(Msg::Refresh)),
            ToolbarItem::new("Theme").on_click(|| Some(Msg::ToggleTheme)),
            ToolbarItem::new("Clear").on_click(|| Some(Msg::Clear)),
            ToolbarItem::new("Prefs").on_click(|| Some(Msg::OpenPrefs)),
            ToolbarItem::new("Confirm").on_click(|| Some(Msg::OpenConfirm)),
        ],
    )
    .expect("toolbar")
}
