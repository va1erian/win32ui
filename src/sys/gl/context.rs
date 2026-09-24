//! A WGL (OpenGL) rendering context bound to a child window.
//!
//! Creation follows the classic bring-up: choose and set a pixel format on the
//! window's device context, create a legacy context just to resolve
//! `wglCreateContextAttribsARB`, then create a modern core-profile context and
//! drop the legacy one. When the extension is missing or its context cannot be
//! created, the legacy context is kept, so a widget still gets OpenGL wherever
//! the driver offers only the 1.1 entry points. Any earlier failure is an
//! [`Error::Gl`], on which the caller falls back to GDI.

use core::ffi::c_void;
use std::mem::size_of;
use std::ptr;

use glow::HasContext;
use windows::Win32::Graphics::Gdi::{GetDC, HDC, ReleaseDC};
use windows::Win32::Graphics::OpenGL::{
    ChoosePixelFormat, HGLRC, PFD_DOUBLEBUFFER, PFD_DRAW_TO_WINDOW, PFD_MAIN_PLANE,
    PFD_SUPPORT_OPENGL, PFD_TYPE_RGBA, PIXELFORMATDESCRIPTOR, SetPixelFormat, SwapBuffers,
    wglCreateContext, wglDeleteContext, wglGetCurrentContext, wglMakeCurrent,
};

use crate::color::Color;
use crate::error::{Error, Result};
use crate::hwnd::Hwnd;
use crate::sys::{raw_hwnd, win32_error};

use super::loader;

// Attributes and profile bits from `wglext.h` (WGL_ARB_create_context), absent
// from the `windows` bindings.
const WGL_CONTEXT_MAJOR_VERSION_ARB: i32 = 0x2091;
const WGL_CONTEXT_MINOR_VERSION_ARB: i32 = 0x2092;
const WGL_CONTEXT_PROFILE_MASK_ARB: i32 = 0x9126;
const WGL_CONTEXT_CORE_PROFILE_BIT_ARB: i32 = 0x0001;

/// The context version a widget asks for; a driver that cannot provide it
/// leaves the legacy context in place instead.
const GL_MAJOR: i32 = 3;
const GL_MINOR: i32 = 3;

/// `wglCreateContextAttribsARB`, resolved at runtime.
type CreateContextAttribsFn = unsafe extern "system" fn(HDC, HGLRC, *const i32) -> HGLRC;
/// `wglSwapIntervalEXT`, resolved at runtime.
type SwapIntervalFn = unsafe extern "system" fn(i32) -> i32;

/// An OpenGL context and the window DC it draws to.
pub(crate) struct Context {
    hwnd: Hwnd,
    hdc: HDC,
    hglrc: HGLRC,
    glow: glow::Context,
    swap_interval: Option<SwapIntervalFn>,
}

impl Context {
    /// Creates a context on `hwnd`'s client DC. The context is made current on
    /// the calling thread and stays current while the surface draws.
    pub(crate) fn new(hwnd: Hwnd) -> Result<Context> {
        let raw = raw_hwnd(hwnd);
        // SAFETY: `raw` is a live child window; `GetDC` returns its client DC
        // or a null handle.
        let hdc = unsafe { GetDC(Some(raw)) };
        if hdc.0.is_null() {
            return Err(Error::Gl("GetDC returned no device context"));
        }
        match Context::build(hwnd, hdc) {
            Ok(context) => Ok(context),
            Err(error) => {
                // SAFETY: `hdc` came from `GetDC` on `raw` and was not released.
                unsafe { ReleaseDC(Some(raw), hdc) };
                Err(error)
            }
        }
    }

    fn build(hwnd: Hwnd, hdc: HDC) -> Result<Context> {
        set_pixel_format(hdc)?;

        // SAFETY: `hdc` has a pixel format set; the returned context is live.
        let legacy = unsafe { wglCreateContext(hdc) }.map_err(win32_error)?;
        // SAFETY: `hdc` and `legacy` are live and belong to this thread.
        unsafe { wglMakeCurrent(hdc, legacy) }.map_err(win32_error)?;

        let hglrc = match create_modern_context(hdc) {
            Some(modern) => {
                // SAFETY: both contexts are live on this thread's DC; making
                // `modern` current releases `legacy`, then it can be deleted.
                unsafe { wglMakeCurrent(hdc, modern) }.map_err(win32_error)?;
                let _ = unsafe { wglDeleteContext(legacy) };
                modern
            }
            None => legacy,
        };

        // SAFETY: a context is current and stays current on the UI thread, as
        // `glow` requires to query the version and load entry points.
        let glow = unsafe { glow::Context::from_loader_function(loader::load) };

        let swap_interval = load_swap_interval();
        if let Some(swap_interval) = swap_interval {
            // SAFETY: `wglSwapIntervalEXT` requires a current context.
            unsafe { swap_interval(1) };
        }

        Ok(Context {
            hwnd,
            hdc,
            hglrc,
            glow,
            swap_interval,
        })
    }

    /// The window this context draws to.
    pub(crate) fn hwnd(&self) -> Hwnd {
        self.hwnd
    }

    /// The `glow` wrapper over this context's entry points.
    pub(crate) fn glow(&self) -> &glow::Context {
        &self.glow
    }

