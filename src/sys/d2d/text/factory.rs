//! The shared DirectWrite factory, family resolution and text formats.

use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_FAMILY_MODEL_TYPOGRAPHIC, DWRITE_FONT_METRICS,
    DWRITE_FONT_STRETCH, DWRITE_FONT_STYLE, DWRITE_FONT_STYLE_ITALIC, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT, DWRITE_UNICODE_RANGE, DWriteCreateFactory, IDWriteFactory2,
    IDWriteFactory6, IDWriteFontCollection, IDWriteFontFallback, IDWriteTextFormat,
    IDWriteTextFormat1,
};
use windows::core::{BOOL, HSTRING, Interface, PCWSTR, w};

use crate::d2d::{FontMetrics, FontStretch};
use crate::error::{Error, Result};
use crate::sys::win32_error;

use super::alias::{self, GdiAlias};
use super::layout::TextLayout;

/// The widest a line may grow when measuring without wrapping.
const UNBOUNDED: f32 = 1.0e6;

/// One font request: an ordered family list plus the style to match.
pub(crate) struct FontRequest<'a> {
    pub(crate) families: &'a [String],
    pub(crate) size: f32,
    pub(crate) weight: u16,
    pub(crate) italic: bool,
    pub(crate) stretch: FontStretch,
}

/// A text format together with what was resolved for it.
pub(crate) struct ResolvedFont {
    pub(crate) format: IDWriteTextFormat,
    /// The first family of the request that is installed.
    pub(crate) family: String,
    pub(crate) metrics: FontMetrics,
}

/// A family found in one of the two system collections.
struct Match {
    /// The name as requested, reported back as the resolved family.
    name: String,
    /// The family name to build a text format with; differs from `name` for a
    /// legacy GDI name.
    family: String,
    collection: IDWriteFontCollection,
    index: u32,
    /// The face a legacy GDI name stands for ("Segoe UI Semibold" is the
    /// semibold face of "Segoe UI").
    face: Option<GdiAlias>,
}

/// The process-wide DirectWrite factory and the two system font collections.
///
/// The typographic collection groups a family's weights ("Segoe UI" has a
/// semibold face), which is what CSS weights need. The GDI-compatible one
/// additionally knows legacy names such as "Segoe UI Semibold" and
/// "Arial Black".
#[derive(Clone)]
pub(crate) struct TextFactory {
    factory: IDWriteFactory2,
    typographic: Option<IDWriteFontCollection>,
    legacy: IDWriteFontCollection,
}

impl TextFactory {
    pub(crate) fn new() -> Result<TextFactory> {
        // SAFETY: creating the shared factory has no preconditions; DirectWrite
        // documents it as thread-safe.
        let factory: IDWriteFactory2 =
            unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }.map_err(win32_error)?;
        let mut legacy = None;
        // SAFETY: the out pointer is a valid local.
        unsafe { factory.GetSystemFontCollection(&mut legacy, false) }.map_err(win32_error)?;
        let legacy = legacy.ok_or(Error::Direct2d("no system font collection"))?;
        Ok(TextFactory {
            typographic: typographic_collection(&factory),
            factory,
            legacy,
        })
    }

    /// Finds `name` as a family, else as a legacy GDI name.
    fn find(&self, name: &str) -> Option<Match> {
        self.find_family(name).or_else(|| {
            let face = alias::resolve(&self.factory, name)?;
            let found = self.find_family(&face.family)?;
            Some(Match {
                name: name.to_owned(),
                face: Some(face),
                ..found
            })
        })
    }

    /// Finds `name` in the typographic collection first, then the legacy one.
    fn find_family(&self, name: &str) -> Option<Match> {
        let wide = HSTRING::from(name);
        self.typographic
            .iter()
            .chain(std::iter::once(&self.legacy))
            .find_map(|collection| {
                let mut index = 0;
                let mut exists = BOOL(0);
                // SAFETY: the out pointers are valid locals and `wide` is
                // NUL-terminated.
                unsafe { collection.FindFamilyName(&wide, &mut index, &mut exists) }.ok()?;
                exists.as_bool().then(|| Match {
                    name: name.to_owned(),
                    family: name.to_owned(),
                    collection: collection.clone(),
                    index,
                    face: None,
                })
            })
    }

    /// Resolves the request's families in order, builds the text format for
    /// the first installed one and chains the rest as glyph fallbacks ahead of
    /// the system fallback (CSS semantics: a later family supplies a glyph the
    /// earlier ones lack).
    pub(crate) fn resolve(&self, request: &FontRequest) -> Result<ResolvedFont> {
        let mut found: Vec<Match> = request
            .families
            .iter()
            .filter_map(|name| self.find(name))
            .collect();
        if found.is_empty() {
            found.extend(self.find("Segoe UI"));
        }
        let Some(primary) = found.first() else {
            return Err(Error::Direct2d("no usable font family"));
        };
        let (weight, style, stretch) = face_for(primary, request);
        let metrics = font_metrics(primary, weight, stretch, style, request.size)?;
        // SAFETY: valid arguments; the collection outlives the call.
        let format = unsafe {
            self.factory.CreateTextFormat(
                &HSTRING::from(primary.family.as_str()),
                &primary.collection,
                weight,
                style,
                stretch,
                request.size,
                w!("en-us"),
            )
        }
        .map_err(win32_error)?;
        if found.len() > 1 {
            // Without the chain the format still falls back through the
            // system, so failing to build one only loses the CSS ordering.
            let _ = self.chain_fallback(&format, &found[1..]);
        }
        Ok(ResolvedFont {
            format,
            family: found[0].name.clone(),
            metrics,
        })
    }

    fn chain_fallback(
        &self,
        format: &IDWriteTextFormat,
        rest: &[Match],
    ) -> windows::core::Result<()> {
        let everything = [DWRITE_UNICODE_RANGE {
            first: 0,
            last: 0x10_FFFF,
        }];
        // SAFETY: every pointer passed is to a live local for the duration of
        // its call; the builder copies what it needs.
        unsafe {
            let builder = self.factory.CreateFontFallbackBuilder()?;
            for family in rest {
                let name = HSTRING::from(family.family.as_str());
                builder.AddMapping(
                    &everything,
                    &[name.as_ptr()],
                    &family.collection,
                    PCWSTR::null(),
                    PCWSTR::null(),
                    1.0,
                )?;
            }
            let system: IDWriteFontFallback = self.factory.GetSystemFontFallback()?;
            builder.AddMappings(&system)?;
            format
                .cast::<IDWriteTextFormat1>()?
                .SetFontFallback(&builder.CreateFontFallback()?)
        }
    }

    /// Lays `text` out with `format`, wrapping at `max_width`.
    pub(crate) fn layout(
        &self,
        format: &IDWriteTextFormat,
        text: &str,
        max_width: f32,
    ) -> Result<TextLayout> {
        let wide: Vec<u16> = text.encode_utf16().collect();
        // SAFETY: `wide` and `format` are valid for the call; DirectWrite copies
        // the string.
        let layout = unsafe {
            self.factory
                .CreateTextLayout(&wide, format, max_width.min(UNBOUNDED), UNBOUNDED)
        }
        .map_err(win32_error)?;
        Ok(TextLayout::new(layout))
    }

    /// The unwrapped width of `text`, trailing spaces included.
    pub(crate) fn measure(&self, format: &IDWriteTextFormat, text: &str) -> Result<f32> {
        Ok(self.layout(format, text, UNBOUNDED)?.width_with_trailing())
    }
}

