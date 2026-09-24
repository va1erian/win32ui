#![forbid(unsafe_code)]

//! The [`App`] trait and [`WindowSpec`].

use crate::theme::Theme;
use crate::units::{Dip, dip};
use crate::window::{Backdrop, TitleBar};

use super::ui::Ui;

/// Where the strip menu is drawn inside the extended title strip.
///
/// Set with [`WindowSpec::menu_strip_placement`]; only meaningful together with
/// [`WindowSpec::menu_in_strip`] (and an active material).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MenuStripPlacement {
    /// The menu gets its own row under the caption row, which keeps the window
    /// title and icon untouched but makes the strip taller (the default).
    #[default]
    Stacked,
    /// The menu shares the caption row with the window title and the caption
    /// buttons, Windows Terminal style: title first, then the items, with the
    /// free strip draggable. Keeps the strip at the caption height.
    Inline,
}

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
    backdrop: Backdrop,
    title_bar: TitleBar,
    menu_in_strip: bool,
    menu_strip_placement: MenuStripPlacement,
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
            backdrop: Backdrop::None,
            title_bar: TitleBar::Standard,
            menu_in_strip: false,
            menu_strip_placement: MenuStripPlacement::default(),
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

    /// The system backdrop material behind the client area.
    ///
    /// The material is best-effort: on unsupported Windows, in high-contrast
    /// mode, or when the user disabled transparency effects, the window falls
    /// back to its solid [`Theme::background`]. Ask
    /// [`Ui::backdrop_active`] whether it is active.
    pub fn backdrop(mut self, backdrop: Backdrop) -> WindowSpec {
        self.backdrop = backdrop;
        self
    }

    /// How the window's title bar is drawn.
    pub fn title_bar(mut self, title_bar: TitleBar) -> WindowSpec {
        self.title_bar = title_bar;
        self
    }

    /// Draws the menu bar in the extended title strip instead of attaching a
    /// native `HMENU` bar, so the items sit on the backdrop material (Windows
    /// Terminal style). Only takes effect with
    /// [`TitleBar::Extended`](crate::TitleBar::Extended) and an active
    /// material; otherwise the native menu bar is used unchanged.
    ///
    /// Call [`Ui::set_menu_bar`](crate::Ui::set_menu_bar) to install the menu
    /// either way.
    pub fn menu_in_strip(mut self, on: bool) -> WindowSpec {
        self.menu_in_strip = on;
        self
    }

    /// Where the strip menu is drawn (stacked row or inline on the caption).
    pub fn menu_strip_placement(mut self, placement: MenuStripPlacement) -> WindowSpec {
        self.menu_strip_placement = placement;
        self
    }

    /// Whether the menu should be drawn in the title strip.
    pub(crate) fn menu_in_strip_kind(&self) -> bool {
        self.menu_in_strip
    }

    /// The requested strip menu placement.
    pub(crate) fn menu_strip_placement_kind(&self) -> MenuStripPlacement {
        self.menu_strip_placement
    }

    /// The requested backdrop material.
    pub(crate) fn backdrop_kind(&self) -> Backdrop {
        self.backdrop
    }

    /// The requested title-bar style.
    pub(crate) fn title_bar_kind(&self) -> TitleBar {
        self.title_bar
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
