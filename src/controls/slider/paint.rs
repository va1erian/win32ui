#![forbid(unsafe_code)]

//! The slider's Direct2D painting. Geometry is in device-independent pixels
//! and the thumb sits at a fractional position, so it glides between device
//! pixels with anti-aliased edges. Nothing here allocates: the canvas caches
//! its brushes, and every colour is a fixed theme token.

use crate::color::Color;
use crate::d2d::{D2dCanvas, PointF, RectF, Stroke};
use crate::theme::Theme;

use super::state::SliderState;

/// The track's thickness at rest and fully hovered.
const TRACK: f32 = 4.0;
const TRACK_HOT: f32 = 6.0;
/// The thumb's outer radius at rest, and how much hover and press add.
const THUMB: f32 = 9.0;
const THUMB_HOVER_GROWTH: f32 = 1.0;
const THUMB_PRESS_GROWTH: f32 = 0.5;
/// The thumb's inner accent dot: radius at rest and the growth per channel.
const DOT: f32 = 4.5;
const DOT_HOVER_GROWTH: f32 = 1.5;
const DOT_PRESS_GROWTH: f32 = 0.5;
/// The gap between the thumb and its focus ring, and the ring's full width.
const RING_GAP: f32 = 2.0;
const RING_WIDTH: f32 = 2.0;
/// Tick marks: the distance from the track's edge, and their length.
const TICK_OFFSET: f32 = 4.0;
const TICK_LENGTH: f32 = 4.0;

/// The colours one frame paints with, derived once per frame from the theme.
struct Palette {
    track: Color,
    buffered: Color,
    fill: Color,
    thumb_ring: Color,
    thumb_face: Color,
    dot: Color,
    focus: Color,
    tick: Color,
}

impl Palette {
    fn new(theme: &Theme, enabled: bool) -> Palette {
        let accent = if enabled {
            theme.accent
        } else {
            theme.text_disabled
        };
        Palette {
            track: theme.scrollbar,
            buffered: theme.scrollbar.lerp(theme.accent, 0.4),
            fill: accent,
            thumb_ring: theme.scrollbar,
            thumb_face: theme.raised,
            dot: accent,
            focus: theme.border_focused,
            tick: theme.text_secondary,
        }
    }
}

/// Maps `(main, cross)` axis coordinates to canvas coordinates: the main axis
/// is x for a horizontal slider and y for a vertical one.
#[derive(Clone, Copy)]
struct Layout {
    vertical: bool,
}

impl Layout {
    fn point(self, main: f32, cross: f32) -> PointF {
        if self.vertical {
            PointF::new(cross, main)
        } else {
            PointF::new(main, cross)
        }
    }

    fn rect(self, main: (f32, f32), cross: (f32, f32)) -> RectF {
        if self.vertical {
            RectF::new(cross.0, main.0, cross.1, main.1)
        } else {
            RectF::new(main.0, cross.0, main.1, cross.1)
        }
    }
}

impl SliderState {
    /// Draws the slider into `bounds` (device-independent pixels).
    pub(super) fn draw(&self, canvas: &mut D2dCanvas<'_>, bounds: RectF, theme: &Theme) {
        let layout = Layout {
            vertical: self.vertical,
        };
        let (length, breadth) = if self.vertical {
            (bounds.height(), bounds.width())
        } else {
            (bounds.width(), bounds.height())
        };
        let axis = self.axis(f64::from(length));
        let palette = Palette::new(theme, self.enabled);
        let pos = |value: f64| axis.pos_of(self.fraction(value)) as f32;

        let hover = self.anim.hover();
        let press = self.anim.press();
        let focus = self.anim.focus();
        let middle = breadth / 2.0;
        let half_track = (TRACK + (TRACK_HOT - TRACK) * hover) / 2.0;
        let cross = (middle - half_track, middle + half_track);
        let (start, end) = (axis.pos_of(0.0) as f32, axis.pos_of(1.0) as f32);
        let thumb = pos(self.value());

        let pill = |canvas: &mut D2dCanvas<'_>, from: f32, to: f32, color: Color| {
            let (low, high) = if from <= to { (from, to) } else { (to, from) };
            let rect = layout.rect((low, high), cross);
            canvas.fill_rounded_rect(rect, rect.pill_radius(), color);
        };

        pill(canvas, start, end, palette.track);
        if let Some((low, high)) = self.buffered {
            pill(canvas, pos(low), pos(high), palette.buffered);
        }
        pill(canvas, start, thumb, palette.fill);
        self.draw_ticks(canvas, layout, (start, end), middle + half_track, &palette);

        let center = layout.point(thumb, middle);
        let outer = THUMB + THUMB_HOVER_GROWTH * hover + THUMB_PRESS_GROWTH * press;
        let dot = DOT + DOT_HOVER_GROWTH * hover + DOT_PRESS_GROWTH * press;
        canvas.fill_ellipse(center, outer, outer, palette.thumb_ring);
        canvas.fill_ellipse(center, outer - 1.0, outer - 1.0, palette.thumb_face);
        canvas.fill_ellipse(center, dot, dot, palette.dot);
        if focus > 0.0 {
            let radius = outer + RING_GAP + RING_WIDTH * focus / 2.0;
            let stroke = Stroke::solid(RING_WIDTH * focus);
            canvas.stroke_ellipse(center, radius, radius, palette.focus, stroke);
        }
    }

    fn draw_ticks(
        &self,
        canvas: &mut D2dCanvas<'_>,
        layout: Layout,
        (start, end): (f32, f32),
        track_edge: f32,
        palette: &Palette,
    ) {
        if self.ticks == 0 {
            return;
        }
        let from = track_edge + TICK_OFFSET;
        for index in 0..=self.ticks {
            let main = start + (end - start) * index as f32 / self.ticks as f32;
            let rect = layout.rect((main - 0.5, main + 0.5), (from, from + TICK_LENGTH));
            canvas.fill_rect(rect, palette.tick);
        }
    }
}
