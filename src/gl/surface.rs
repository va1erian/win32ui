#![forbid(unsafe_code)]

//! [`GlSurface`]: an OpenGL context bound to a window.
//!
//! WGL's `unsafe` lives in [`crate::sys::gl`]; this is the safe view a
//! [`CustomWidget`](crate::CustomWidget) draws through. A `Renderer::Gl`
//! widget receives the [`glow::Context`] of its surface in
//! [`paint_gl`](crate::CustomWidget::paint_gl), already made current with the
//! viewport set and the framebuffer cleared to the theme background; the
//! surface presents the frame when the widget returns.

use std::cell::Cell;

use crate::color::Color;
use crate::error::Result;
use crate::hwnd::Hwnd;
use crate::sys;
use crate::sys::gl::Context;

/// An OpenGL context for one window.
///
/// Created lazily by the custom-widget paint path (like
/// [`D2dSurface`](crate::d2d::D2dSurface)) and resized from `WM_SIZE`. The
/// context is made current on the UI thread when a frame begins.
pub struct GlSurface {
    context: Context,
    pixels: Cell<(u32, u32)>,
    dpi: Cell<u32>,
    vsync: Cell<bool>,
}

impl GlSurface {
    /// Creates a context on `hwnd`'s client area. Fails when WGL cannot create
    /// a context (no driver, a remote session), so the caller can fall back to
    /// GDI.
    pub fn new(hwnd: Hwnd) -> Result<GlSurface> {
        let context = Context::new(hwnd)?;
        let client = sys::window::client_rect(hwnd);
        let pixels = (client.width().max(1) as u32, client.height().max(1) as u32);
        let dpi = sys::dpi::window_dpi(hwnd);
        let vsync = true;
        context.set_vsync(vsync);
        Ok(GlSurface {
            context,
            pixels: Cell::new(pixels),
            dpi: Cell::new(dpi),
            vsync: Cell::new(vsync),
        })
    }

    /// The window this surface draws to.
    pub fn hwnd(&self) -> Hwnd {
        self.context.hwnd()
    }

    /// Resizes the framebuffer to the client size in device pixels (call on
    /// `WM_SIZE`). The viewport is applied when the next frame begins.
    pub fn resize(&self, width: i32, height: i32) {
        self.pixels.set((width.max(1) as u32, height.max(1) as u32));
    }

    /// Applies a new DPI (call on `WM_DPICHANGED`). OpenGL draws in physical
    /// pixels, so this only feeds [`GlSurface::dpi`]; `begin_frame` re-reads
    /// the window's DPI each frame, which child windows are never told about.
    pub fn set_dpi(&self, dpi: u32) {
        self.dpi.set(dpi);
    }

    /// The window's dots-per-inch.
    pub fn dpi(&self) -> u32 {
        self.dpi.get()
    }

    /// Enables (`true`) or disables (`false`) vertical sync. Best-effort: a
    /// driver without `WGL_EXT_swap_control` ignores it.
    pub fn set_vsync(&self, on: bool) {
        self.context.set_vsync(on);
        self.vsync.set(on);
    }

    /// Whether vertical sync is requested.
    pub fn vsync(&self) -> bool {
        self.vsync.get()
    }

    /// Starts a frame: makes the context current, applies the viewport, clears
    /// to `background` and returns the `glow` context to draw with. The
    /// depth buffer is cleared too, so 3D widgets can depth-test.
    pub fn begin_frame(&self, background: Color) -> &glow::Context {
        let dpi = sys::dpi::window_dpi(self.context.hwnd());
        if dpi != self.dpi.get() {
            self.set_dpi(dpi);
        }
        self.context.make_current();
        let (width, height) = self.pixels.get();
        self.context.set_viewport(width as i32, height as i32);
        self.context.clear_to(background);
        self.context.glow()
    }

    /// Presents the frame and validates the window's whole client area.
    ///
    /// A frame covers every pixel (the framework clears it), so the update
    /// region is fully painted and must be validated, exactly as the Direct2D
    /// path does; otherwise Windows keeps sending `WM_PAINT`.
    pub fn end_frame(&self) -> Result<()> {
        self.context.swap()?;
        sys::window::validate(self.context.hwnd());
        Ok(())
    }
}
