//! [`Layout`]: laid-out text with hit testing and selection boxes.

use std::ops::Range;

use crate::d2d::RectF;
use crate::sys::d2d::text::TextLayout;

use super::index::{advance, to_utf16};

/// The width of a caret box, in device-independent pixels.
const CARET_WIDTH: f32 = 1.0;

/// One line of a [`Layout`].
#[derive(Clone, Debug, PartialEq)]
pub struct LineMetrics {
    /// The bytes of the text on this line, including a line break and
    /// trailing whitespace.
    pub range: Range<usize>,
    /// Distance from the top of the layout to the top of the line.
    pub top: f32,
    /// The line's height.
    pub height: f32,
    /// Distance from the top of the line to its baseline.
    pub baseline: f32,
}

/// The result of [`Layout::hit_test_point`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HitTest {
    /// The caret position (byte offset) nearest the point: before the
    /// character hit when the point is on its leading half, after it otherwise.
    pub index: usize,
    /// Whether the point is inside the text rather than beside or past it.
    pub inside: bool,
}

/// Text laid out by a [`Font`](super::Font), ready to draw.
///
/// `Send + Sync`. Coordinates are relative to the layout's top-left corner.
/// Text positions are UTF-8 byte offsets; offsets past the end clamp to it and
/// offsets inside a character snap to its start. Queries that DirectWrite
/// cannot answer (which does not happen for a valid layout) return an empty
/// result.
pub struct Layout {
    text: Box<str>,
    sys: TextLayout,
}

impl Layout {
    pub(super) fn new(text: &str, sys: TextLayout) -> Layout {
        Layout {
            text: text.into(),
            sys,
        }
    }

    pub(crate) fn sys(&self) -> &TextLayout {
        &self.sys
    }

    /// The laid-out text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The (width, height) the text occupies: the widest line, trailing
    /// spaces included, by the height of all lines.
    pub fn size(&self) -> (f32, f32) {
        self.sys.size()
    }

    /// The width of the widest line.
    pub fn width(&self) -> f32 {
        self.size().0
    }

    /// The height of all lines.
    pub fn height(&self) -> f32 {
        self.size().1
    }

    /// The lines, top to bottom.
    pub fn lines(&self) -> Vec<LineMetrics> {
        let Ok(lines) = self.sys.lines() else {
            return Vec::new();
        };
        let (mut top, mut start) = (0.0, 0);
        lines
            .into_iter()
            .map(|line| {
                let end = advance(&self.text, start, line.length as usize);
                let metrics = LineMetrics {
                    range: start..end,
                    top,
                    height: line.height,
                    baseline: line.baseline,
                };
                top += line.height;
                start = end;
                metrics
            })
            .collect()
    }

    /// The caret position nearest the point `(x, y)`.
    pub fn hit_test_point(&self, x: f32, y: f32) -> HitTest {
        let Ok(hit) = self.sys.hit_test_point(x, y) else {
            return HitTest {
                index: 0,
                inside: false,
            };
        };
        let start = advance(&self.text, 0, hit.position as usize);
        let index = if hit.trailing {
            advance(&self.text, start, hit.length as usize)
        } else {
            start
        };
        HitTest {
            index,
            inside: hit.inside,
        }
    }

    /// The caret box at byte offset `index`: one DIP wide, as tall as its line.
    pub fn caret_rect(&self, index: usize) -> RectF {
        let position = to_utf16(&self.text, index) as u32;
        self.sys.caret(position).map_or_else(
            |_| RectF::default(),
            |caret| {
                RectF::new(
                    caret.left,
                    caret.top,
                    caret.left + CARET_WIDTH,
                    caret.bottom,
                )
            },
        )
    }

    /// The boxes covering the text from byte `start` up to `end`, one per line
    /// (more where the text changes direction), for painting a selection
    /// behind the text.
    pub fn selection_rects(&self, start: usize, end: usize) -> Vec<RectF> {
        let (start, end) = (to_utf16(&self.text, start), to_utf16(&self.text, end));
        if end <= start {
            return Vec::new();
        }
        self.sys
            .range_boxes(start as u32, (end - start) as u32)
            .unwrap_or_default()
    }
}
