//! A DirectWrite text layout whose ranges carry their own style: the raw half
//! of [`RichLayout`](crate::d2d::RichLayout).
//!
//! The layout is created exactly like a plain one (so wrapping, bidi and hit
//! testing are DirectWrite's); the spans are then applied as per-range
//! attributes (`SetFontSize`, `SetFontWeight`, `SetFontStyle`, `SetUnderline`)
//! and remembered so the draw path can set a per-range brush.

use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FONT_STYLE_ITALIC, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT, DWRITE_TEXT_RANGE,
    IDWriteTextLayout,
};

use crate::d2d::{RectF, Rgba};
use crate::error::Result;

use super::layout::{PointHit, TextLayout};

/// The style applied to one UTF-16 range of a rich layout.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RichStyle {
    /// First UTF-16 unit of the range.
    pub(crate) start: u32,
    /// UTF-16 units in the range.
    pub(crate) length: u32,
    /// Em size in device-independent pixels.
    pub(crate) size: f32,
    /// Weight 100-900.
    pub(crate) weight: u16,
    /// Italic.
    pub(crate) italic: bool,
    /// Whether the range is underlined.
    pub(crate) underline: bool,
    /// The colour the range draws with.
    pub(crate) color: Rgba,
}

/// A laid-out block of text with per-range styling.
pub(crate) struct RichTextLayout {
    layout: TextLayout,
    styles: Box<[RichStyle]>,
}

impl RichTextLayout {
    pub(super) fn new(layout: TextLayout, styles: Box<[RichStyle]>) -> RichTextLayout {
        RichTextLayout { layout, styles }
    }

    /// The DirectWrite layout, for drawing.
    pub(crate) fn raw(&self) -> &IDWriteTextLayout {
        self.layout.raw()
    }

    /// The applied ranges, in text order.
    pub(crate) fn styles(&self) -> &[RichStyle] {
        &self.styles
    }

    /// The overall (width, height).
    pub(crate) fn size(&self) -> (f32, f32) {
        self.layout.size()
    }

    pub(crate) fn hit_test_point(&self, x: f32, y: f32) -> Result<PointHit> {
        self.layout.hit_test_point(x, y)
    }

    /// The boxes covering `length` UTF-16 units from `position`, one per line
    /// (or bidi run) crossed.
    pub(crate) fn range_boxes(&self, position: u32, length: u32) -> Result<Vec<RectF>> {
        self.layout.range_boxes(position, length)
    }

    /// Applies `style` as per-range attributes. Called once per range, right
    /// after the layout is created.
    pub(crate) fn apply(&self, style: &RichStyle) -> Result<()> {
        let range = DWRITE_TEXT_RANGE {
            startPosition: style.start,
            length: style.length,
        };
        // SAFETY: `raw` is a live layout, `range` lies inside its text, and
        // every other argument is a plain value.
        unsafe {
            self.raw()
                .SetFontSize(style.size, range)
                .map_err(crate::sys::win32_error)?;
            self.raw()
                .SetFontWeight(DWRITE_FONT_WEIGHT(i32::from(style.weight)), range)
                .map_err(crate::sys::win32_error)?;
            let font_style = if style.italic {
                DWRITE_FONT_STYLE_ITALIC
            } else {
                DWRITE_FONT_STYLE_NORMAL
            };
            self.raw()
                .SetFontStyle(font_style, range)
                .map_err(crate::sys::win32_error)?;
            self.raw()
                .SetUnderline(style.underline, range)
                .map_err(crate::sys::win32_error)?;
        }
        Ok(())
    }
}
