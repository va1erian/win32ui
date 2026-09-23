#![forbid(unsafe_code)]

//! The derived colour palette for [`ProgressBar`](super::progressbar::ProgressBar).

use crate::color::Color;
use crate::theme::Theme;

/// Colours for the progress bar.
#[derive(Clone, Copy, Debug)]
pub struct ProgressBarTheme {
    /// The empty portion of the bar.
    pub track: Color,
    /// The fill for [`ProgressState::Normal`](super::progressbar::ProgressState::Normal).
    pub fill: Color,
    /// The fill for [`ProgressState::Paused`](super::progressbar::ProgressState::Paused).
    pub paused: Color,
    /// The fill for [`ProgressState::Error`](super::progressbar::ProgressState::Error).
    pub error: Color,
}

impl ProgressBarTheme {
    /// Derives a palette from the app [`Theme`]. Override any field after
    /// calling this for a custom look.
    pub fn from_theme(theme: &Theme) -> ProgressBarTheme {
        ProgressBarTheme {
            track: theme.border,
            fill: theme.accent,
            paused: theme.warning,
            error: theme.danger,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ProgressBarTheme;
    use crate::theme::Theme;

    #[test]
    fn follows_semantic_tokens() {
        for theme in [Theme::light(), Theme::dark()] {
            let bar = ProgressBarTheme::from_theme(&theme);
            assert_eq!(bar.track, theme.border);
            assert_eq!(bar.fill, theme.accent);
            assert_eq!(bar.paused, theme.warning);
            assert_eq!(bar.error, theme.danger);
        }
    }
}
