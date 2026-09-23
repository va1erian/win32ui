#![forbid(unsafe_code)]

//! DirectWrite text: fonts, measurement, layout and drawing.
//!
//! A [`TextSystem`] resolves a [`FontSpec`] to a [`Font`]. The same font
//! measures ([`Font::width`]), lays out ([`Font::layout`]) and, through
//! [`D2dCanvas::draw_text`](super::D2dCanvas::draw_text), draws, so a
//! document layout that measures text sees exactly the widths that get
//! painted, glyph fallback for CJK, symbols and emoji included.
//!
//! # Threads
//!
//! [`TextSystem`], [`Font`] and [`Layout`] are `Send + Sync`: DirectWrite's
//! factory is free-threaded and its text formats and layouts are immutable,
//! so a worker thread may resolve fonts, measure and lay out text while the UI
//! thread draws. Only [`D2dCanvas`](super::D2dCanvas) and
//! [`D2dSurface`](super::D2dSurface) are tied to the UI thread.
//!
//! # Indices
//!
//! [`Layout`] positions (`index`, `start`, `end`) are UTF-8 byte offsets into
//! the laid-out `&str`, always on `char` boundaries.

mod cache;
mod draw;
mod family;
mod font;
mod index;
mod layout;
mod rich;
mod system;

pub use font::{Font, FontMetrics};
pub use layout::{HitTest, Layout, LineMetrics};
pub use rich::{RichHit, RichLayout, Span};
pub use system::{FontSpec, FontStretch, TextSystem};
