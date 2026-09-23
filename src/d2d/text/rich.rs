#![forbid(unsafe_code)]

//! Rich text: one wrapped DirectWrite layout whose runs carry their own style.
//!
//! DirectWrite formats ranges of a single `IDWriteTextLayout`, so word wrap,
//! bidi reordering and hit testing span every run for free — no per-run line
//! breaking. Build a [`RichLayout`] from a [`Font`] and a list of [`Span`]s,
//! draw it with [`D2dCanvas::draw_rich_text`](crate::d2d::D2dCanvas::draw_rich_text),
//! and query it with [`RichLayout::hit_test_point`] and
//! [`RichLayout::rects_of_span`].

use std::ops::Range;

use crate::d2d::{RectF, Rgba};
use crate::sys::d2d::text::RichTextLayout;

use super::index::advance;

/// One styled run of a [`RichLayout`].
///
/// Build it with [`Span::new`] and the chaining setters; a plain `new` span is
/// regular, upright, black and un-underlined at the font's size.
#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    /// The run's text.
    pub text: String,
    /// Weight 100-900; 400 is regular, 700 bold. Clamped when laid out.
    pub weight: u16,
    /// Italic.
    pub italic: bool,
    /// The glyph colour.
    pub color: Rgba,
    /// Whether DirectWrite underlines the run.
    pub underline: bool,
    /// The em size in device-independent pixels; `None` uses the font's size,
    /// so runs of different sizes share one baseline automatically.
    pub size_dip: Option<f32>,
}

impl Span {
    /// A regular, upright, black span at the font's size.
    pub fn new(text: impl Into<String>) -> Span {
        Span {
            text: text.into(),
            weight: 400,
            italic: false,
            color: Rgba::BLACK,
            underline: false,
            size_dip: None,
        }
    }

    /// Sets the weight (100-900).
    pub fn weight(mut self, weight: u16) -> Span {
        self.weight = weight;
        self
    }

    /// Sets italic.
    pub fn italic(mut self, italic: bool) -> Span {
        self.italic = italic;
        self
    }

    /// Sets the glyph colour.
    pub fn color(mut self, color: impl Into<Rgba>) -> Span {
        self.color = color.into();
        self
    }

    /// Sets whether the run is underlined.
    pub fn underline(mut self, underline: bool) -> Span {
        self.underline = underline;
        self
    }

    /// Sets the em size in device-independent pixels.
    pub fn size(mut self, size_dip: f32) -> Span {
        self.size_dip = Some(size_dip);
        self
    }
}

/// Where a point falls in a [`RichLayout`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RichHit {
    /// The index of the span the point hit.
    pub span: usize,
    /// The byte offset within that span's text.
    pub offset: usize,
    /// Whether the point is inside the text rather than beside or past it.
    pub inside: bool,
}

/// One span's position: byte and UTF-16 ranges in the concatenated text.
#[derive(Clone, Debug)]
struct Meta {
    range: Range<usize>,
    utf16: Range<u32>,
}

/// Text laid out by a [`Font`](super::Font), each [`Span`] styled, ready to
/// draw.
///
/// `Send + Sync`; coordinates are relative to the layout's top-left corner and
/// measured in device-independent pixels. Byte offsets in [`RichHit`] are into
/// the hit span's own [`text`](RichLayout::span_text).
pub struct RichLayout {
    text: Box<str>,
    spans: Box<[Meta]>,
    sys: RichTextLayout,
}

impl RichLayout {
    pub(super) fn new(text: String, spans: &[Span], sys: RichTextLayout) -> RichLayout {
        let mut metas = Vec::with_capacity(spans.len());
        let (mut byte, mut unit) = (0, 0);
        for span in spans {
            let next_byte = byte + span.text.len();
            let units = span.text.encode_utf16().count() as u32;
            metas.push(Meta {
                range: byte..next_byte,
                utf16: unit..unit + units,
            });
            byte = next_byte;
            unit += units;
        }
        RichLayout {
            text: text.into_boxed_str(),
            spans: metas.into_boxed_slice(),
            sys,
        }
    }

    pub(crate) fn sys(&self) -> &RichTextLayout {
        &self.sys
    }

    /// The concatenated text of every span.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// How many spans the layout has.
    pub fn span_count(&self) -> usize {
        self.spans.len()
    }

    /// The text of span `span`, or `""` when out of range.
    pub fn span_text(&self, span: usize) -> &str {
        self.spans
            .get(span)
            .map_or("", |meta| &self.text[meta.range.clone()])
    }

    /// The (width, height) the text occupies.
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

    /// The span and byte offset nearest the point `(x, y)`. Every span,
    /// clickable or not, is reported; the caller decides what a hit means.
    pub fn hit_test_point(&self, x: f32, y: f32) -> RichHit {
        let Ok(hit) = self.sys.hit_test_point(x, y) else {
            return RichHit {
                span: 0,
                offset: 0,
                inside: false,
            };
        };
        let start = advance(&self.text, 0, hit.position as usize);
        let byte = if hit.trailing {
            advance(&self.text, start, hit.length as usize)
        } else {
            start
        };
        let span = self.span_at(byte);
        RichHit {
            span,
            offset: byte.saturating_sub(self.spans[span].range.start),
            inside: hit.inside,
        }
    }

    /// The boxes covering span `span`, one per line (more where the text
    /// changes direction), for a hover underline or highlight.
    pub fn rects_of_span(&self, span: usize) -> Vec<RectF> {
        let Some(meta) = self.spans.get(span) else {
            return Vec::new();
        };
        if meta.utf16.is_empty() {
            return Vec::new();
        }
        self.sys
            .range_boxes(meta.utf16.start, meta.utf16.end - meta.utf16.start)
            .unwrap_or_default()
    }

    /// The span whose byte range contains `byte`, or the last one past the end.
    fn span_at(&self, byte: usize) -> usize {
        if self.spans.is_empty() {
            return 0;
        }
        self.spans
            .iter()
            .position(|meta| byte < meta.range.end)
            .unwrap_or(self.spans.len() - 1)
    }
}
