//! Raw `ImageList_*` calls behind the RAII [`ImageList`](crate::ImageList)
//! control type.
//!
//! `ImageList_Add` copies the pixels, so the temporary DIB this creates is
//! deleted immediately; only the returned index is kept.

use windows::Win32::Graphics::Gdi::{HBITMAP, HGDIOBJ};
use windows::Win32::UI::Controls::{
    HIMAGELIST, ILC_COLOR32, ImageList_Add, ImageList_Create, ImageList_Destroy,
    ImageList_GetImageCount,
};

use crate::error::{Error, Result};

/// Creates a 32-bpp image list whose images are `icon` square device pixels
/// (`ImageList_Create` itself clamps a non-positive size to 1).
pub(crate) fn create(icon: i32) -> Result<HIMAGELIST> {
    let icon = icon.max(1);
    // SAFETY: `ImageList_Create` takes plain values; ownership of the returned
    // handle passes to the caller, which destroys it in `Drop`.
    let list = unsafe { ImageList_Create(icon, icon, ILC_COLOR32, 0, 1) };
    if list.0 == 0 {
        Err(Error::Gdi("image list"))
    } else {
        Ok(list)
    }
}

/// Adds a top-down RGBA image (`width * height * 4` bytes), returning its
/// index, or `None` if the image could not be added.
pub(crate) fn add_rgba(list: HIMAGELIST, width: i32, height: i32, rgba: &[u8]) -> Option<usize> {
    let bitmap: HBITMAP = crate::sys::gdi::create_dib(width, height, rgba).ok()?;
    // SAFETY: `list` is live; `bitmap` is a 32-bpp DIB owned by this function
    // and valid for the call. `ImageList_Add` copies its pixels.
    let index = unsafe { ImageList_Add(list, bitmap, None) };
    // SAFETY: the image list copied the bitmap, so the DIB is ours to delete.
    crate::sys::gdi::delete_object(HGDIOBJ(bitmap.0));
    if index < 0 {
        None
    } else {
        Some(index as usize)
    }
}

/// The number of images in the list.
pub(crate) fn count(list: HIMAGELIST) -> usize {
    // SAFETY: `list` is live and owned by the caller.
    unsafe { ImageList_GetImageCount(list) as usize }
}

/// Destroys an image list. Called once per handle from `ImageList::drop`.
pub(crate) fn destroy(list: HIMAGELIST) {
    // SAFETY: `list` was created by `create` and is owned by the caller.
    unsafe {
        let _ = ImageList_Destroy(Some(list));
    }
}
