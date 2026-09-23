#![forbid(unsafe_code)]

//! Anti-aliased 2D drawing with Direct2D.
//!
//! GDI ([`crate::gdi`]) has no anti-aliasing or alpha. A [`D2dSurface`] binds
//! to any window; each frame, [`D2dSurface::begin_draw`] returns a
//! [`D2dCanvas`] whose shapes are anti-aliased, and [`D2dCanvas::end_draw`]
//! presents them. Coordinates are `f32` device-independent pixels (see
//! [`RectF`]); the surface applies the window's DPI, so the same code draws
//! correctly at 100% to 200% scaling.
//!
//! A window painted with Direct2D must not also be painted with GDI on the
//! same pixels: use a surface per owner-drawn child window.
//!
//! Thread affinity: a [`D2dSurface`] and its canvas belong to the UI thread
//! (they are neither `Send` nor `Sync`).

mod bitmap;
mod brush;
mod canvas;
mod dc;
mod geometry;
mod path;
mod surface;
mod text;

pub use bitmap::{ImageId, Interpolation};
pub use brush::{ExtendMode, GradientStop, LinearGradient, RadialGradient, Rgba};
pub use canvas::D2dCanvas;
pub use dc::DcCanvas;
pub use geometry::{
    BASE_DPI, Cap, DashStyle, LineJoin, PointF, Radius, RectF, RoundedRect, Stroke, clamp_radius,
    pixels_to_dips,
};
pub use path::{ArcSize, Path, PathBuilder, Sweep};
pub use surface::D2dSurface;
pub use text::{
    Font, FontMetrics, FontSpec, FontStretch, HitTest, Layout, LineMetrics, TextSystem,
};