    /// Sets the viewport to the whole framebuffer.
    pub(crate) fn set_viewport(&self, width: i32, height: i32) {
        // SAFETY: a context is current and `width`/`height` are non-negative.
        unsafe { self.glow.viewport(0, 0, width.max(0), height.max(0)) };
    }

    /// Clears the colour and depth buffers to `background`.
    pub(crate) fn clear_to(&self, background: Color) {
        // SAFETY: a context is current; the constants are valid clear bits.
        unsafe {
            self.glow.clear_color(
                f32::from(background.r) / 255.0,
                f32::from(background.g) / 255.0,
                f32::from(background.b) / 255.0,
                1.0,
            );
            self.glow
                .clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
        }
    }

    /// Makes this context current on the calling thread.
    pub(crate) fn make_current(&self) {
        // SAFETY: `hdc` and `hglrc` are live and owned by this context.
        let _ = unsafe { wglMakeCurrent(self.hdc, self.hglrc) };
    }

    /// Presents the back buffer.
    pub(crate) fn swap(&self) -> Result<()> {
        // SAFETY: `hdc` is this context's live device context.
        unsafe { SwapBuffers(self.hdc) }.map_err(win32_error)
    }

    /// Enables (`true`) or disables (`false`) vertical sync. Ignored when the
    /// driver has no `WGL_EXT_swap_control`.
    pub(crate) fn set_vsync(&self, on: bool) {
        let Some(swap_interval) = self.swap_interval else {
            return;
        };
        self.make_current();
        // SAFETY: a current context is required, and one is current here.
        let _ = unsafe { swap_interval(i32::from(on)) };
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        // SAFETY: `hglrc` is live and owned by this context. It is released
        // first when current, so the thread binding never points at freed
        // memory; `hdc` came from `GetDC` and is released exactly once.
        unsafe {
            if wglGetCurrentContext() == self.hglrc {
                let _ = wglMakeCurrent(HDC(ptr::null_mut()), HGLRC(ptr::null_mut()));
            }
            let _ = wglDeleteContext(self.hglrc);
            ReleaseDC(Some(raw_hwnd(self.hwnd)), self.hdc);
        }
    }
}

/// Chooses and sets a 32-bit double-buffered OpenGL pixel format on `hdc`.
/// A DC accepts exactly one pixel format, so this must run once per context.
fn set_pixel_format(hdc: HDC) -> Result<()> {
    let pfd = PIXELFORMATDESCRIPTOR {
        nSize: size_of::<PIXELFORMATDESCRIPTOR>() as u16,
        nVersion: 1,
        dwFlags: PFD_DRAW_TO_WINDOW | PFD_SUPPORT_OPENGL | PFD_DOUBLEBUFFER,
        iPixelType: PFD_TYPE_RGBA,
        cColorBits: 32,
        cAlphaBits: 8,
        cDepthBits: 24,
        cStencilBits: 8,
        iLayerType: PFD_MAIN_PLANE.0 as u8,
        ..Default::default()
    };
    // SAFETY: `pfd` is fully initialised for the call.
    let format = unsafe { ChoosePixelFormat(hdc, &pfd) };
    if format == 0 {
        return Err(Error::Gl(
            "the driver offers no matching OpenGL pixel format",
        ));
    }
    // SAFETY: `format` came from `ChoosePixelFormat` for this DC and `pfd`
    // describes it.
    unsafe { SetPixelFormat(hdc, format, &pfd) }.map_err(win32_error)
}

/// Creates a core-profile context, or `None` when the extension is unavailable
/// or refuses the request (the caller keeps the legacy context).
fn create_modern_context(hdc: HDC) -> Option<HGLRC> {
    let proc = loader::load("wglCreateContextAttribsARB");
    if proc.is_null() {
        return None;
    }
    // SAFETY: `proc` was resolved for this exact function name, whose signature
    // is `CreateContextAttribsFn`, while a context was current.
    let create: CreateContextAttribsFn = unsafe { std::mem::transmute(proc) };
    let attribs = [
        WGL_CONTEXT_MAJOR_VERSION_ARB,
        GL_MAJOR,
        WGL_CONTEXT_MINOR_VERSION_ARB,
        GL_MINOR,
        WGL_CONTEXT_PROFILE_MASK_ARB,
        WGL_CONTEXT_CORE_PROFILE_BIT_ARB,
        0,
    ];
    // SAFETY: `attribs` is a zero-terminated name/value list, `hdc` has a pixel
    // format set, and a null share context asks for none.
    let modern = unsafe { create(hdc, HGLRC(ptr::null_mut()), attribs.as_ptr()) };
    (!modern.0.is_null()).then_some(modern)
}

/// Resolves `wglSwapIntervalEXT`, if the driver exports it.
fn load_swap_interval() -> Option<SwapIntervalFn> {
    let proc = loader::load("wglSwapIntervalEXT");
    if proc.is_null() {
        return None;
    }
    // SAFETY: `proc` was resolved for this exact function name, whose
    // signature is `SwapIntervalFn`, while a context was current.
    Some(unsafe { std::mem::transmute::<*const c_void, SwapIntervalFn>(proc) })
}
