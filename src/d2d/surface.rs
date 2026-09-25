#![forbid(unsafe_code)]

//! [`D2dSurface`]: a Direct2D render target bound to a window.

use std::cell::{Cell, RefCell};

use crate::error::{Error, Result};
use crate::geometry::Rect;
use crate::hwnd::Hwnd;
use crate::message::Message;
use crate::sys;
use crate::sys::d2d::Target;

use super::BASE_DPI;
use super::RectF;
use super::bitmap::ImageCache;
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
    transparent: bool,
    pub(super) drawing: Cell<bool>,
    pub(super) images: RefCell<ImageCache>,
    /// The device-pixel rectangle this frame is clipped to, or `None` for a
    /// whole-window frame. `D2dCanvas::paint_rect` reports it so a virtualized
    /// widget knows what to draw.
    frame: Cell<Option<Rect>>,
    /// The current canvas translation, in device-independent pixels, set by
    /// [`D2dCanvas::set_translation`](super::D2dCanvas::set_translation). A
    /// scrolling widget draws its document in its own coordinates, so
    /// `paint_rect` undoes the translation to report the dirty rectangle in
    /// those coordinates.
    translation: Cell<(f32, f32)>,
    /// Set when the render target was resized, which blanks it: the next frame
    /// must repaint the whole client, not just the newly exposed rectangle.
    needs_full_repaint: Cell<bool>,
}

impl D2dSurface {
    /// Creates a surface for `hwnd`, sized to its client area. Fails when
    /// Direct2D cannot create a render target (a broken driver, for example),
    /// so a caller can fall back to GDI.
    pub fn new(hwnd: Hwnd) -> Result<D2dSurface> {
        D2dSurface::build(hwnd, false)
    }

    /// Creates a surface whose pixels carry premultiplied alpha, sized to
    /// `hwnd`'s client area. Clearing it with
    /// [`D2dCanvas::clear_rgba`](crate::d2d::D2dCanvas::clear_rgba) and
    /// [`Rgba::TRANSPARENT`](crate::d2d::Rgba::TRANSPARENT) makes those pixels
    /// transparent, so an extended-frame window's DWM material shows through
    /// them. The rest of the surface stays opaque where it is filled.
    pub fn transparent(hwnd: Hwnd) -> Result<D2dSurface> {
        D2dSurface::build(hwnd, true)
    }

    fn build(hwnd: Hwnd, transparent: bool) -> Result<D2dSurface> {
        let client = sys::window::client_rect(hwnd);
        let pixels = (client.width().max(1) as u32, client.height().max(1) as u32);
        let dpi = sys::dpi::window_dpi(hwnd);
        let target = Target::new(hwnd, pixels.0, pixels.1, dpi as f32, transparent)?;
        Ok(D2dSurface {
            hwnd,
            target: RefCell::new(Some(target)),
            pixels: Cell::new(pixels),
            dpi: Cell::new(dpi),
            transparent,
            drawing: Cell::new(false),
            images: RefCell::new(ImageCache::new()),
            frame: Cell::new(None),
            translation: Cell::new((0.0, 0.0)),
            // A fresh surface is blank, so its first frame repaints everything.
            needs_full_repaint: Cell::new(true),
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
        if self.pixels.get() == pixels {
            return;
        }
        self.pixels.set(pixels);
        // `Resize` discards the surface's contents, so the next frame has to
        // paint every pixel again.
        self.needs_full_repaint.set(true);
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
        self.begin_frame(None)
    }

    /// Starts a frame clipped to `rect` (device pixels, the window's update
    /// region), so drawing only touches those pixels and, when the frame ends,
    /// only that rectangle is validated. Use it to repaint a dirty region
    /// without redrawing the rest of the surface.
    ///
    /// If the render target had to be created for this frame, the clip is
    /// widened to the whole client area: a fresh target starts blank, so every
    /// pixel must be painted.
    pub fn begin_draw_rect(&self, rect: Rect) -> Result<D2dCanvas<'_>> {
        self.begin_frame(Some(rect))
    }

    fn begin_frame(&self, clip: Option<Rect>) -> Result<D2dCanvas<'_>> {
        if self.drawing.replace(true) {
            return Err(Error::Direct2d("begin_draw while a frame is in progress"));
        }
        let created = self.target.borrow().is_none() || self.needs_full_repaint.replace(false);
        match self.prepare_target() {
            Ok(()) => {
                let mut canvas = D2dCanvas::begin(self);
                // The render target keeps its transform between frames, so a
                // frame starts at the origin; a scrolling widget re-applies its
                // offset afterwards. `clear` is then a plain (clip-aware) fill
                // rather than a transform-independent `Clear`.
                canvas.set_translation(0.0, 0.0);
                if let Some(rect) = clip {
                    // Clip to the visible client: a scrolled child taller than
                    // its viewport would otherwise paint (and a virtualized
                    // widget would request the data for) the whole document,
                    // both on the frame a fresh target is created and whenever
                    // the update rectangle spans it.
                    let visible = sys::window::visible_client_rect(self.hwnd);
                    let rect = if created {
                        visible
                    } else {
                        sys::window::intersect(rect, visible)
                    };
                    self.frame.set(Some(rect));
                    canvas.push_clip(self.to_dips(rect));
                } else {
                    self.frame.set(None);
                }
                Ok(canvas)
            }
            Err(error) => {
                self.drawing.set(false);
                Err(error)
            }
        }
    }

    /// The device-pixel rectangle the current frame is clipped to, if any.
    pub(super) fn frame(&self) -> Option<Rect> {
        self.frame.get()
    }

    /// The current canvas translation, in device-independent pixels.
    pub(super) fn translation(&self) -> (f32, f32) {
        self.translation.get()
    }

    /// Records the canvas translation `set_translation` applied, so
    /// `paint_rect` can map the dirty rectangle back to drawing coordinates.
    pub(super) fn set_translation(&self, x: f32, y: f32) {
        self.translation.set((x, y));
    }

    /// Converts a device-pixel rectangle in this surface's window to
    /// device-independent pixels (the canvas's drawing space).
    fn to_dips(&self, rect: Rect) -> RectF {
        let scale = self.scale();
        RectF::new(
            rect.left as f32 / scale,
            rect.top as f32 / scale,
            rect.right as f32 / scale,
            rect.bottom as f32 / scale,
        )
    }

    fn prepare_target(&self) -> Result<()> {
        let dpi = sys::dpi::window_dpi(self.hwnd);
        if dpi != self.dpi.get() {
            self.set_dpi(dpi);
        }
        let mut target = self.target.borrow_mut();
        if target.is_none() {
            let (width, height) = self.pixels.get();
            *target = Some(Target::new(
                self.hwnd,
                width,
                height,
                dpi as f32,
                self.transparent,
            )?);
        }
        Ok(())
    }

    /// Discards the render target after a device loss and asks for a repaint.
    pub(super) fn recreate_later(&self) {
        self.discard_target();
        sys::window::invalidate(self.hwnd);
    }

    /// Drops the render target so the next frame rebuilds it at the current
    /// size. Called on a resize: a target left at the old size would present
    /// nothing after a maximize/minimize/restore.
    pub(crate) fn discard_target(&self) {
        *self.target.borrow_mut() = None;
    }
}
