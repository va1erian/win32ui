//! Window icons: an `HICON` built from RGBA pixels and installed on a window.

use core::ffi::c_void;

use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateBitmap, HGDIOBJ};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconIndirect, DestroyIcon, HICON, ICON_BIG, ICON_SMALL, ICONINFO, SendMessageW,
    WM_SETICON,
};
use windows::core::BOOL;

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
