//! A Direct2D render target (window or device context) and the device-dependent
//! resources made from it (solid brushes). Stroke styles, RGBA/gradient
//! brushes, bitmaps and the clip/layer stack live in the sibling modules and
//! are dropped together with this struct when the device is lost, so a stale
//! resource can never be drawn with.
//!
//! Both target kinds share the same resource caches and drawing code: the
//! concrete interfaces only differ where the surface is bound (`Resize` for a
//! window, `BindDC` for a device context), so the base
//! `ID2D1RenderTarget` is what the caches and draw calls use.

use std::collections::HashMap;

use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct2D::Common::{D2D_RECT_F, D2D_SIZE_U, D2D1_COLOR_F};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ELLIPSE, D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_ROUNDED_RECT, ID2D1DCRenderTarget, ID2D1HwndRenderTarget, ID2D1RenderTarget,
    ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::Gdi::HDC;
use windows::core::Interface;
use windows_numerics::{Matrix3x2, Vector2};

use crate::color::Color;
use crate::d2d::{BASE_DPI, PointF, RectF, Stroke, clamp_radius};
use crate::error::{Error, Result};
use crate::geometry::Rect;
use crate::hwnd::Hwnd;
use crate::sys::{raw_hwnd, win32_error};

mod draw_text;

use super::{EndDraw, bitmap, brush, factory, geometry, is_target_lost};

/// The concrete render-target interface, kept for the two operations that are
/// not on the shared base: resizing a window target and binding a DC target.
enum Device {
    Hwnd(ID2D1HwndRenderTarget),
    Dc(ID2D1DCRenderTarget),
}

/// A render target with its resource caches.
pub(crate) struct Target {
    pub(crate) render: ID2D1RenderTarget,
    device: Device,
    brushes: HashMap<Color, ID2D1SolidColorBrush>,
    pub(crate) strokes: brush::Strokes,
    pub(crate) paints: brush::Paints,
    pub(crate) images: bitmap::Images,
    pub(crate) shapes: geometry::Shapes,
}

fn color_f(color: Color) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: f32::from(color.r) / 255.0,
        g: f32::from(color.g) / 255.0,
        b: f32::from(color.b) / 255.0,
        a: 1.0,
    }
}

pub(crate) fn rect_f(rect: RectF) -> D2D_RECT_F {
    D2D_RECT_F {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
    }
}

pub(crate) fn vector(point: PointF) -> Vector2 {
    Vector2 {
        X: point.x,
        Y: point.y,
    }
}

pub(crate) fn rounded(rect: RectF, radius: f32) -> D2D1_ROUNDED_RECT {
    let radius = clamp_radius(rect, radius);
    D2D1_ROUNDED_RECT {
        rect: rect_f(rect),
        radiusX: radius,
        radiusY: radius,
    }
}

pub(crate) fn ellipse(center: PointF, rx: f32, ry: f32) -> D2D1_ELLIPSE {
    D2D1_ELLIPSE {
        point: vector(center),
        radiusX: rx,
        radiusY: ry,
    }
}

impl Target {
    /// Creates a target for `hwnd` sized `width`×`height` device pixels at `dpi`.
    pub(crate) fn new(hwnd: Hwnd, width: u32, height: u32, dpi: f32) -> Result<Target> {
        let properties = D2D1_RENDER_TARGET_PROPERTIES {
            dpiX: dpi,
            dpiY: dpi,
            ..Default::default()
        };
        let window = D2D1_HWND_RENDER_TARGET_PROPERTIES {
            hwnd: raw_hwnd(hwnd),
            pixelSize: D2D_SIZE_U { width, height },
            ..Default::default()
        };
        // SAFETY: both property structs are valid for the call and `hwnd` is a
        // live window; the factory keeps no pointer into them.
        let render = unsafe { factory()?.CreateHwndRenderTarget(&properties, &window) }
            .map_err(win32_error)?;
        Target::with_device(Device::Hwnd(render))
    }

    /// Creates an unbound target for a device context. [`Target::bind_dc`]
    /// attaches it to an `HDC` for one draw.
    pub(crate) fn new_dc() -> Result<Target> {
        // `D2D1_ALPHA_MODE_IGNORE` keeps the DC opaque: the anti-aliased edges
        // are blended into the pixels GDI already painted, and a later
        // `BitBlt`/`GetPixel` never sees the premultiplied black that would
        // otherwise fringe every shape.
        let properties = D2D1_RENDER_TARGET_PROPERTIES {
            pixelFormat: windows::Win32::Graphics::Direct2D::Common::D2D1_PIXEL_FORMAT {
                format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: windows::Win32::Graphics::Direct2D::Common::D2D1_ALPHA_MODE_IGNORE,
            },
            dpiX: BASE_DPI,
            dpiY: BASE_DPI,
            ..Default::default()
        };
        // SAFETY: the property struct is valid for the call.
        let render =
            unsafe { factory()?.CreateDCRenderTarget(&properties) }.map_err(win32_error)?;
        Target::with_device(Device::Dc(render))
    }

    /// Shares the resource caches, binding the base interface of `device` for
    /// every draw call.
    fn with_device(device: Device) -> Result<Target> {
        let render: ID2D1RenderTarget = match &device {
            Device::Hwnd(target) => target.cast(),
            Device::Dc(target) => target.cast(),
        }
        .map_err(win32_error)?;
        Ok(Target {
            render,
            device,
            brushes: HashMap::new(),
            strokes: brush::Strokes::new(),
            paints: brush::Paints::new(),
            images: bitmap::Images::new(),
            shapes: geometry::Shapes::new(),
        })
    }

