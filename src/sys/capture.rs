//! Rendering a window's pixels with `PrintWindow` into a top-down DIB section.

use core::ffi::c_void;
use core::ptr::{null_mut, slice_from_raw_parts_mut};

use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, HGDIOBJ, ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};
use windows::Win32::UI::WindowsAndMessaging::PW_RENDERFULLCONTENT;

use crate::error::{Error, Result};
use crate::geometry::Rect;
use crate::hwnd::Hwnd;

use super::{raw_hwnd, win32_error};

/// A rendered window: `width * height * 4` RGBA bytes, row-major, top-down.
pub(crate) struct Captured {
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) pixels: Vec<u8>,
}

/// Renders `hwnd` at `width`×`height` into a fresh DIB section and returns its
/// pixels as RGBA (alpha forced to 255).
///
/// `PW_RENDERFULLCONTENT` asks the window to draw itself into the supplied DC,
/// so occluded windows and DirectComposition content are captured correctly.
pub(crate) fn capture(hwnd: Hwnd, width: i32, height: i32) -> Result<Captured> {
    if width <= 0 || height <= 0 {
        return Err(Error::Gdi("window capture size"));
    }

    let mut info = BITMAPINFO::default();
    info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    info.bmiHeader.biWidth = width;
    info.bmiHeader.biHeight = -height; // negative: top-down rows
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB.0;

    // SAFETY: a null DC asks for a memory DC compatible with the screen; it is
    // released below.
    let memory_dc = unsafe { CreateCompatibleDC(None) };
    if memory_dc.0.is_null() {
        return Err(Error::Gdi("window capture DC"));
    }

    let mut bits: *mut c_void = null_mut();
    // SAFETY: `info` is a fully initialised BITMAPINFO, `bits` is a valid
    // out-pointer, and no source DC/section is used.
    let bitmap = match unsafe { CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0) }
    {
        Ok(bitmap) if !bits.is_null() => bitmap,
        Ok(bitmap) => {
            // SAFETY: `bitmap` was just created and is not selected anywhere;
            // `memory_dc` is live and empty.
            unsafe {
                let _ = DeleteObject(HGDIOBJ(bitmap.0));
                let _ = DeleteDC(memory_dc);
            }
            return Err(Error::Gdi("window capture DIB"));
        }
        Err(source) => {
            // SAFETY: `memory_dc` is live and has no bitmap selected.
            unsafe {
                let _ = DeleteDC(memory_dc);
            }
            return Err(win32_error(source));
        }
    };

    // SAFETY: `bitmap` and `memory_dc` are live; the previously selected object
    // is restored before both are released.
    let old = unsafe { SelectObject(memory_dc, HGDIOBJ(bitmap.0)) };

    // SAFETY: `hwnd` identifies the window being captured, `memory_dc` has the
    // DIB selected, and the flag comes from the Windows SDK (`winuser.h`).
    let painted = unsafe {
        PrintWindow(
            raw_hwnd(hwnd),
            memory_dc,
            PRINT_WINDOW_FLAGS(PW_RENDERFULLCONTENT),
        )
    };

    let pixels = if painted.as_bool() {
        let bytes = width as usize * height as usize * 4;
        // SAFETY: `bits` points at `bytes` writable bytes owned by the DIB
        // section, which stays selected and alive until the cleanup below.
        let source = unsafe { &*slice_from_raw_parts_mut(bits as *mut u8, bytes) };
        let mut pixels = Vec::with_capacity(bytes);
        for pixel in source.as_chunks::<4>().0 {
            // Windows 32-bpp DIBs are BGRA; expose RGBA and force opaque alpha
            // because `PrintWindow` leaves the alpha channel undefined.
            pixels.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 0xFF]);
        }
        pixels
    } else {
        Vec::new()
    };

    // SAFETY: `old` was returned by selecting `bitmap`; restoring it lets both
    // the DC and the bitmap be released without leaving a dangling selection.
    unsafe {
        SelectObject(memory_dc, old);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(memory_dc);
    }

    if !painted.as_bool() {
        return Err(win32_error(windows::core::Error::from_thread()));
    }

    Ok(Captured {
        width,
        height,
        pixels,
    })
}

