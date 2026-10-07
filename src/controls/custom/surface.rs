#![forbid(unsafe_code)]

//! The renderer surface behind a [`Custom`]: releasing it, reaching the OpenGL
//! context outside a paint, and rendering the Direct2D paint offscreen.

use std::cell::RefCell;

use crate::capture::RgbaImage;
use crate::controls::custom_inner::CustomShared;
use crate::d2d::{D2dSurface, pixels_to_dips};
use crate::error::{Error, Result};
use crate::hwnd::Hwnd;
use crate::sys;

use super::{Custom, CustomWidget, Renderer, RendererState, draw_d2d};

impl<W: CustomWidget, M: 'static> Custom<W, M> {
    /// Drops the widget's renderer surface — the Direct2D target and its
    /// uploaded-image caches (or the OpenGL context) — so a heavy view does not
    /// hold them while it is hidden. The next paint recreates it, so a caller
    /// that also cached image handles from the surface must drop them too.
    pub fn release_renderer(&self) {
        {
            let widget = self.shared.widget.borrow();
            self.renderer
                .borrow_mut()
                .teardown_gl(|gl| widget.gl_teardown(gl));
        }
        *self.renderer.borrow_mut() = RendererState::Untried;
    }

    /// Releases the widget's uploaded Direct2D images — the retained RGBA cache
    /// and the device bitmaps — while keeping the render target. Use it instead
    /// of [`release_renderer`](Custom::release_renderer) when hiding a heavy
    /// Direct2D view: the covers' memory is freed, but the surface is not
    /// dropped, so the next show does not recreate the target (a fresh target
    /// paints nothing until its first frame, so the window can flash stale
    /// pixels). A caller that cached image handles from the surface must drop
    /// them too. A no-op for the OpenGL and GDI renderers.
    pub fn release_images(&self) {
        self.renderer.borrow().release_images();
    }

    /// Runs `f` with the widget's OpenGL context made current, outside a paint,
    /// and returns its result. `None` when the widget has no live
    /// [`GlSurface`](crate::gl::GlSurface) — it uses another renderer, its first
    /// frame has not run yet, or the context could not be created.
    ///
    /// Unlike [`CustomWidget::paint_gl`](crate::CustomWidget::paint_gl) neither
    /// the viewport nor the framebuffer is touched, and nothing is presented:
    /// issue GL calls directly. Use it to free GPU resources without waiting
    /// for a paint (hiding a view, say), or to upload assets up front. A widget
    /// that only frees on teardown can implement
    /// [`CustomWidget::gl_teardown`](crate::CustomWidget::gl_teardown) instead.
    pub fn with_gl<R>(&self, f: impl FnOnce(&glow::Context) -> R) -> Option<R> {
        match &*self.renderer.borrow() {
            RendererState::Gl(surface) => Some(surface.with_gl(f)),
            _ => None,
        }
    }

    /// Renders the widget's [`paint_d2d`](CustomWidget::paint_d2d) into an
    /// image, offscreen, at its current client size, DPI, theme and scroll
    /// offset.
    ///
    /// The frame is drawn with Direct2D's software rasterizer into a memory
    /// buffer, independent of the window, the GPU and the desktop: it works
    /// when the widget is hidden or occluded, and where
    /// [`Ui::capture`](crate::Ui::capture)'s `PrintWindow` returns blank
    /// Direct2D panes (a session whose desktop cannot be read). Images the
    /// widget uploaded to its window surface keep their
    /// [`ImageId`](crate::d2d::ImageId)s, and images uploaded during the
    /// offscreen frame are carried back to the window surface once it exists
    /// (a widget that has never painted on screen should not keep the ids it
    /// uploads here). The pixels are opaque RGBA.
    ///
    /// Fails when the widget's [`renderer`](CustomWidget::renderer) is not
    /// [`Renderer::Direct2D`], when it has no area, when the widget is mutably
    /// borrowed, or when Direct2D is unavailable.
    pub fn render_image(&self) -> Result<RgbaImage> {
        render(&self.shared, &self.renderer, self.control.hwnd())
    }
}

/// Renders the widget behind `shared` offscreen; see [`Custom::render_image`].
/// `renderer` is the window's renderer, whose uploaded images are shared with
/// the offscreen surface when it is not mid-frame.
pub(super) fn render<W: CustomWidget, M: 'static>(
    shared: &CustomShared<W, M>,
    renderer: &RefCell<RendererState>,
    hwnd: Hwnd,
) -> Result<RgbaImage> {
    let widget = shared
        .widget
        .try_borrow()
        .map_err(|_| Error::Direct2d("the widget is mutably borrowed"))?;
    if widget.renderer() != Renderer::Direct2D {
        return Err(Error::Direct2d("the widget does not paint with Direct2D"));
    }
    let client = sys::window::client_rect(hwnd);
    if client.is_empty() {
        return Err(Error::Direct2d("the widget has no area to render"));
    }
    let dpi = sys::dpi::window_dpi(hwnd);
    let offset = shared.scroll.borrow().as_ref().map_or(0, |s| s.offset());
    let theme = shared.ui.theme();

    let live = renderer.try_borrow().ok();
    let images = match live.as_deref() {
        Some(RendererState::Direct2d(surface)) => Some(&**surface),
        _ => None,
    };
    let surface =
        D2dSurface::offscreen(client.width() as u32, client.height() as u32, dpi, images)?;
    drop(live);
    let mut canvas = surface.begin_draw()?;
    // The scroll offset converts at the app's DPI, exactly as the window paint does.
    let scroll_offset = pixels_to_dips(offset, shared.ui.dpi());
    draw_d2d(&mut canvas, &*widget, &theme, scroll_offset);
    canvas.end_draw()?;
    // An image the widget uploaded during this frame (and may keep the id of)
    // must exist on its window surface too.
    if let Ok(live) = renderer.try_borrow()
        && let RendererState::Direct2d(window) = &*live
    {
        window.adopt_images(&surface);
    }
    surface
        .read_pixels()
        .ok_or(Error::Direct2d("offscreen surface has no buffer"))
}
