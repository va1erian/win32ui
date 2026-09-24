#![forbid(unsafe_code)]

//! OpenGL rendering for custom widgets.
//!
//! A [`CustomWidget`](crate::CustomWidget) that returns
//! [`Renderer::Gl`](crate::Renderer::Gl) draws through a [`GlSurface`] bound to
//! its window: on each frame the surface makes a WGL context current, sizes
//! the viewport and clears the framebuffer, the widget issues its own GL calls
//! through [`CustomWidget::paint_gl`](crate::CustomWidget::paint_gl) (given the
//! [`glow`] context), and the surface swaps the buffers. WGL's `unsafe` is
//! isolated in `src/sys/gl/`.

mod surface;

pub use surface::GlSurface;
