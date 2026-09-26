//! Window icons: an `HICON` built from RGBA pixels, converted back to RGBA for
//! painting, and installed on a window.

use core::ffi::c_void;
use core::ptr::null_mut;

use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateCompatibleDC,
    CreateDIBSection, DIB_RGB_COLORS, DeleteDC, GetDIBits, GetObjectW, HGDIOBJ, SelectObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconIndirect, DI_NORMAL, DestroyIcon, DrawIconEx, GetIconInfo, GetSystemMetrics, HICON,
    ICON_BIG, ICON_SMALL, ICONINFO, IMAGE_ICON, LR_DEFAULTSIZE, LoadImageW, SM_CXICON, SM_CYICON,
    SendMessageW, WM_SETICON,
};
use windows::core::{BOOL, PCWSTR};

use crate::capture::RgbaImage;
use crate::error::{Error, Result};
use crate::hwnd::Hwnd;

use super::{raw_hwnd, win32_error};

/// Builds a 32-bit icon from tightly packed RGBA pixels.
pub(crate) fn create_icon(width: i32, height: i32, rgba: &[u8]) -> Result<HICON> {
    let color = super::gdi::create_dib(width, height, rgba)?;
    // A 1-bpp AND mask: all-zero bits mark every pixel opaque, leaving the
    // colour bitmap's alpha channel to decide transparency. `CreateBitmap`
    // leaves the bits undefined for a null pointer, so pass a zeroed buffer.
    let stride = (width as usize).div_ceil(16) * 2;
    let mask_bits = vec![0u8; stride * height as usize];
    // SAFETY: `mask_bits` is a readable buffer of the size `CreateBitmap`
    // expects for a 1-bpp bitmap and outlives the call.
    let mask = unsafe {
        CreateBitmap(
            width,
            height,
            1,
            1,
            Some(mask_bits.as_ptr() as *const c_void),
        )
    };
    if mask.0.is_null() {
        super::gdi::delete_object(HGDIOBJ(color.0));
        return Err(Error::Icon("mask bitmap"));
    }

    let info = ICONINFO {
        fIcon: BOOL(1),
        xHotspot: 0,
        yHotspot: 0,
        hbmMask: mask,
        hbmColor: color,
    };
    // SAFETY: `info` points at two live bitmaps for the duration of the call,
    // which copies them into the icon it returns.
    let icon = unsafe { CreateIconIndirect(&info) };
    // The icon owns copies, so the source bitmaps are ours to delete either way.
    super::gdi::delete_object(HGDIOBJ(color.0));
    super::gdi::delete_object(HGDIOBJ(mask.0));
    icon.map_err(win32_error)
}

/// Loads a private copy of the icon resource `id` from this module at the
/// system icon size, for [`Icon`](crate::Icon) to own and destroy. `None` when
/// the program has no such resource.
pub(crate) fn load_icon(id: u16) -> Option<(HICON, i32, i32)> {
    let module = unsafe { GetModuleHandleW(None) }.ok()?;
    // SAFETY: `GetSystemMetrics` takes an index; `LoadImageW` reads a resource
    // name in our own module and returns a new icon the caller owns.
    let width = unsafe { GetSystemMetrics(SM_CXICON) };
    let height = unsafe { GetSystemMetrics(SM_CYICON) };
    let name = PCWSTR(id as usize as *const u16);
    let handle = unsafe {
        LoadImageW(
            Some(module.into()),
            name,
            IMAGE_ICON,
            width,
            height,
            LR_DEFAULTSIZE,
        )
    }
    .ok()?;
    Some((HICON(handle.0), width, height))
}

