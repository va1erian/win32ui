#![forbid(unsafe_code)]

//! The [`App`] trait and [`WindowSpec`].

use crate::theme::Theme;
use crate::units::{Dip, dip};

use super::ui::Ui;

/// A widget-layer application.
///
/// Widget events are mapped to the app's own [`App::Msg`] type by closures
/// given when the widgets are built, then queued and delivered to [`update`].
/// `update` is never re-entered: a message raised while it runs — a widget
/// call that fires a notification, a modal menu loop — is delivered after it
/// returns.
pub trait App: 'static {
    /// The application's message type.
    type Msg: 'static;

    /// Handles one message. Use `ui` to create widgets, change the window, or
    /// enqueue further messages.
    fn update(&mut self, msg: Self::Msg, ui: &mut Ui<Self::Msg>);
}

/// How to build the top-level window for [`run_app`](super::run_app).
pub struct WindowSpec {
    title: String,
    width: Dip,
    height: Dip,
    theme: Theme,
    theme_explicit: bool,
}

impl WindowSpec {
    /// A window with `title`, a default size and the light theme.
    pub fn new(title: impl Into<String>) -> WindowSpec {
        WindowSpec {
            title: title.into(),
            width: dip(640.0),
            height: dip(480.0),
            theme: Theme::light(),
            theme_explicit: false,
        }
    }

    /// The window's initial client size, as a design value.
    pub fn size(mut self, width: Dip, height: Dip) -> WindowSpec {
        self.width = width;
        self.height = height;
        self
    }

    /// The palette the window and its controls are built with.
    pub fn theme(mut self, theme: Theme) -> WindowSpec {
        self.theme = theme;
        self.theme_explicit = true;
        self
    }

    /// The effective theme: the explicit one if set, otherwise `fallback` (the
    /// opener's theme, for a secondary window).
    pub(crate) fn theme_or(&self, fallback: Theme) -> Theme {
        if self.theme_explicit {
            self.theme
        } else {
            fallback
        }
    }

    pub(crate) fn parts(&self) -> (&str, Dip, Dip, Theme) {
        (&self.title, self.width, self.height, self.theme)
    }
}
