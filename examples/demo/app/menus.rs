//! The demo's menu bar and the list's context popup, both mapped to `Msg`.

use win32ui::prelude::*;

use super::Msg;
use super::options::ThemeChoice;

/// The window's menu bar: File, View and Theme, exercising submenus, radio
/// items, checked items, disabled items and shortcuts.
pub(super) fn menu_bar(theme: Theme) -> Menu<Msg> {
    let file_menu = Menu::new()
        .item("&Scan", Shortcut::ctrl(Key::S), || Msg::Scan)
        .item("&Refresh", Shortcut::ctrl(Key::R), || Msg::Refresh)
        .separator()
        .item("&Clear", Shortcut::key(Key::DELETE), || Msg::Clear)
        .separator()
        .item("E&xit", Shortcut::ctrl(Key::Q), || Msg::Quit);
    let view_menu = Menu::new()
        .radio_item("&Light", None, !theme.is_dark, || {
            Msg::SetTheme(ThemeChoice::Light)
        })
        .radio_item("&Dark", None, theme.is_dark, || {
            Msg::SetTheme(ThemeChoice::Dark)
        })
        .separator()
        .checked_item("Load &remote images", None, false, || {
            Msg::RemoteImages(true)
        })
        .disabled_item("Always disabled", None, || Msg::Refresh);
    let theme_switch = Menu::new().item("&Toggle", Shortcut::ctrl(Key::T), || Msg::ToggleTheme);
    Menu::new()
        .submenu("&File", file_menu)
        .submenu("&View", view_menu)
        .submenu("&Theme", theme_switch)
}

/// The list's context menu, shown at the cursor by `Msg::ShowListMenu`.
pub(super) fn context() -> Menu<Msg> {
    Menu::new()
        .item("&Play", None, || Msg::ContextPlay)
        .checked_item("&Loop", None, true, || Msg::ContextPlay)
        .radio_item("&Shuffle", None, true, || Msg::ContextPlay)
        .disabled_item("&Transcode", None, || Msg::Refresh)
        .separator()
        .submenu(
            "&Copy to",
            Menu::new()
                .item("&Clipboard", Shortcut::ctrl(Key::C), || Msg::Copy)
                .item("&File…", None, || Msg::Clear),
        )
        .separator()
        .item("&Delete", Shortcut::key(Key::DELETE), || Msg::ContextDelete)
}