/// Reads `icon`'s pixels back as tightly packed, top-down, straight-alpha RGBA.
///
/// The icon is drawn into a 32-bpp top-down DIB with `DrawIconEx`, which
/// composes its colour bitmap and its AND mask, then the DIB is copied out with
/// `GetDIBits`. The size comes from the icon's own colour bitmap, so a caller
/// need not know it.
pub(crate) fn icon_rgba(icon: HICON) -> Result<RgbaImage> {
    let mut icon_info = ICONINFO::default();
    // SAFETY: `icon_info` is a valid out-parameter; `GetIconInfo` fills it and
    // hands back the colour and mask bitmaps the caller must delete.
    unsafe { GetIconInfo(icon, &mut icon_info) }.map_err(win32_error)?;

    let mut bitmap = BITMAP::default();
    // SAFETY: `bitmap` is a `BITMAP` and the size passed matches its type, so
    // `GetObjectW` writes at most that many bytes.
    let read = unsafe {
        GetObjectW(
            HGDIOBJ(icon_info.hbmColor.0),
            size_of::<BITMAP>() as i32,
            Some(&mut bitmap as *mut BITMAP as *mut c_void),
        )
    };
    // The icon owns copies of both bitmaps, so these are ours to delete either
    // way and must be deleted even when the size read failed.
    super::gdi::delete_object(HGDIOBJ(icon_info.hbmColor.0));
    super::gdi::delete_object(HGDIOBJ(icon_info.hbmMask.0));
    if read == 0 || bitmap.bmWidth <= 0 || bitmap.bmHeight <= 0 {
        return Err(Error::Icon("bitmap size"));
    }
    let width = bitmap.bmWidth;
    let height = bitmap.bmHeight;

    let mut info = BITMAPINFO::default();
    info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    info.bmiHeader.biWidth = width;
    info.bmiHeader.biHeight = -height; // negative: top-down rows
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB.0;

    let mut bits: *mut c_void = null_mut();
    // SAFETY: `info` is a fully initialised `BITMAPINFO` and `bits` a valid
    // out-pointer; a null source DC asks for a plain DIB section.
    let dib = unsafe { CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0) }
        .map_err(win32_error)?;
    if bits.is_null() {
        super::gdi::delete_object(HGDIOBJ(dib.0));
        return Err(Error::Icon("DIB section"));
    }
    // SAFETY: a null DC asks for a memory DC compatible with the screen.
    let dc = unsafe { CreateCompatibleDC(None) };
    if dc.0.is_null() {
        super::gdi::delete_object(HGDIOBJ(dib.0));
        return Err(Error::Icon("memory DC"));
    }
    // SAFETY: `dib` is a live bitmap and `dc` a live DC.
    let previous = unsafe { SelectObject(dc, HGDIOBJ(dib.0)) };

    // SAFETY: `dc` has `dib` selected, and the icon and target size are valid;
    // `DI_NORMAL` composites the colour bitmap and the mask.
    let drawn = unsafe { DrawIconEx(dc, 0, 0, icon, width, height, 0, None, DI_NORMAL) };
    let result = drawn.map_err(win32_error).and_then(|()| {
        let bytes = width as usize * height as usize * 4;
        let mut buffer = vec![0u8; bytes];
        // SAFETY: `buffer` holds `bytes` writable bytes, `info` describes the
        // DIB, and both `dc` and `dib` are live with `dib` selected.
        let copied = unsafe {
            GetDIBits(
                dc,
                dib,
                0,
                height as u32,
                Some(buffer.as_mut_ptr() as *mut c_void),
                &mut info,
                DIB_RGB_COLORS,
            )
        };
        if copied == 0 {
            return Err(Error::Icon("icon pixels"));
        }
        let mut pixels = Vec::with_capacity(bytes);
        for pixel in buffer.as_chunks::<4>().0 {
            // Windows 32-bpp DIBs are BGRA; expose RGBA.
            pixels.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
        Ok(RgbaImage {
            width: width as u32,
            height: height as u32,
            pixels,
        })
    });

    // SAFETY: the previous bitmap is restored before the DC and DIB are freed.
    unsafe {
        SelectObject(dc, previous);
        let _ = DeleteDC(dc);
    }
    super::gdi::delete_object(HGDIOBJ(dib.0));
    result
}

/// Destroys an icon returned by [`create_icon`].
pub(crate) fn destroy_icon(icon: HICON) {
    // SAFETY: `icon` came from `CreateIconIndirect`; destroying a stale handle
    // is a documented failure, not undefined behaviour.
    unsafe {
        let _ = DestroyIcon(icon);
    }
}

/// Installs `icon` as the window's large and small icon.
pub(crate) fn set_icon(hwnd: Hwnd, icon: HICON) {
    let value = LPARAM(icon.0 as isize);
    // SAFETY: `WM_SETICON` reads the icon handle from `lparam`; the window does
    // not take ownership, so the caller keeps the `Icon` alive.
    unsafe {
        let _ = SendMessageW(
            raw_hwnd(hwnd),
            WM_SETICON,
            Some(WPARAM(ICON_BIG as usize)),
            Some(value),
        );
        let _ = SendMessageW(
            raw_hwnd(hwnd),
            WM_SETICON,
            Some(WPARAM(ICON_SMALL as usize)),
            Some(value),
        );
    }
}
