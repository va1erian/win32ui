#![forbid(unsafe_code)]

//! The progress bar's two renderers: Direct2D (anti-aliased, the default) and
//! GDI (the fallback when Direct2D cannot be created).

use crate::d2d::{D2dCanvas, RectF};
use crate::gdi::Canvas;

use super::state::ProgressBarState;

impl ProgressBarState {
    /// Draws the bar with anti-aliased, fully round ends. Geometry is in
    /// device-independent pixels, so the caps are exact semicircles at any DPI.
    pub(super) fn draw_d2d(&self, canvas: &mut D2dCanvas) {
        canvas.clear(self.theme.background);
        let track = canvas.bounds();
        canvas.fill_rounded_rect(track, track.pill_radius(), self.theme.track);

        let (left, width) = if self.marquee {
            self.marquee_span(track.width())
        } else {
            (0.0, self.value_width(track.width()))
        };
        if width > 0.0 {
            let fill = RectF::new(
                track.left + left,
                track.top,
                track.left + left + width,
                track.bottom,
            );
            canvas.fill_rounded_rect(fill, fill.pill_radius(), self.fill_color());
        }
    }

    /// Draws the bar with GDI: stair-stepped ends, but no dependency on Direct2D.
    pub(super) fn draw_gdi(&self, canvas: &Canvas) {
        let radius = self.bounds.height().max(1);
        canvas.round_rect(self.bounds, radius, self.theme.track, None);

        let fill = if self.marquee {
            self.marquee_fill()
        } else {
            self.value_fill()
        };
        if let Some(rect) = fill {
            let radius = self.bounds.height().min(rect.width()).max(1);
            canvas.round_rect(rect, radius, self.fill_color(), None);
        }
    }
}
