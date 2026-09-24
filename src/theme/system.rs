#![forbid(unsafe_code)]

//! [`Theme::system`]: reading the OS theme (light/dark, accent colour, high
//! contrast), and [`is_theme_change`], which tells a [`Message`] that reports
//! it apart from everything else a window receives.

use crate::message::Message;
use crate::sys;
use crate::sys::theme_system::HighContrastColors;

use super::tokens::Theme;

impl Theme {
    /// Reads the current system theme.
    ///
    /// - Light/dark comes from `AppsUseLightTheme` under
    ///   `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize`.
    /// - The accent colour comes from `DwmGetColorizationColor` (see that
    ///   function's doc comment for why, over the WinRT `UISettings`).
    /// - High contrast comes from `SystemParametersInfoW(SPI_GETHIGHCONTRAST)`,
    ///   mapped to a [`Theme`] through `GetSysColor`.
    ///
    /// High contrast wins over light/dark and the accent colour, the same
    /// priority Windows itself gives the setting.
    pub fn system() -> Theme {
        match sys::theme_system::high_contrast_colors() {
            Some(colors) => Theme::from_high_contrast(colors),
            None => Theme::from_light_dark_accent(
                sys::theme_system::apps_use_light_theme(),
                sys::theme_system::accent_color(),
            ),
        }
    }

    /// Builds the light or dark palette, with the accent colour (and the
    /// borders that match it) swapped in when one is known. Factored out of
    /// [`Theme::system`] so the mapping is unit-testable without a live
    /// desktop.
    fn from_light_dark_accent(is_light: bool, accent: Option<crate::Color>) -> Theme {
        let mut theme = if is_light {
            Theme::light()
        } else {
            Theme::dark()
        };
        if let Some(accent) = accent {
            theme.accent = accent;
            theme.border_focused = accent;
        }
        theme
    }

    /// Builds the high-contrast palette from raw `GetSysColor` values.
    /// Factored out of [`Theme::system`] so the mapping is unit-testable
    /// without a live desktop.
    fn from_high_contrast(colors: HighContrastColors) -> Theme {
        let is_dark = colors.window.luminance() < 0.5;
        Theme {
            is_dark,
            background: colors.window,
            surface: colors.btn_face,
            raised: colors.window,
            text: colors.window_text,
            text_secondary: colors.window_text,
            text_disabled: colors.gray_text,
            text_on_accent: colors.highlight_text,
            accent: colors.hotlight,
            warning: colors.hotlight,
            danger: colors.hotlight,
            selection: colors.highlight,
            selection_unfocused: colors.highlight,
            hover: colors.highlight,
            pressed: colors.highlight,
            border: colors.window_frame,
            border_focused: colors.hotlight,
            input_background: colors.window,
            scrollbar: colors.window_frame,
            scrollbar_track: colors.window,
        }
    }
}

/// Whether `message` reports that the system theme changed: a
/// `WM_SETTINGCHANGE` with section `"ImmersiveColorSet"` (the light/dark
/// setting or the accent colour changed), or a `WM_SYSCOLORCHANGE`/
/// `WM_THEMECHANGED` (system colours or the visual style changed, e.g.
/// entering or leaving high contrast).
///
/// [`Window::follow_system_theme`](crate::Window::follow_system_theme) uses
/// this to know when to re-read [`Theme::system`].
pub fn is_theme_change(message: &Message) -> bool {
    match message {
        Message::SettingChange { section } => section.as_deref() == Some("ImmersiveColorSet"),
        Message::SysColorChange | Message::ThemeChanged => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Color;

    #[test]
    fn immersive_color_set_is_a_theme_change() {
        assert!(is_theme_change(&Message::SettingChange {
            section: Some("ImmersiveColorSet".into()),
        }));
    }

    #[test]
    fn other_setting_change_sections_are_not() {
        assert!(!is_theme_change(&Message::SettingChange {
            section: Some("Environment".into()),
        }));
        assert!(!is_theme_change(&Message::SettingChange { section: None }));
    }

    #[test]
    fn sys_color_change_and_theme_changed_are_theme_changes() {
        assert!(is_theme_change(&Message::SysColorChange));
        assert!(is_theme_change(&Message::ThemeChanged));
    }

    #[test]
    fn unrelated_messages_are_not_theme_changes() {
        assert!(!is_theme_change(&Message::Paint));
        assert!(!is_theme_change(&Message::Close));
    }

    #[test]
    fn light_dark_mapping_swaps_the_palette() {
        assert_eq!(Theme::from_light_dark_accent(true, None), Theme::light());
        assert_eq!(Theme::from_light_dark_accent(false, None), Theme::dark());
    }

    #[test]
    fn an_accent_colour_overrides_the_default_accent_and_focus_border() {
        let accent = Color::rgb(0x12, 0x34, 0x56);
        let theme = Theme::from_light_dark_accent(true, Some(accent));
        assert_eq!(theme.accent, accent);
        assert_eq!(theme.border_focused, accent);
        // Everything else keeps the light palette.
        assert_eq!(theme.background, Theme::light().background);
    }

    #[test]
    fn high_contrast_theme_is_built_from_raw_sys_colors() {
        let colors = HighContrastColors {
            window: Color::rgb(0, 0, 0),
            window_text: Color::rgb(255, 255, 255),
            window_frame: Color::rgb(255, 255, 255),
            btn_face: Color::rgb(0, 0, 0),
            gray_text: Color::rgb(128, 128, 128),
            highlight: Color::rgb(255, 255, 0),
            highlight_text: Color::rgb(0, 0, 0),
            hotlight: Color::rgb(0, 255, 255),
        };
        let theme = Theme::from_high_contrast(colors);
        assert!(theme.is_dark);
        assert_eq!(theme.background, colors.window);
        assert_eq!(theme.text, colors.window_text);
        assert_eq!(theme.accent, colors.hotlight);
        assert_eq!(theme.selection, colors.highlight);
        assert_eq!(theme.border, colors.window_frame);
    }

    #[test]
    fn a_light_high_contrast_palette_is_not_flagged_dark() {
        let colors = HighContrastColors {
            window: Color::rgb(255, 255, 255),
            window_text: Color::rgb(0, 0, 0),
            window_frame: Color::rgb(0, 0, 0),
            btn_face: Color::rgb(255, 255, 255),
            gray_text: Color::rgb(96, 96, 96),
            highlight: Color::rgb(0, 0, 128),
            highlight_text: Color::rgb(255, 255, 255),
            hotlight: Color::rgb(0, 0, 255),
        };
        assert!(!Theme::from_high_contrast(colors).is_dark);
    }
}
