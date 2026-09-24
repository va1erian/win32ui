#![forbid(unsafe_code)]

//! A font handle. Dropping it releases the underlying `HFONT`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use windows::Win32::Graphics::Gdi::HFONT;

use crate::error::Result;
use crate::sys;

/// Standard font weights (the `FW_*` scale).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontWeight {
    /// `FW_NORMAL` (400).
    Regular,
    /// `FW_MEDIUM` (500).
    Medium,
    /// `FW_SEMIBOLD` (600).
    Semibold,
    /// `FW_BOLD` (700).
    Bold,
}

impl FontWeight {
    /// The numeric weight Win32 expects.
    pub const fn value(self) -> i32 {
        match self {
            FontWeight::Regular => 400,
            FontWeight::Medium => 500,
            FontWeight::Semibold => 600,
            FontWeight::Bold => 700,
        }
    }
}

thread_local! {
    /// The per-DPI fonts behind [`Font::shared_ui`].
    static SHARED: RefCell<HashMap<u32, Rc<Font>>> = RefCell::new(HashMap::new());
}

/// A GDI font.
pub struct Font {
    handle: HFONT,
    ascent: i32,
}

impl Font {
    /// Creates a font for `family` at `point_size` points on a `dpi` display.
    pub fn new(family: &str, point_size: f32, weight: FontWeight, dpi: u32) -> Result<Font> {
        // A negative height asks for a character height; the 96/72 factor turns
        // points into logical units at the given DPI.
        let height = -((point_size * dpi as f32 / 72.0).round() as i32).max(1);
        let handle = sys::gdi::create_font(family, height, weight.value())?;
        Ok(Font {
            handle,
            ascent: height.abs(),
        })
    }

    /// The UI font derived from system metrics at the given DPI.
    /// Uses `SystemParametersInfoForDpi(SPI_GETNONCLIENTMETRICS)` to get the
    /// system's configured message font (typically Segoe UI 9pt), falling back
    /// to Segoe UI 9pt if the call fails.
    pub fn system_ui(dpi: u32) -> Result<Font> {
        Font::system_ui_weight(dpi, FontWeight::Regular)
    }

    /// The UI font of [`Font::system_ui`] at another weight.
    pub fn system_ui_weight(dpi: u32, weight: FontWeight) -> Result<Font> {
        let (face, size) = sys::gdi::system_ui_font_metrics(dpi);
        Font::new(&face, size, weight, dpi)
    }

    /// The UI font for `dpi`, created once per thread and shared by every
    /// control. It is never released while the thread runs, so a control (or a
    /// pending repaint) can never hold a deleted `HFONT`; a DPI change simply
    /// switches controls to the entry for the new DPI.
    pub(crate) fn shared_ui(dpi: u32) -> Result<Rc<Font>> {
        SHARED.with(|cache| {
            if let Some(font) = cache.borrow().get(&dpi) {
                return Ok(Rc::clone(font));
            }
            let font = Rc::new(Font::system_ui(dpi)?);
            cache.borrow_mut().insert(dpi, Rc::clone(&font));
            Ok(font)
        })
    }

    /// Whether `handle` is one of the shared UI fonts (as opposed to a font a
    /// caller set on a control on purpose, which must not be replaced).
    pub(crate) fn is_shared_ui(handle: HFONT) -> bool {
        SHARED.with(|cache| cache.borrow().values().any(|font| font.handle == handle))
    }

    /// The nominal pixel height of the font.
    pub fn pixel_height(&self) -> i32 {
        self.ascent
    }

    pub(crate) fn raw(&self) -> HFONT {
        self.handle
    }
}

impl Drop for Font {
    fn drop(&mut self) {
        sys::gdi::delete_object(windows::Win32::Graphics::Gdi::HGDIOBJ(self.handle.0));
    }
}

/// The face the system draws its own UI with (typically Segoe UI, or Segoe UI
/// Variable), for DirectWrite text that should match the GDI controls.
pub(crate) fn system_ui_family() -> String {
    sys::gdi::system_ui_font_metrics(96).0
}