/// The weight, style and stretch to ask `found` for. A legacy GDI name fixes
/// the face for a regular request and lifts a bolder one to at least that
/// face, as GDI does.
fn face_for(
    found: &Match,
    request: &FontRequest,
) -> (DWRITE_FONT_WEIGHT, DWRITE_FONT_STYLE, DWRITE_FONT_STRETCH) {
    let weight = i32::from(request.weight);
    let requested_style = if request.italic {
        DWRITE_FONT_STYLE_ITALIC
    } else {
        DWRITE_FONT_STYLE_NORMAL
    };
    let requested_stretch = DWRITE_FONT_STRETCH(request.stretch as i32);
    let Some(face) = &found.face else {
        return (
            DWRITE_FONT_WEIGHT(weight),
            requested_style,
            requested_stretch,
        );
    };
    let weight = if weight <= 400 {
        face.weight.0
    } else {
        weight.max(face.weight.0)
    };
    (
        DWRITE_FONT_WEIGHT(weight),
        if request.italic {
            requested_style
        } else {
            face.style
        },
        if request.stretch == FontStretch::Normal {
            face.stretch
        } else {
            requested_stretch
        },
    )
}

/// The typographic system collection, when the OS has one (Windows 10 1803+).
fn typographic_collection(factory: &IDWriteFactory2) -> Option<IDWriteFontCollection> {
    let newer = factory.cast::<IDWriteFactory6>().ok()?;
    // SAFETY: plain value arguments on a live factory.
    let collection =
        unsafe { newer.GetSystemFontCollection(false, DWRITE_FONT_FAMILY_MODEL_TYPOGRAPHIC) }
            .ok()?;
    collection.cast().ok()
}

fn font_metrics(
    found: &Match,
    weight: DWRITE_FONT_WEIGHT,
    stretch: DWRITE_FONT_STRETCH,
    style: DWRITE_FONT_STYLE,
    size: f32,
) -> Result<FontMetrics> {
    let mut raw = DWRITE_FONT_METRICS::default();
    // SAFETY: the family index came from this collection and the out pointer is
    // a valid local.
    unsafe {
        found
            .collection
            .GetFontFamily(found.index)
            .and_then(|family| family.GetFirstMatchingFont(weight, stretch, style))
            .map(|font| font.GetMetrics(&mut raw))
    }
    .map_err(win32_error)?;
    let scale = size / f32::from(raw.designUnitsPerEm.max(1));
    Ok(FontMetrics {
        ascent: f32::from(raw.ascent) * scale,
        descent: f32::from(raw.descent) * scale,
        line_gap: f32::from(raw.lineGap) * scale,
        x_height: f32::from(raw.xHeight) * scale,
        cap_height: f32::from(raw.capHeight) * scale,
    })
}
