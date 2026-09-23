#![forbid(unsafe_code)]

//! The Direct2D paint path for custom widgets.
//!
//! A [`CustomWidget`](super::CustomWidget) that opts into
//! [`Renderer::Direct2D`](super::Renderer::Direct2D) is drawn through a
//! [`D2dSurface`] bound to its window. Direct2D is tried on the first paint; if
//! it cannot be created (a
//! broken driver, say) the widget falls back to GDI for good, painting the
//! theme background. The surface is resized on `WM_SIZE`, re-reads its DPI on
//! every frame (child windows are never sent `WM_DPICHANGED`) and re-creates
//! its target after a device loss.

use crate::d2d::D2dSurface;
use crate::hwnd::Hwnd;
use crate::sys;

/// Which renderer is currently drawing a custom widget.
pub(crate) enum RendererState {
    /// Not yet decided: the first paint chooses Direct2D or falls back.
    Untried,
    /// Painting with Direct2D.
    Direct2d(Box<D2dSurface>),
    /// Direct2D is unavailable; paint the theme background with GDI.
    Gdi,
}

impl RendererState {
    /// Paints one Direct2D frame, creating the surface on first use. `draw`
    /// receives the freshly begun canvas and must fill the viewport; a lost
    /// device is handled by [`D2dSurface`] and does not fall back.
    ///
    /// Returns `false` when the frame could not be begun (Direct2D cannot be
    /// created, or a frame is already in progress), so the caller paints the
    /// GDI fallback instead.
    pub(crate) fn paint(
        &mut self,
        hwnd: Hwnd,
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
        let Ok(mut canvas) = surface.begin_draw() else {
            return false;
        };
        draw(&mut canvas);
        if canvas.end_draw().is_err() {
            *self = RendererState::Gdi;
            sys::window::invalidate(hwnd);
        }
        true
    }

    /// Resizes the backing surface (call on `WM_SIZE`).
    pub(crate) fn resize(&self, width: i32, height: i32) {
        if let RendererState::Direct2d(surface) = self {
            surface.resize(width, height);
        }
    }

    /// Whether the next [`RendererState::paint`] will draw with Direct2D.
    pub(crate) fn is_direct2d(&self) -> bool {
        matches!(self, RendererState::Direct2d(_))
    }
}
