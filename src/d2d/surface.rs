#![forbid(unsafe_code)]

//! [`D2dSurface`]: a Direct2D render target bound to a window.

use std::cell::{Cell, RefCell};

use crate::error::{Error, Result};
use crate::hwnd::Hwnd;
use crate::message::Message;
use crate::sys;
use crate::sys::d2d::Target;

use super::BASE_DPI;
use super::canvas::D2dCanvas;

/// A Direct2D render target for one window.
///
/// The device resources are created when the surface is and re-created
/// transparently after a device loss (`D2DERR_RECREATE_TARGET`): the failed
/// frame is dropped, the window is invalidated, and the next
/// [`begin_draw`](D2dSurface::begin_draw) builds a fresh target. Call
/// [`resize`](D2dSurface::resize) from `WM_SIZE` and
/// [`set_dpi`](D2dSurface::set_dpi) from `WM_DPICHANGED`; `begin_draw` also
/// re-reads the window's DPI, which child windows are never told about.
pub struct D2dSurface {
    hwnd: Hwnd,
    pub(super) target: RefCell<Option<Target>>,
    pixels: Cell<(u32, u32)>,
    dpi: Cell<u32>,
    pub(super) drawing: Cell<bool>,
}

impl D2dSurface {
    /// Creates a surface for `hwnd`, sized to its client area. Fails when
    /// Direct2D cannot create a render target (a broken driver, for example),
    /// so a caller can fall back to GDI.
    pub fn new(hwnd: Hwnd) -> Result<D2dSurface> {
        let client = sys::window::client_rect(hwnd);
        let pixels = (client.width().max(1) as u32, client.height().max(1) as u32);
        let dpi = sys::dpi::window_dpi(hwnd);
        let target = Target::new(hwnd, pixels.0, pixels.1, dpi as f32)?;
        Ok(D2dSurface {
            hwnd,
            target: RefCell::new(Some(target)),
            pixels: Cell::new(pixels),
            dpi: Cell::new(dpi),
            drawing: Cell::new(false),
        })
    }

    /// The window this surface draws to.
    pub fn hwnd(&self) -> Hwnd {
        self.hwnd
    }

    /// Resizes the backing surface to the client size in device pixels
    /// (call on `WM_SIZE`).
    pub fn resize(&self, width: i32, height: i32) {
        let pixels = (width.max(1) as u32, height.max(1) as u32);
        self.pixels.set(pixels);
        let mut target = self.target.borrow_mut();
        if let Some(live) = target.as_mut()
            && live.resize(pixels.0, pixels.1).is_err()
        {
            *target = None;
        }
    }

    /// Applies a new DPI (call on `WM_DPICHANGED`).
    pub fn set_dpi(&self, dpi: u32) {
        self.dpi.set(dpi);
        if let Some(target) = self.target.borrow().as_ref() {
            target.set_dpi(dpi as f32);
        }
    }

    /// The scale from device-independent to device pixels (1.0 at 96 DPI).
    pub fn scale(&self) -> f32 {
        self.dpi.get() as f32 / BASE_DPI
    }

    /// Whether `message` is `WM_ERASEBKGND`. A Direct2D window should return
    /// `Some(1)` for it: the frame covers every pixel, and erasing first
    /// flickers.
    pub fn is_erase_background(message: &Message) -> bool {
        matches!(message, Message::Other { code, .. } if *code == sys::d2d::WM_ERASEBKGND)
    }

    /// Starts a frame, re-creating the render target if the device was lost
    /// since the last one. The whole window is validated when the frame ends,
    /// so call this only from a paint handler (or after invalidating).
    pub fn begin_draw(&self) -> Result<D2dCanvas<'_>> {
        if self.drawing.replace(true) {
            return Err(Error::Direct2d("begin_draw while a frame is in progress"));
        }
        match self.prepare_target() {
            Ok(()) => Ok(D2dCanvas::begin(self)),
            Err(error) => {
                self.drawing.set(false);
                Err(error)
            }
        }
    }

    fn prepare_target(&self) -> Result<()> {
        let dpi = sys::dpi::window_dpi(self.hwnd);
        if dpi != self.dpi.get() {
            self.set_dpi(dpi);
        }
        let mut target = self.target.borrow_mut();
        if target.is_none() {
            let (width, height) = self.pixels.get();
            *target = Some(Target::new(self.hwnd, width, height, dpi as f32)?);
        }
        Ok(())
    }

    /// Discards the render target after a device loss and asks for a repaint.
    pub(super) fn recreate_later(&self) {
        *self.target.borrow_mut() = None;
        sys::window::invalidate(self.hwnd);
    }
}
