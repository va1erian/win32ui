//! Device-dependent bitmap and bitmap-brush caches. Bitmaps are uploaded
//! premultiplied from a retained `Arc` copy (owned by the surface), so they
//! can be re-uploaded lazily after a device loss.

use std::collections::HashMap;

use windows::Win32::Graphics::Direct2D::Common::{
    D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_BRUSH_PROPERTIES, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
    D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR, D2D1_BITMAP_PROPERTIES, D2D1_EXTEND_MODE_WRAP,
    ID2D1Bitmap, ID2D1BitmapBrush, ID2D1RenderTarget,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM;

use crate::capture::RgbaImage;
use crate::d2d::{BASE_DPI, ImageId, Interpolation, RectF};

use super::target::{Target, rect_f};

/// Bitmaps and tiled bitmap brushes, cached per render target.
pub(crate) struct Images {
    bitmaps: HashMap<ImageId, ID2D1Bitmap>,
    tiled: HashMap<ImageId, ID2D1BitmapBrush>,
}

impl Images {
    pub(crate) fn new() -> Images {
        Images {
            bitmaps: HashMap::new(),
            tiled: HashMap::new(),
        }
    }

    /// The bitmap for `id`, creating (and premultiplying) it from `image` on
    /// first use.
    fn bitmap(
        &mut self,
        render: &ID2D1RenderTarget,
        id: ImageId,
        image: &RgbaImage,
    ) -> Option<ID2D1Bitmap> {
        if let Some(bitmap) = self.bitmaps.get(&id) {
            return Some(bitmap.clone());
        }
        let pixels = premultiply(image);
        let properties = D2D1_BITMAP_PROPERTIES {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_R8G8B8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: BASE_DPI,
            dpiY: BASE_DPI,
        };
        // SAFETY: `pixels` is a valid premultiplied buffer of the reported size
        // and pitch; the property struct is valid for the call.
        let bitmap = unsafe {
            render.CreateBitmap(
                D2D_SIZE_U {
                    width: image.width,
                    height: image.height,
                },
                Some(pixels.as_ptr().cast()),
                image.width * 4,
                &properties,
            )
        }
        .ok()?;
        self.bitmaps.insert(id, bitmap.clone());
        Some(bitmap)
    }

    /// A wrap-mode bitmap brush for tiling `id`.
    fn tiled(
        &mut self,
        render: &ID2D1RenderTarget,
        id: ImageId,
        image: &RgbaImage,
        interpolation: Interpolation,
    ) -> Option<ID2D1BitmapBrush> {
        if let Some(brush) = self.tiled.get(&id) {
            return Some(brush.clone());
        }
        let bitmap = self.bitmap(render, id, image)?;
        let properties = D2D1_BITMAP_BRUSH_PROPERTIES {
            extendModeX: D2D1_EXTEND_MODE_WRAP,
            extendModeY: D2D1_EXTEND_MODE_WRAP,
            interpolationMode: interpolation_mode(interpolation),
        };
        // SAFETY: valid bitmap (from this target) and property struct.
        let brush = unsafe { render.CreateBitmapBrush(&bitmap, Some(&properties), None) }.ok()?;
        self.tiled.insert(id, brush.clone());
        Some(brush)
    }
}

/// The Direct2D interpolation mode for a bitmap draw.
fn interpolation_mode(
    interpolation: Interpolation,
) -> windows::Win32::Graphics::Direct2D::D2D1_BITMAP_INTERPOLATION_MODE {
    match interpolation {
        Interpolation::Nearest => D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
        Interpolation::Linear | Interpolation::HighQualityCubic => {
            D2D1_BITMAP_INTERPOLATION_MODE_LINEAR
        }
    }
}

/// Converts straight-alpha RGBA to the premultiplied form Direct2D bitmaps use.
fn premultiply(image: &RgbaImage) -> Vec<u8> {
    let mut out = Vec::with_capacity(image.pixels.len());
    let (chunks, _) = image.pixels.as_chunks::<4>();
    for &[r, g, b, a] in chunks {
        let alpha = u32::from(a);
        let channel = |c: u8| ((u32::from(c) * alpha + 127) / 255) as u8;
        out.extend_from_slice(&[channel(r), channel(g), channel(b), a]);
    }
    out
}

impl Target {
    pub(crate) fn draw_image(
        &mut self,
        id: ImageId,
        image: &RgbaImage,
        dest: RectF,
        src: Option<RectF>,
        opacity: f32,
        interpolation: Interpolation,
    ) {
        if let Some(bitmap) = self.images.bitmap(&self.render, id, image) {
            let dest_rect = rect_f(dest);
            let src_rect = src.map(rect_f);
            // SAFETY: valid bitmap (from this target) and rectangles; drawing
            // is active.
            unsafe {
                self.render.DrawBitmap(
                    &bitmap,
                    Some(&dest_rect),
                    opacity,
                    interpolation_mode(interpolation),
                    src_rect.as_ref().map(|rect| rect as *const _),
                )
            }
        }
    }

    pub(crate) fn fill_image_tiled(
        &mut self,
        id: ImageId,
        image: &RgbaImage,
        dest: RectF,
        opacity: f32,
        interpolation: Interpolation,
    ) {
        if let Some(brush) = self.images.tiled(&self.render, id, image, interpolation) {
            // SAFETY: the brush is from this target; drawing is active.
            unsafe {
                brush.SetOpacity(opacity);
                self.render.FillRectangle(&rect_f(dest), &brush);
            }
        }
    }
}
