#![forbid(unsafe_code)]

//! An owner-drawn status bar.
//!
//! The native `msctls_statusbar32` can't be given a dark palette (it exposes
//! no text-colour API), so this is a small custom child window that paints its
//! parts itself. It is a [`CustomWidget`](crate::CustomWidget), so it shares
//! the crate's single owner-draw pattern with the toolbar and user widgets.

use crate::app::Ui;
use crate::color::Color;
use crate::controls::control::{AsControl, Control};
use crate::controls::custom::{Custom, CustomWidget};
use crate::error::Result;
use crate::gdi::{Canvas, Font, TextFormat};
use crate::geometry::{Rect, Size};
use crate::theme::{Theme, Themed};
use crate::units::dip;

/// Colours for the status bar.
#[derive(Clone, Copy, Debug)]
pub struct StatusBarTheme {
    /// Background.
    pub background: Color,
    /// Text.
    pub text: Color,
    /// Separator/border colour.
    pub border: Color,
}

impl StatusBarTheme {
    /// Derives a palette from the app [`Theme`]. Override any field after
    /// calling this for a custom look.
    pub fn from_theme(theme: &Theme) -> StatusBarTheme {
        StatusBarTheme {
            background: theme.surface,
            text: theme.text_secondary,
            border: theme.border,
        }
    }
}

/// The mutable state behind a [`StatusBar`], shared with the child window.
struct StatusBarWidget {
    parts: Vec<i32>,
    texts: Vec<String>,
    font: Font,
    height: i32,
}

impl StatusBarWidget {
    fn part_edges(&self, bounds: Rect) -> Vec<i32> {
        self.parts
            .iter()
            .map(|edge| if *edge < 0 { bounds.right } else { *edge })
            .collect()
    }

    fn draw(&self, canvas: &Canvas, bounds: Rect, theme: &StatusBarTheme) {
        canvas.fill_rect(bounds, theme.background);
        canvas.fill_rect(
            Rect::new(bounds.left, bounds.top, bounds.right, bounds.top + 1),
            theme.border,
        );

        let mut left = bounds.left;
        for (index, &right) in self.part_edges(bounds).iter().enumerate() {
            if index > 0 && right > left {
                canvas.fill_rect(
                    Rect::new(left, bounds.top + 3, left + 1, bounds.bottom - 3),
                    theme.border,
                );
            }
            let text = self.texts.get(index).map(String::as_str).unwrap_or("");
            if !text.is_empty() {
                let cell = Rect::new(left + 8, bounds.top, right - 4, bounds.bottom);
                canvas.with_font(&self.font, |canvas| {
                    canvas.draw_text(
                        cell,
                        text,
                        theme.text,
                        TextFormat::left()
                            .single_line()
                            .vcenter()
                            .end_ellipsis()
                            .no_prefix(),
                    );
                });
            }
            left = right;
        }
    }
}

impl CustomWidget for StatusBarWidget {
    /// The status bar raises no events.
    type Event = ();

    fn paint(&self, canvas: &Canvas, bounds: Rect, theme: &Theme) {
        let theme = StatusBarTheme::from_theme(theme);
        self.draw(canvas, bounds, &theme);
    }

    fn preferred_size(&self, _dpi: u32) -> Option<Size> {
        Some(Size::new(0, self.height))
    }
}

/// An owner-drawn status bar with parts and text.
pub struct StatusBar<M: 'static> {
    custom: Custom<StatusBarWidget, M>,
}

impl<M: 'static> StatusBar<M> {
    /// Creates the bar as a child of the window behind `ui`, adopting `ui`'s
    /// theme. Use [`Themed::apply_theme`] for a one-off override.
    pub fn new(ui: &mut Ui<M>) -> Result<StatusBar<M>> {
        let dpi = ui.dpi();
        let height = dip(22.0).to_px(dpi).value();
        let widget = StatusBarWidget {
            parts: vec![-1],
            texts: Vec::new(),
            font: Font::system_ui(dpi)?,
            height,
        };
        let custom = Custom::new(ui, widget)?;
        Ok(StatusBar { custom })
    }

    /// Splits the bar into parts whose right edges are given in client
    /// coordinates. Use a negative edge (e.g. `-1`) for "extend to the right".
    pub fn set_parts(&self, edges: &[i32]) {
        self.custom.widget().borrow_mut().parts = edges.to_vec();
        self.custom.invalidate();
    }

    /// Sets the text shown in one part.
    pub fn set_text(&self, part: usize, text: &str) {
        {
            let widget = self.custom.widget();
            let mut widget = widget.borrow_mut();
            if widget.texts.len() <= part {
                widget.texts.resize(part + 1, String::new());
            }
            widget.texts[part] = text.to_string();
        }
        self.custom.invalidate();
    }
}

impl<M: 'static> AsControl for StatusBar<M> {
    fn control(&self) -> &Control {
        self.custom.control()
    }
}

impl<M: 'static> Themed for StatusBar<M> {
    fn apply_theme(&self, theme: &Theme) {
        self.custom.apply_theme(theme);
    }
}
