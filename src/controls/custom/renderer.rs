#![forbid(unsafe_code)]

//! The paint backends behind a [`CustomWidget`](super::CustomWidget):
//! [`RendererState`] lazily creates the surface the widget asked for
//! ([`Renderer::Direct2D`](super::Renderer::Direct2D) or
//! [`Renderer::Gl`](super::Renderer::Gl)) on the first frame, and falls back to
//! GDI for good when it cannot be created.
//!
//! Direct2D is bound to the window by a [`D2dSurface`] (resized on `WM_SIZE`,
//! re-reading its DPI every frame, recreated after device loss). OpenGL is
//! bound by a [`GlSurface`] (resized on `WM_SIZE`, its viewport applied and the
//! framebuffer cleared at the start of every frame).

use crate::color::Color;
use crate::d2d::D2dSurface;
use crate::geometry::Rect;
use crate::gl::GlSurface;
use crate::hwnd::Hwnd;
use crate::sys;

/// Which renderer is currently drawing a custom widget.
pub(crate) enum RendererState {
    /// Not yet decided: the first paint creates the requested surface or
    /// falls back.
    Untried,
    /// Painting with Direct2D.
    Direct2d(Box<D2dSurface>),
    /// Painting with OpenGL.
    Gl(Box<GlSurface>),
    /// The requested renderer is unavailable; paint the theme background with
    /// GDI.
    Gdi,
}

impl RendererState {
    /// Paints one Direct2D frame clipped to `dirty` (the window's update
    /// rectangle, in device pixels), creating the surface on first use. `draw`
    /// receives the freshly begun canvas and must fill the viewport; a lost
    /// device is handled by [`D2dSurface`] and does not fall back.
    ///
    /// Returns `false` when the frame could not be begun (Direct2D cannot be
    /// created, or a frame is already in progress), so the caller paints the
    /// GDI fallback instead.
    pub(crate) fn paint(
        &mut self,
        hwnd: Hwnd,
        dirty: Rect,
        draw: impl FnOnce(&mut crate::d2d::D2dCanvas),
    ) -> bool {
        if matches!(*self, RendererState::Untried) {
            *self = D2dSurface::new(hwnd).map_or(RendererState::Gdi, |surface| {
                RendererState::Direct2d(Box::new(surface))
            });
        }
        let RendererState::Direct2d(surface) = self else {
            return false;
        };
        let Ok(mut canvas) = surface.begin_draw_rect(dirty) else {
            return false;
        };
        draw(&mut canvas);
        if canvas.end_draw().is_err() {
            *self = RendererState::Gdi;
            sys::window::invalidate(hwnd);
        }
        true
    }

    /// Paints one OpenGL frame, creating the surface on first use. `draw`
    /// receives the current [`glow::Context`] with the viewport set to the
    /// window's client size and the framebuffer cleared to `background`; the
    /// frame is presented (`SwapBuffers`) once it returns.
    ///
    /// When the frame cannot be presented (OpenGL cannot be created, or the
    /// swap failed) `teardown` runs with the context still current, before the
    /// surface is dropped and the renderer falls back to GDI, so the widget can
    /// free its GPU resources.
    ///
    /// Returns `false` when the frame could not be presented, so the caller
    /// paints the GDI fallback instead.
    pub(crate) fn paint_gl(
        &mut self,
        hwnd: Hwnd,
        background: Color,
        draw: impl FnOnce(&glow::Context),
        teardown: impl FnOnce(&glow::Context),
    ) -> bool {
        if matches!(*self, RendererState::Untried) {
            *self = GlSurface::new(hwnd).map_or(RendererState::Gdi, |surface| {
                RendererState::Gl(Box::new(surface))
            });
        }
        let RendererState::Gl(surface) = self else {
            return false;
        };
        let gl = surface.begin_frame(background);
        draw(gl);
        if surface.end_frame().is_err() {
            surface.with_gl(teardown);
            *self = RendererState::Gdi;
            sys::window::invalidate(hwnd);
            return false;
        }
        true
    }

    /// Drops the OpenGL surface, if one exists, first running `teardown` with
    /// its context current. Use it when the widget is destroyed so GPU
    /// resources are freed while the context is still live. A no-op for the
    /// GDI and Direct2D renderers.
    pub(crate) fn teardown_gl(&mut self, teardown: impl FnOnce(&glow::Context)) {
        if let RendererState::Gl(surface) = self {
            surface.with_gl(teardown);
            *self = RendererState::Gdi;
        }
    }

    /// Resizes the backing surface (call on `WM_SIZE`).
    pub(crate) fn resize(&self, width: i32, height: i32) {
        match self {
            RendererState::Direct2d(surface) => surface.resize(width, height),
            RendererState::Gl(surface) => surface.resize(width, height),
            RendererState::Untried | RendererState::Gdi => {}
        }
    }

    /// Releases the Direct2D surface's uploaded images, keeping the render
    /// target. A no-op for the OpenGL and GDI renderers, which retain no such
    /// images.
    pub(crate) fn release_images(&self) {
        if let RendererState::Direct2d(surface) = self {
            surface.release_images();
        }
    }
}
