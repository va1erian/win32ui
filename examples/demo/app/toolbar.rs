//! The demo's toolbar, mapped to `Msg`.
//!
//! It exercises the toolbar's groups (separators), named vector icons, an
//! icon-only group, a flexible spacer that pushes the right-hand group to the
//! edge, a star toggle, a disabled button and a text-under-icon label.

use win32ui::prelude::*;

use super::Msg;

/// Builds the window's toolbar. Each button carries a tooltip; the ones that
/// have a shortcut show the same text as their menu item.
pub(super) fn build(ui: &mut Ui<Msg>, _theme: Theme) -> Toolbar<Msg> {
    Toolbar::new(
        ui,
        vec![
            ToolbarItem::new("Reply")
                .id(1u32)
                .with_icon(ToolbarIcon::Reply)
                .label_mode(LabelMode::IconOnly)
                .tooltip("Reply")
                .on_click(|| Some(Msg::Reply)),
            ToolbarItem::new("Forward")
                .id(2u32)
                .with_icon(ToolbarIcon::Forward)
                .label_mode(LabelMode::IconOnly)
                .tooltip("Forward")
                .on_click(|| Some(Msg::Forward)),
            ToolbarItem::new("Archive")
                .id(3u32)
                .with_icon(ToolbarIcon::Archive)
                .label_mode(LabelMode::IconOnly)
                .tooltip("Archive")
                .on_click(|| Some(Msg::Archive)),
            // Nothing is selected, so the button is dimmed and emits nothing.
            ToolbarItem::new("Delete")
                .id(4u32)
                .with_icon(ToolbarIcon::Delete)
                .label_mode(LabelMode::IconOnly)
                .enabled(false)
                .tooltip("Nothing selected"),
            Toolbar::separator(),
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
            // A flexible spacer absorbs the leftover width, so the group after
            // it sits against the right end.
            Toolbar::flexible_spacer(),
            ToolbarItem::new("Star")
                .id(5u32)
                .with_icon(ToolbarIcon::StarFilled)
                .label_mode(LabelMode::IconOnly)
                .toggle()
                .tooltip("Star or unstar")
                .on_toggle(|checked| Some(Msg::Star(checked))),
            ToolbarItem::new("Compose")
                .with_icon(ToolbarIcon::Compose)
                .label_mode(LabelMode::TextUnder)
                .tooltip("Compose")
                .on_click(|| Some(Msg::Compose)),
            ToolbarItem::new("Settings")
                .with_icon(ToolbarIcon::glyph('\u{E713}'))
                .label_mode(LabelMode::IconOnly)
                .tooltip("Settings")
                .on_click(|| Some(Msg::OpenPrefs)),
        ],
    )
    .expect("toolbar")
}
