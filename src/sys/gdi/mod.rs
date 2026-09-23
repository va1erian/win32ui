//! GDI primitives: fonts, brushes, pens, DIB sections, off-screen buffers and
//! text.
//!
//! Split by responsibility: [`objects`] wraps object creation/selection and
//! text measurement, [`paint`] the paint session, clipping and the per-window
//! back-buffer cache.

mod objects;
mod paint;

pub(crate) use objects::*;
pub(crate) use paint::*;
