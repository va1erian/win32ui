//! [`Font`]: a resolved font with cached measurement.

use std::sync::{Arc, Mutex, PoisonError};

use crate::error::Result;
use crate::sys::d2d::text::{ResolvedFont, RichStyle, TextFactory};

use super::cache::WidthCache;
use super::layout::Layout;
use super::rich::{RichLayout, Span};
use super::system::FontSpec;

/// How many distinct strings a font remembers the width of.
const WIDTH_CACHE_ENTRIES: usize = 16_384;

/// Strings longer than this (in bytes) are measured but not remembered, so a
/// few huge paragraphs cannot crowd out the short strings layout repeats.
const MAX_CACHED_LEN: usize = 256;

/// Vertical metrics of a font at its size, in device-independent pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontMetrics {
    /// Distance from the baseline up to the top of the line.
    pub ascent: f32,
    /// Distance from the baseline down to the bottom of the line.
    pub descent: f32,
    /// Extra space between lines.
    pub line_gap: f32,
    /// Height of a lower-case `x`.
    pub x_height: f32,
    /// Height of a capital letter.
    pub cap_height: f32,
}

impl FontMetrics {
    /// The distance between baselines: ascent + descent + line gap.
    pub fn line_height(&self) -> f32 {
        self.ascent + self.descent + self.line_gap
    }
}

struct FontInner {
    factory: TextFactory,
    spec: FontSpec,
    resolved: ResolvedFont,
    widths: Mutex<WidthCache>,
}

/// A font at one size and style, from [`TextSystem::font`](super::TextSystem::font).
///
/// Cloning is cheap and shares the width cache. `Send + Sync`: measure from
/// any thread.
#[derive(Clone)]
pub struct Font {
    inner: Arc<FontInner>,
}

impl Font {
    pub(super) fn new(factory: TextFactory, spec: FontSpec, resolved: ResolvedFont) -> Font {
        Font {
            inner: Arc::new(FontInner {
                factory,
                spec,
                resolved,
                widths: Mutex::new(WidthCache::new(WIDTH_CACHE_ENTRIES)),
            }),
        }
    }

    /// The request this font was built from.
    pub fn spec(&self) -> &FontSpec {
        &self.inner.spec
    }

    /// The first family of the request that is installed (after generic
    /// families are mapped), which is the one metrics come from.
    pub fn family(&self) -> &str {
        &self.inner.resolved.family
    }

    /// The font's vertical metrics.
    pub fn metrics(&self) -> FontMetrics {
        self.inner.resolved.metrics
    }

    /// The width of `text` on one line, trailing spaces included, in
    /// device-independent pixels. Uses the same glyph fallback as drawing.
    ///
    /// Results are remembered per string (bounded, least recently used), so
    /// repeated measurement is a hash lookup. Returns 0 for an empty string
    /// and, should DirectWrite fail, for that string.
    pub fn width(&self, text: &str) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        if let Some(width) = self.widths().get(text) {
            return width;
        }
        let Ok(width) = self
            .inner
            .factory
            .measure(&self.inner.resolved.format, text)
        else {
            return 0.0;
        };
        if text.len() <= MAX_CACHED_LEN {
            self.widths().insert(text, width);
        }
        width
    }

    /// How many strings [`width`](Font::width) currently remembers.
    pub fn cached_widths(&self) -> usize {
        self.widths().len()
    }

    /// Lays `text` out wrapped at `max_width` (use `f32::INFINITY` for a
    /// single unwrapped line). Draw it with
    /// [`D2dCanvas::draw_text`](crate::d2d::D2dCanvas::draw_text).
    pub fn layout(&self, text: &str, max_width: f32) -> Result<Layout> {
        let sys = self
            .inner
            .factory
            .layout(&self.inner.resolved.format, text, max_width)?;
        Ok(Layout::new(text, sys))
    }

    /// Lays `spans` out as one wrapped, flowing line at `max_width` (use
    /// `f32::INFINITY` for a single unwrapped line). Every run is styled in
    /// place, so word wrap, bidi and hit testing cross run boundaries. Draw it
    /// with [`D2dCanvas::draw_rich_text`](crate::d2d::D2dCanvas::draw_rich_text).
    pub fn rich_layout(&self, spans: &[Span], max_width: f32) -> Result<RichLayout> {
        let text: String = spans.iter().map(|span| span.text.as_str()).collect();
        let base = self.inner.spec.size_dip;
        let styles = spans
            .iter()
            .scan(0, |start, span| {
                let length = span.text.encode_utf16().count() as u32;
                let style = RichStyle {
                    start: *start,
                    length,
                    size: span
                        .size_dip
                        .filter(|size| size.is_finite() && *size > 0.0)
                        .unwrap_or(base),
                    weight: span.weight.clamp(100, 900),
                    italic: span.italic,
                    underline: span.underline,
                    color: span.color,
                };
                *start += length;
                Some(style)
            })
            .collect::<Vec<_>>();
        let sys = self.inner.factory.rich_layout(
            &self.inner.resolved.format,
            &text,
            max_width,
            &styles,
        )?;
        Ok(RichLayout::new(text, spans, sys))
    }

    fn widths(&self) -> std::sync::MutexGuard<'_, WidthCache> {
        self.inner
            .widths
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}
