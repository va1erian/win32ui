#![forbid(unsafe_code)]

//! The progress bar's data and the geometry derived from it. Pure logic, so it
//! is unit-tested without a window; both renderers draw from it.

use std::ops::RangeInclusive;

use crate::color::Color;
use crate::controls::progressbar_theme::ProgressBarTheme;
use crate::geometry::Rect;
use crate::message::TimerId;

/// How far the marquee highlight travels per tick, as a fraction of its range.
const MARQUEE_STEP: f32 = 0.02;
/// The marquee highlight's width, as a fraction of the bar's width.
const MARQUEE_FRACTION: f32 = 0.3;

/// The visual state of a progress bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgressState {
    /// Normal progress (accent fill).
    Normal,
    /// Paused (caution fill).
    Paused,
    /// Error (danger fill).
    Error,
}

pub(super) struct ProgressBarState {
    pub(super) theme: ProgressBarTheme,
    pub(super) min: i32,
    pub(super) max: i32,
    pub(super) value: i32,
    pub(super) state: ProgressState,
    pub(super) marquee: bool,
    pub(super) offset: f32,
    pub(super) timer: Option<TimerId>,
    pub(super) bounds: Rect,
}

impl ProgressBarState {
    pub(super) fn fill_color(&self) -> Color {
        match self.state {
            ProgressState::Normal => self.theme.fill,
            ProgressState::Paused => self.theme.paused,
            ProgressState::Error => self.theme.error,
        }
    }

    /// Stores `range`, swapping a reversed range so `min <= max` always holds.
    pub(super) fn set_range(&mut self, range: RangeInclusive<i32>) {
        let (mut start, mut end) = (*range.start(), *range.end());
        if start > end {
            std::mem::swap(&mut start, &mut end);
        }
        self.min = start;
        self.max = end;
        self.value = self.value.clamp(start, end);
    }

    /// Stores `value`, clamped to the current range.
    pub(super) fn set_value(&mut self, value: i32) {
        self.value = value.clamp(self.min, self.max);
    }

    /// Advances the marquee highlight, wrapping at the end.
    pub(super) fn advance_marquee(&mut self) {
        if !self.marquee {
            return;
        }
        self.offset += MARQUEE_STEP;
        if self.offset > 1.0 {
            self.offset = 0.0;
        }
    }

    /// The filled fraction of the range, `0.0..=1.0`.
    pub(super) fn value_fraction(&self) -> f32 {
        let span = self.max - self.min;
        if span <= 0 {
            return 0.0;
        }
        (self.value - self.min) as f32 / span as f32
    }

    /// The filled width of a bar `total` wide.
    pub(super) fn value_width(&self, total: f32) -> f32 {
        total * self.value_fraction()
    }

    /// The marquee highlight's `(left, width)` on a bar `total` wide.
    pub(super) fn marquee_span(&self, total: f32) -> (f32, f32) {
        let width = total * MARQUEE_FRACTION;
        (((total - width).max(0.0)) * self.offset, width)
    }

    /// The rectangle of the filled portion in pixels, if any (GDI path).
    pub(super) fn value_fill(&self) -> Option<Rect> {
        let total = self.bounds.width();
        if total <= 0 {
            return None;
        }
        let width = (self.value_width(total as f32).round() as i32).min(total);
        (width > 0).then(|| {
            Rect::new(
                self.bounds.left,
                self.bounds.top,
                self.bounds.left + width,
                self.bounds.bottom,
            )
        })
    }

    /// The rectangle of the marquee highlight in pixels (GDI path).
    pub(super) fn marquee_fill(&self) -> Option<Rect> {
        let (left, width) = self.marquee_span(self.bounds.width() as f32);
        let (left, width) = (left.round() as i32, width as i32);
        (width > 0).then(|| {
            Rect::new(
                self.bounds.left + left,
                self.bounds.top,
                self.bounds.left + left + width,
                self.bounds.bottom,
            )
        })
    }
}
