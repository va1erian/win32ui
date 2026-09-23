#![forbid(unsafe_code)]

//! `FlowText`: a wrapped, flowing line of styled runs with clickable links.
//!
//! Runs are laid out left to right as **one** DirectWrite layout, so word wrap,
//! bidi reordering and hit testing cross run boundaries — no per-run line
//! breaking. Each run is styled from theme tokens: normal, weak
//! (de-emphasised) or link. A link is accent-coloured, gets the hand cursor on
//! hover, is underlined while hovered, and emits its own mapped `Msg` on click
//! through the same per-window queue as every other widget.
//!
//! ```ignore
//! FlowText::new(ui)?
//!     .run(Run::link(&artist).on_click(|| Some(Msg::GoToArtist(artist.clone()))))
//!     .separator(" · ")
//!     .run(Run::link(&album).on_click(|| Some(Msg::GoToAlbum(album.clone()))))
//!     .separator(" · ")
//!     .run(Run::weak(format!("({year})")))
//! ```

mod widget;

use crate::app::Ui;
use crate::controls::control::{AsControl, Control};
use crate::controls::custom::Custom;
use crate::error::Result;
use crate::theme::{Theme, Themed};
use crate::units::Dip;

use widget::FlowTextWidget;

/// How a [`Run`] is styled from the theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunStyle {
    /// Primary text ([`Theme::text`]).
    Normal,
    /// De-emphasised text ([`Theme::text_secondary`]).
    Weak,
    /// A clickable link ([`Theme::accent`], hand cursor, underlined on hover).
    Link,
}

/// One run of a [`FlowText`] line.
///
/// Build it with [`Run::normal`], [`Run::weak`] or [`Run::link`], and map a
/// click with [`Run::on_click`] (only meaningful for a link).
pub struct Run<M> {
    text: String,
    style: RunStyle,
    weight: u16,
    italic: bool,
    size_dip: Option<f32>,
    on_click: Option<Box<dyn Fn() -> Option<M>>>,
}

impl<M> Run<M> {
    fn new(text: impl Into<String>, style: RunStyle) -> Run<M> {
        Run {
            text: text.into(),
            style,
            weight: 400,
            italic: false,
            size_dip: None,
            on_click: None,
        }
    }

    /// A primary-text run.
    pub fn normal(text: impl Into<String>) -> Run<M> {
        Run::new(text, RunStyle::Normal)
    }

    /// A de-emphasised run.
    pub fn weak(text: impl Into<String>) -> Run<M> {
        Run::new(text, RunStyle::Weak)
    }

    /// A clickable link run.
    pub fn link(text: impl Into<String>) -> Run<M> {
        Run::new(text, RunStyle::Link)
    }

    /// Sets the font weight (100-900).
    pub fn weight(mut self, weight: u16) -> Run<M> {
        self.weight = weight;
        self
    }

    /// Sets italic.
    pub fn italic(mut self, italic: bool) -> Run<M> {
        self.italic = italic;
        self
    }

    /// Sets the em size in design units. Runs of different sizes share one
    /// baseline.
    pub fn size(mut self, size_dip: f32) -> Run<M> {
        self.size_dip = Some(size_dip);
        self
    }

    /// Maps a click on this run to the app's message.
    pub fn on_click(mut self, click: impl Fn() -> Option<M> + 'static) -> Run<M> {
        self.on_click = Some(Box::new(click));
        self
    }
}

/// A wrapped line of styled, optionally clickable runs.
///
/// Build it with [`FlowText::new`] and the chaining setters. Size it through
/// the layout, or ask [`FlowText::preferred_height`] for the height the runs
/// wrap to at a given width.
pub struct FlowText<M: 'static> {
    custom: Custom<FlowTextWidget<M>, M>,
}

impl<M: 'static> FlowText<M> {
    /// Creates an empty flow line as a child of the window behind `ui`,
    /// adopting `ui`'s theme. Fails when DirectWrite is unavailable.
    pub fn new(ui: &mut Ui<M>) -> Result<FlowText<M>> {
        let widget = FlowTextWidget::new()?;
        let custom = Custom::new(ui, widget)?.on_event(|msg| Some(msg));
        Ok(FlowText { custom })
    }

    /// Appends `run` to the line.
    pub fn run(self, run: Run<M>) -> FlowText<M> {
        self.custom.widget().borrow_mut().push(run);
        self.custom.invalidate();
        self
    }

    /// Appends a weak separator run (for example `" · "`).
    pub fn separator(self, text: &str) -> FlowText<M> {
        self.run(Run::weak(text))
    }

    /// Replaces the base font (a family list and an em size in design units)
    /// that runs without an explicit size inherit.
    pub fn set_font(&self, family: &str, size_dip: f32) -> Result<()> {
        self.custom.widget().borrow().set_font(family, size_dip)?;
        self.custom.invalidate();
        Ok(())
    }

    /// The height the runs wrap to at `width`, in design units, so a layout
    /// can size the widget to its wrapped content.
    pub fn preferred_height(&self, width: Dip) -> Dip {
        let widget = self.custom.widget();
        let theme = widget.borrow().theme();
        widget
            .borrow()
            .height_for(width.value(), &theme)
            .map_or_else(
                |_| crate::units::dip(widget::DEFAULT_SIZE * 1.4),
                crate::units::dip,
            )
    }
}

impl<M: 'static> AsControl for FlowText<M> {
    fn control(&self) -> &Control {
        self.custom.control()
    }
}

impl<M: 'static> Themed for FlowText<M> {
    fn apply_theme(&self, theme: &Theme) {
        self.custom.apply_theme(theme);
    }
}
