//! A DirectWrite text layout: measurement, hit testing and selection boxes.
//!
//! Positions here are UTF-16 code units, DirectWrite's native unit; the safe
//! layer converts to and from byte offsets.

use windows::Win32::Graphics::DirectWrite::{
    DWRITE_HIT_TEST_METRICS, DWRITE_LINE_METRICS, DWRITE_TEXT_METRICS, IDWriteTextLayout,
};
use windows::core::BOOL;

use crate::d2d::RectF;
use crate::error::Result;
use crate::sys::win32_error;

/// `HRESULT_FROM_WIN32(ERROR_INSUFFICIENT_BUFFER)`, which the range and line
/// queries report (with the needed count) when the buffer is too small.
const INSUFFICIENT_BUFFER: i32 = 0x8007_007A_u32 as i32;

/// One laid-out line, from `GetLineMetrics`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LineMetrics {
    /// UTF-16 units in the line, newline and trailing whitespace included.
    pub(crate) length: u32,
    pub(crate) height: f32,
    pub(crate) baseline: f32,
}

/// The outcome of a point hit test.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PointHit {
    /// UTF-16 offset of the hit cluster.
    pub(crate) position: u32,
    /// UTF-16 units in the hit cluster.
    pub(crate) length: u32,
    /// Whether the point is on the trailing half of the cluster.
    pub(crate) trailing: bool,
    pub(crate) inside: bool,
}

/// A laid-out block of text.
pub(crate) struct TextLayout {
    layout: IDWriteTextLayout,
}

impl TextLayout {
    pub(super) fn new(layout: IDWriteTextLayout) -> TextLayout {
        TextLayout { layout }
    }

    /// The DirectWrite layout, for drawing.
    pub(crate) fn raw(&self) -> &IDWriteTextLayout {
        &self.layout
    }

    fn metrics(&self) -> DWRITE_TEXT_METRICS {
        let mut metrics = DWRITE_TEXT_METRICS::default();
        // SAFETY: the out pointer is a valid local. A failure leaves the zeroed
        // default, which reads as an empty layout.
        let _ = unsafe { self.layout.GetMetrics(&mut metrics) };
        metrics
    }

    /// The widest line, trailing whitespace included.
    pub(crate) fn width_with_trailing(&self) -> f32 {
        self.metrics().widthIncludingTrailingWhitespace
    }

    /// The overall (width, height).
    pub(crate) fn size(&self) -> (f32, f32) {
        let metrics = self.metrics();
        (metrics.widthIncludingTrailingWhitespace, metrics.height)
    }

    pub(crate) fn lines(&self) -> Result<Vec<LineMetrics>> {
        let mut raw = vec![DWRITE_LINE_METRICS::default(); self.metrics().lineCount as usize];
        let mut count = 0;
        // SAFETY: `raw` is writable for its length and `count` is a valid local.
        unsafe { self.layout.GetLineMetrics(Some(&mut raw), &mut count) }.map_err(win32_error)?;
        raw.truncate(count as usize);
        Ok(raw
            .iter()
            .map(|line| LineMetrics {
                length: line.length,
                height: line.height,
                baseline: line.baseline,
            })
            .collect())
    }

    pub(crate) fn hit_test_point(&self, x: f32, y: f32) -> Result<PointHit> {
        let (mut trailing, mut inside) = (BOOL(0), BOOL(0));
        let mut metrics = DWRITE_HIT_TEST_METRICS::default();
        // SAFETY: every out pointer is a valid local.
        unsafe {
            self.layout
                .HitTestPoint(x, y, &mut trailing, &mut inside, &mut metrics)
        }
        .map_err(win32_error)?;
        Ok(PointHit {
            position: metrics.textPosition,
            length: metrics.length,
            trailing: trailing.as_bool(),
            inside: inside.as_bool(),
        })
    }

    /// The caret box (zero width) at UTF-16 offset `position`.
    pub(crate) fn caret(&self, position: u32) -> Result<RectF> {
        let (mut x, mut y) = (0.0, 0.0);
        let mut metrics = DWRITE_HIT_TEST_METRICS::default();
        // SAFETY: every out pointer is a valid local.
        unsafe {
            self.layout
                .HitTestTextPosition(position, false, &mut x, &mut y, &mut metrics)
        }
        .map_err(win32_error)?;
        Ok(RectF::new(x, y, x, y + metrics.height))
    }

    /// The boxes covering `length` UTF-16 units from `position`, one per
    /// line (or bidi run) crossed.
    pub(crate) fn range_boxes(&self, position: u32, length: u32) -> Result<Vec<RectF>> {
        let mut boxes = vec![DWRITE_HIT_TEST_METRICS::default(); 8];
        let mut count = 0;
        // SAFETY: `boxes` is writable for its length and `count` is a valid local.
        let mut outcome = unsafe {
            self.layout
                .HitTestTextRange(position, length, 0.0, 0.0, Some(&mut boxes), &mut count)
        };
        if matches!(&outcome, Err(error) if error.code().0 == INSUFFICIENT_BUFFER) {
            boxes.resize(count as usize, DWRITE_HIT_TEST_METRICS::default());
            // SAFETY: as above, with the buffer now as large as DirectWrite asked.
            outcome = unsafe {
                self.layout.HitTestTextRange(
                    position,
                    length,
                    0.0,
                    0.0,
                    Some(&mut boxes),
                    &mut count,
                )
            };
        }
        outcome.map_err(win32_error)?;
        boxes.truncate(count as usize);
        Ok(boxes
            .iter()
            .map(|b| RectF::new(b.left, b.top, b.left + b.width, b.top + b.height))
            .collect())
    }
}
