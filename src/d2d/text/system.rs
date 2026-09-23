//! [`TextSystem`]: the entry point that turns a [`FontSpec`] into a [`Font`].

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use crate::error::{Error, Result};
use crate::sys::d2d::text::{FontRequest, TextFactory};

use super::family;
use super::font::Font;

/// How narrow or wide a face is (CSS `font-stretch`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FontStretch {
    /// 50%.
    UltraCondensed = 1,
    /// 62.5%.
    ExtraCondensed = 2,
    /// 75%.
    Condensed = 3,
    /// 87.5%.
    SemiCondensed = 4,
    /// 100%.
    #[default]
    Normal = 5,
    /// 112.5%.
    SemiExpanded = 6,
    /// 125%.
    Expanded = 7,
    /// 150%.
    ExtraExpanded = 8,
    /// 200%.
    UltraExpanded = 9,
}

/// A font request, in CSS terms.
#[derive(Clone, Debug, PartialEq)]
pub struct FontSpec {
    /// A comma-separated family list such as `"Segoe UI, Arial, sans-serif"`,
    /// resolved entry by entry against the installed fonts. Legacy GDI names
    /// (`Segoe UI Semibold`, `Arial Black`) and the generic families `serif`,
    /// `sans-serif` and `monospace` (Cambria, Segoe UI, Consolas) work. A
    /// later entry also supplies glyphs an earlier one lacks; anything still
    /// missing (CJK, symbols, emoji) falls back through the system.
    pub family: String,
    /// The em size in device-independent pixels.
    pub size_dip: f32,
    /// Weight from 100 (thin) to 900 (black); 400 is regular, 700 bold.
    /// Clamped to that range.
    pub weight: u16,
    /// Italic: the family's italic face when it has one, else a synthesised
    /// slant.
    pub italic: bool,
    /// Face width.
    pub stretch: FontStretch,
}

impl FontSpec {
    /// A regular-weight, upright, normal-width font of `size_dip`.
    pub fn new(family: impl Into<String>, size_dip: f32) -> FontSpec {
        FontSpec {
            family: family.into(),
            size_dip,
            weight: 400,
            italic: false,
            stretch: FontStretch::Normal,
        }
    }

    /// Sets the weight (100-900).
    pub fn weight(mut self, weight: u16) -> FontSpec {
        self.weight = weight;
        self
    }

    /// Sets italic.
    pub fn italic(mut self, italic: bool) -> FontSpec {
        self.italic = italic;
        self
    }

    /// Sets the face width.
    pub fn stretch(mut self, stretch: FontStretch) -> FontSpec {
        self.stretch = stretch;
        self
    }

    fn key(&self) -> FontKey {
        FontKey {
            family: self.family.clone(),
            size_bits: self.size_dip.to_bits(),
            weight: self.weight.clamp(100, 900),
            italic: self.italic,
            stretch: self.stretch,
        }
    }
}

/// [`FontSpec`] with the size as bits so it can be hashed.
#[derive(PartialEq, Eq, Hash)]
struct FontKey {
    family: String,
    size_bits: u32,
    weight: u16,
    italic: bool,
    stretch: FontStretch,
}

struct Shared {
    factory: TextFactory,
    fonts: Mutex<HashMap<FontKey, Font>>,
}

/// Resolves fonts and owns the DirectWrite factory and the per-spec font
/// cache. Cloning is cheap and shares both. `Send + Sync`: use it from any
/// thread.
#[derive(Clone)]
pub struct TextSystem {
    shared: Arc<Shared>,
}

impl TextSystem {
    /// Creates the text system. Fails when DirectWrite is unavailable.
    pub fn new() -> Result<TextSystem> {
        Ok(TextSystem {
            shared: Arc::new(Shared {
                factory: TextFactory::new()?,
                fonts: Mutex::new(HashMap::new()),
            }),
        })
    }

    /// The font for `spec`, built once per distinct spec and then shared:
    /// repeated calls return clones of the same font, with its width cache.
    pub fn font(&self, spec: &FontSpec) -> Result<Font> {
        if !(spec.size_dip.is_finite() && spec.size_dip > 0.0) {
            return Err(Error::Direct2d("font size must be positive"));
        }
        let key = spec.key();
        let mut fonts = self
            .shared
            .fonts
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(font) = fonts.get(&key) {
            return Ok(font.clone());
        }
        let families = family::candidates(&spec.family);
        let resolved = self.shared.factory.resolve(&FontRequest {
            families: &families,
            size: spec.size_dip,
            weight: key.weight,
            italic: spec.italic,
            stretch: spec.stretch,
        })?;
        let font = Font::new(self.shared.factory.clone(), spec.clone(), resolved);
        fonts.insert(key, font.clone());
        Ok(font)
    }
}
