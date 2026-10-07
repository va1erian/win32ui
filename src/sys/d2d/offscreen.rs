//! An offscreen Direct2D target: a software `ID2D1DCRenderTarget` bound to a
//! 32-bpp top-down DIB section, so a frame can be drawn and read back without
//! a window, a GPU or a readable desktop.

use core::ffi::c_void;
use core::ptr::null_mut;

use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
    DeleteDC, DeleteObject, GdiFlush, HBITMAP, HDC, HGDIOBJ, SelectObject,
};

use crate::capture::RgbaImage;
use crate::error::{Error, Result};
use crate::geometry::Rect;

use super::target::Target;

/// A memory DC with a DIB section selected, owning both.
pub(crate) struct Offscreen {
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *const u8,
    width: u32,
    height: u32,
}

impl Offscreen {
    /// Creates a `width`×`height` pixel buffer and a software render target
    /// bound to it at `dpi`. The buffer must outlive the target, so the caller
    /// keeps both and drops the target first.
    pub(crate) fn new(width: u32, height: u32, dpi: f32) -> Result<(Offscreen, Target)> {
        let (w, h) = (
            i32::try_from(width).map_err(|_| Error::Gdi("offscreen size"))?,
            i32::try_from(height).map_err(|_| Error::Gdi("offscreen size"))?,
        );
        if w <= 0 || h <= 0 {
            return Err(Error::Gdi("offscreen size"));
        }
        let mut info = BITMAPINFO::default();
        info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = w;
        info.bmiHeader.biHeight = -h; // negative: top-down rows
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB.0;

        // SAFETY: a null DC asks for a memory DC compatible with the screen;
        // `Offscreen::drop` (or the error path below) deletes it.
        let dc = unsafe { CreateCompatibleDC(None) };
        if dc.0.is_null() {
            return Err(Error::Gdi("offscreen DC"));
        }
        let mut bits: *mut c_void = null_mut();
        // SAFETY: `info` is a fully initialised BITMAPINFO, `bits` is a valid
        // out-pointer, and no file mapping is used.
        let bitmap =
            unsafe { CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) };
        let bitmap = match bitmap {
            Ok(bitmap) if !bits.is_null() => bitmap,
            other => {
                // SAFETY: the bitmap (if any) was just created and is selected
                // nowhere; `dc` is live and empty.
                unsafe {
                    if let Ok(bitmap) = other {
                        let _ = DeleteObject(HGDIOBJ(bitmap.0));
                    }
                    let _ = DeleteDC(dc);
                }
                return Err(Error::Gdi("offscreen DIB"));
            }
        };
        // SAFETY: both handles are live; `Drop` restores `old` before deleting.
        let old = unsafe { SelectObject(dc, HGDIOBJ(bitmap.0)) };
        let offscreen = Offscreen {
            dc,
            bitmap,
            old,
            bits: bits.cast_const().cast(),
            width,
            height,
        };
        let target = Target::new_dc(true)?;
        target.set_dpi(dpi);
        target.bind_dc(offscreen.dc, Rect::new(0, 0, w, h))?;
        Ok((offscreen, target))
    }

    /// The buffer's size in device pixels.
    pub(crate) fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Reads the buffer back as straight RGBA with opaque alpha. Call it after
    /// the frame drawn into the bound target has ended.
    pub(crate) fn read(&self) -> RgbaImage {
        let bytes = self.width as usize * self.height as usize * 4;
        // SAFETY: flushes GDI's batch for this thread so the DIB holds every
        // drawn pixel before it is read; takes no arguments.
        unsafe {
            let _ = GdiFlush();
        }
        // SAFETY: `bits` points at the DIB section's `width * height * 4`
        // bytes, which live as long as `self` (the bitmap is deleted in `Drop`).
        let source = unsafe { core::slice::from_raw_parts(self.bits, bytes) };
        let mut pixels = Vec::with_capacity(bytes);
        for pixel in source.as_chunks::<4>().0 {
            // 32-bpp DIBs are BGRA; the target ignores alpha, so force opaque.
            pixels.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 0xFF]);
        }
        RgbaImage {
            width: self.width,
            height: self.height,
            pixels,
        }
    }
}

impl Drop for Offscreen {
    fn drop(&mut self) {
        // SAFETY: `old` was returned by selecting `bitmap`; restoring it lets
        // the DC and the bitmap be deleted without a dangling selection.
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
            let _ = DeleteDC(self.dc);
        }
    }
}
