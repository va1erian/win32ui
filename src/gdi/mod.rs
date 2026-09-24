#![forbid(unsafe_code)]

//! GDI drawing helpers: RAII handles and a double-buffered paint context.

mod bitmap;
mod brush;
mod cache;
mod font;
mod paint;
mod pen;

pub use bitmap::Bitmap;
pub use brush::Brush;
pub(crate) use font::system_ui_family;
pub use font::{Font, FontWeight};
pub use paint::{Canvas, Paint, TextFormat};
pub use pen::Pen;

pub(crate) use cache::solid_brush as cache_brush;