    /// Binds a device-context target to `hdc`, mapping the rectangle `rect` to
    /// the target origin. Cheap: call it before every draw.
    pub(crate) fn bind_dc(&self, hdc: HDC, rect: Rect) -> Result<()> {
        let Device::Dc(target) = &self.device else {
            return Err(Error::Direct2d("bind_dc on a window render target"));
        };
        let sub = RECT {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        };
        // SAFETY: `hdc` is live for the draw and `sub` is a valid rectangle the
        // target copies before returning.
        unsafe { target.BindDC(hdc, &sub) }.map_err(win32_error)
    }

    /// Resizes the backing surface to `width`×`height` device pixels. A DC
    /// target has no backing surface, so this does nothing for it.
    pub(crate) fn resize(&mut self, width: u32, height: u32) -> Result<()> {
        let Device::Hwnd(target) = &self.device else {
            return Ok(());
        };
        // SAFETY: the size struct is valid for the call.
        unsafe { target.Resize(&D2D_SIZE_U { width, height }) }.map_err(win32_error)
    }

    /// Sets the DPI that maps device-independent to device pixels.
    pub(crate) fn set_dpi(&self, dpi: f32) {
        // SAFETY: plain value arguments on a live target.
        unsafe { self.render.SetDpi(dpi, dpi) }
    }

    /// The target's size in device-independent pixels.
    pub(crate) fn size(&self) -> (f32, f32) {
        // SAFETY: a getter on a live target.
        let size = unsafe { self.render.GetSize() };
        (size.width, size.height)
    }

    pub(crate) fn begin_draw(&mut self) {
        // SAFETY: a live target; the safe layer pairs every `begin_draw` with
        // an `end_draw`.
        unsafe { self.render.BeginDraw() }
    }

    /// Ends the frame, popping any clip left open, and reports whether the
    /// device was lost.
    pub(crate) fn end_draw(&mut self) -> Result<EndDraw> {
        self.drain_clips();
        // SAFETY: pairs the `begin_draw`; the tag out-pointers are optional.
        match unsafe { self.render.EndDraw(None, None) } {
            Ok(()) => Ok(EndDraw::Presented),
            Err(error) if is_target_lost(error.code().0) => Ok(EndDraw::TargetLost),
            Err(error) => Err(win32_error(error)),
        }
    }

    pub(crate) fn clear(&mut self, color: Color) {
        // SAFETY: the colour struct is valid for the call; drawing is active.
        unsafe { self.render.Clear(Some(&color_f(color))) }
    }

    pub(crate) fn brush(&mut self, color: Color) -> Option<ID2D1SolidColorBrush> {
        if let Some(brush) = self.brushes.get(&color) {
            return Some(brush.clone());
        }
        // SAFETY: the colour struct is valid for the call; default properties.
        let brush = unsafe { self.render.CreateSolidColorBrush(&color_f(color), None) }.ok()?;
        self.brushes.insert(color, brush.clone());
        Some(brush)
    }

    pub(crate) fn fill_rect(&mut self, rect: RectF, color: Color) {
        if let Some(brush) = self.brush(color) {
            // SAFETY: valid rect and brush from this target; drawing is active.
            unsafe { self.render.FillRectangle(&rect_f(rect), &brush) }
        }
    }

    pub(crate) fn fill_rounded_rect(&mut self, rect: RectF, radius: f32, color: Color) {
        if let Some(brush) = self.brush(color) {
            // SAFETY: valid geometry and brush from this target.
            unsafe {
                self.render
                    .FillRoundedRectangle(&rounded(rect, radius), &brush)
            }
        }
    }

    pub(crate) fn fill_ellipse(&mut self, center: PointF, rx: f32, ry: f32, color: Color) {
        if let Some(brush) = self.brush(color) {
            // SAFETY: valid geometry and brush from this target.
            unsafe { self.render.FillEllipse(&ellipse(center, rx, ry), &brush) }
        }
    }

    pub(crate) fn stroke_rect(&mut self, rect: RectF, color: Color, stroke: Stroke) {
        if let Some((brush, style)) = self.pen(color, stroke) {
            // SAFETY: valid rect, brush and stroke style from this target.
            unsafe {
                self.render
                    .DrawRectangle(&rect_f(rect), &brush, stroke.width, style.as_ref())
            }
        }
    }

    pub(crate) fn stroke_rounded_rect(
        &mut self,
        rect: RectF,
        radius: f32,
        color: Color,
        stroke: Stroke,
    ) {
        if let Some((brush, style)) = self.pen(color, stroke) {
            // SAFETY: valid geometry, brush and stroke style from this target.
            unsafe {
                self.render.DrawRoundedRectangle(
                    &rounded(rect, radius),
                    &brush,
                    stroke.width,
                    style.as_ref(),
                )
            }
        }
    }

    pub(crate) fn stroke_ellipse(
        &mut self,
        center: PointF,
        rx: f32,
        ry: f32,
        color: Color,
        stroke: Stroke,
    ) {
        if let Some((brush, style)) = self.pen(color, stroke) {
            // SAFETY: valid geometry, brush and stroke style from this target.
            unsafe {
                self.render.DrawEllipse(
                    &ellipse(center, rx, ry),
                    &brush,
                    stroke.width,
                    style.as_ref(),
                )
            }
        }
    }

    pub(crate) fn line(&mut self, from: PointF, to: PointF, color: Color, stroke: Stroke) {
        if let Some((brush, style)) = self.pen(color, stroke) {
            // SAFETY: valid points, brush and stroke style from this target.
            unsafe {
                self.render.DrawLine(
                    vector(from),
                    vector(to),
                    &brush,
                    stroke.width,
                    style.as_ref(),
                )
            }
        }
    }

    pub(crate) fn set_translation(&mut self, x: f32, y: f32) {
        // SAFETY: the matrix is valid for the call.
        unsafe { self.render.SetTransform(&Matrix3x2::translation(x, y)) }
    }
}
