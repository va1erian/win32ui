#![forbid(unsafe_code)]

//! GDI drawing helpers: RAII handles and a double-buffered paint context.

mod bitmap;
mod brush;
mod font;
mod paint;
mod pen;

pub use bitmap::Bitmap;
pub use brush::Brush;
pub use font::{Font, FontWeight};
pub use paint::{Canvas, Paint, TextFormat};
pub use pen::Pen;
