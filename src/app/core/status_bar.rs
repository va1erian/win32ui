#![forbid(unsafe_code)]

//! The material status bar half of [`Core`](super::Core): the bottom band the
//! top-level window paints on the backdrop material, alongside the strip menu.
//!
//! A child `StatusBar` window cannot show the parent's DWM material (child
//! HWNDs composite opaquely), so this bar is not a child at all: the app
//! installs a [`MaterialStatusBarState`] and reserves the band, and the
//! top-level transparent Direct2D surface draws it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use super::Core;
use crate::d2d::{D2dCanvas, Font, FontSpec, Layout, PointF, RectF, Rgba, TextSystem};
use crate::sys;
use crate::theme::Theme;
use crate::units::dip;

/// The material status bar's design height, in device-independent pixels.
pub(crate) const HEIGHT_DIP: f32 = 22.0;
/// Status bar font size, in device-independent pixels.
const FONT_SIZE: f32 = 12.0;
/// The left inset of a part's text.
const TEXT_INSET: f32 = 8.0;

/// The shared menu font, resolved once per UI thread. `None` when DirectWrite
/// is unavailable, in which case the material status bar cannot be built.
pub(crate) fn font() -> Option<Font> {
    thread_local! {
        static FONT: RefCell<Option<Font>> = const { RefCell::new(None) };
    }
    FONT.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            let system = TextSystem::new().ok()?;
            let spec = FontSpec::new("Segoe UI, sans-serif", FONT_SIZE);
            *slot = Some(system.font(&spec).ok()?);
        }
        slot.clone()
    })
}

/// The mutable state behind a [`MaterialStatusBar`](crate::MaterialStatusBar),
/// shared with the top-level painter.
pub(crate) struct MaterialStatusBarState {
    /// Part edges in client coordinates; a negative edge means "to the right".
    pub(crate) parts: RefCell<Vec<i32>>,
    /// One text per part.
    pub(crate) texts: RefCell<Vec<String>>,
    /// The laid-out texts, rebuilt when the texts change so painting allocates
    /// nothing.
    layouts: RefCell<Vec<Layout>>,
    /// The cached line height of the status bar font, in device-independent
    /// pixels.
    line_height_dip: Cell<f32>,
}

impl MaterialStatusBarState {
    /// Creates empty state with a single full-width part, or `None` when
    /// DirectWrite is unavailable (the caller falls back to the child bar).
    pub(crate) fn new() -> Option<Rc<MaterialStatusBarState>> {
        let line_height_dip = font()?.metrics().line_height();
        Some(Rc::new(MaterialStatusBarState {
            parts: RefCell::new(vec![-1]),
            texts: RefCell::new(Vec::new()),
            layouts: RefCell::new(Vec::new()),
            line_height_dip: Cell::new(line_height_dip),
        }))
    }

    /// Rebuilds the cached text layouts (after a text change or font reload).
    pub(crate) fn rebuild(&self) {
        let Some(font) = font() else {
            return;
        };
        self.line_height_dip.set(font.metrics().line_height());
        let texts = self.texts.borrow();
        let mut layouts = Vec::with_capacity(texts.len());
        for text in texts.iter() {
            match font.layout(text, f32::INFINITY) {
                Ok(layout) => layouts.push(layout),
                Err(_) => break,
            }
        }
        *self.layouts.borrow_mut() = layouts;
    }
}

impl<M: 'static> Core<M> {
    /// Installs the material status bar: records it and extends the frame over
    /// the bottom band.
    pub(crate) fn set_material_status_bar(&self, state: Rc<MaterialStatusBarState>) {
        *self.material_status_bar.borrow_mut() = Some(state);
        self.refresh_material_status_bar();
    }

    /// Whether a material status bar is installed.
    pub(crate) fn has_material_status_bar(&self) -> bool {
        self.material_status_bar.borrow().is_some()
    }

    /// The material status bar's height in device pixels (0 when not
    /// installed), as published to the window.
    pub(crate) fn material_status_bar_height_px(&self) -> i32 {
        crate::window::nc::status_bar(self.hwnd.get())
    }

    /// Recomputes the bottom band's height for the window's DPI and re-applies
    /// the extended frame.
    pub(crate) fn refresh_material_status_bar(&self) {
        let hwnd = self.hwnd.get();
        if hwnd.is_null() {
            return;
        }
        let height = if self.has_material_status_bar() {
            dip(HEIGHT_DIP).to_px(sys::dpi::window_dpi(hwnd)).value()
        } else {
            0
        };
        crate::window::nc::set_status_bar(hwnd, height);
        sys::nc::apply_extended_frame(hwnd);
    }

    /// Paints the bottom material status bar: the parts, their separators and
    /// text, over the transparent band. When the material is not active the
    /// band is filled with the opaque status-bar background instead, so the
    /// fallback matches the child bar.
    pub(crate) fn paint_material_status_bar(
        &self,
        canvas: &mut D2dCanvas,
        dpi: u32,
        theme: &Theme,
    ) {
        let Some(state) = self.material_status_bar.borrow().as_ref().cloned() else {
            return;
        };
        let hwnd = self.hwnd.get();
        let band = crate::window::nc::status_bar(hwnd);
        if band <= 0 {
            return;
        }
        let client = sys::window::client_rect(hwnd);
        let scale = dpi as f32 / 96.0;
        let (width_dip, top_dip, bottom_dip) = (
            client.right as f32 / scale,
            (client.bottom - band) as f32 / scale,
            client.bottom as f32 / scale,
        );
        let paint = crate::StatusBarTheme::from_theme(theme);

        if !crate::theme::backdrop_active(hwnd) {
            canvas.fill_rect_rgba(
                RectF::new(0.0, top_dip, width_dip, bottom_dip),
                Rgba::from(paint.background),
            );
        }
        canvas.fill_rect_rgba(
            RectF::new(
                0.0,
                top_dip,
                width_dip,
                (client.bottom - band + 1) as f32 / scale,
            ),
            Rgba::from(paint.border),
        );

        let parts = state.parts.borrow();
        let layouts = state.layouts.borrow();
        let line_height_dip = state.line_height_dip.get();
        let mut left = 0;
        for (index, &edge) in parts.iter().enumerate() {
            let right = if edge < 0 { client.right } else { edge };
            if index > 0 && right > left {
                let x = left as f32 / scale;
                canvas.fill_rect_rgba(
                    RectF::new(
                        x,
                        (client.bottom - band + 3) as f32 / scale,
                        x + 1.0 / scale,
                        (client.bottom - 3) as f32 / scale,
                    ),
                    Rgba::from(paint.border),
                );
            }
            if let Some(layout) = layouts.get(index)
                && !layout.text().is_empty()
            {
                let top = top_dip + (band as f32 / scale - line_height_dip) / 2.0;
                canvas.draw_text(
                    layout,
                    PointF::new(left as f32 / scale + TEXT_INSET, top.max(top_dip)),
                    paint.text,
                );
            }
            left = right;
        }
    }
}