/// Renders the screen region `rect` (screen coordinates) into a fresh DIB
/// section, via a `BitBlt` from the screen DC, and returns its pixels as RGBA
/// (alpha forced to 255).
///
/// Unlike [`capture`], this includes everything on screen inside `rect`: the
/// DWM-drawn frame, caption buttons, popups and the backdrop material. The
/// window must be on screen and unobscured for the result to be meaningful.
pub(crate) fn capture_screen(rect: Rect) -> Result<Captured> {
    let width = rect.width();
    let height = rect.height();
    if width <= 0 || height <= 0 {
        return Err(Error::Gdi("screen capture size"));
    }

    let mut info = BITMAPINFO::default();
    info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    info.bmiHeader.biWidth = width;
    info.bmiHeader.biHeight = -height; // negative: top-down rows
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB.0;

    // SAFETY: a null DC asks for a memory DC compatible with the screen; it is
    // released below.
    let memory_dc = unsafe { CreateCompatibleDC(None) };
    if memory_dc.0.is_null() {
        return Err(Error::Gdi("screen capture DC"));
    }

    let mut bits: *mut c_void = null_mut();
    // SAFETY: `info` is fully initialised, `bits` is a valid out-pointer, and
    // no source DC/section is used.
    let bitmap = match unsafe { CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0) }
    {
        Ok(bitmap) if !bits.is_null() => bitmap,
        Ok(bitmap) => {
            // SAFETY: `bitmap` was just created; `memory_dc` is live and empty.
            unsafe {
                let _ = DeleteObject(HGDIOBJ(bitmap.0));
                let _ = DeleteDC(memory_dc);
            }
            return Err(Error::Gdi("screen capture DIB"));
        }
        Err(source) => {
            // SAFETY: `memory_dc` is live and has no bitmap selected.
            unsafe {
                let _ = DeleteDC(memory_dc);
            }
            return Err(win32_error(source));
        }
    };

    // SAFETY: `bitmap` and `memory_dc` are live; the previously selected object
    // is restored before both are released.
    let old = unsafe { SelectObject(memory_dc, HGDIOBJ(bitmap.0)) };

    // SAFETY: a null window asks for the screen DC; it is released below.
    let screen_dc = unsafe { GetDC(None) };
    if screen_dc.0.is_null() {
        // SAFETY: `old` was returned by selecting `bitmap`; restoring it lets
        // the DC and bitmap be released cleanly.
        unsafe {
            SelectObject(memory_dc, old);
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            let _ = DeleteDC(memory_dc);
        }
        return Err(Error::Gdi("screen capture DC"));
    }

    // SAFETY: both DCs are live and `rect`/the DIB geometry are valid; `SRCCOPY`
    // is the documented plain-copy raster op.
    let copied = unsafe {
        BitBlt(
            memory_dc,
            0,
            0,
            width,
            height,
            Some(screen_dc),
            rect.left,
            rect.top,
            SRCCOPY,
        )
    };
    // SAFETY: `screen_dc` came from `GetDC(None)` above.
    unsafe {
        let _ = ReleaseDC(None, screen_dc);
    }

    let pixels = if copied.is_ok() {
        let bytes = width as usize * height as usize * 4;
        // SAFETY: `bits` points at `bytes` writable bytes owned by the DIB
        // section, which stays selected and alive until the cleanup below.
        let source = unsafe { &*slice_from_raw_parts_mut(bits as *mut u8, bytes) };
        let mut pixels = Vec::with_capacity(bytes);
        for pixel in source.as_chunks::<4>().0 {
            // Windows 32-bpp DIBs are BGRA; expose RGBA with opaque alpha.
            pixels.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 0xFF]);
        }
        pixels
    } else {
        Vec::new()
    };

    // SAFETY: `old` was returned by selecting `bitmap`; restoring it lets both
    // the DC and the bitmap be released without leaving a dangling selection.
    unsafe {
        SelectObject(memory_dc, old);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(memory_dc);
    }

    if !copied.is_ok() {
        return Err(win32_error(windows::core::Error::from_thread()));
    }

    Ok(Captured {
        width,
        height,
        pixels,
    })
}
