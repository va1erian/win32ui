#![forbid(unsafe_code)]

//! An RAII handle over a Win32 image list, used to give tree nodes icons.
//!
//! Build the list, add its images, then hand it to
//! [`TreeView::images`](super::TreeView::images). The list is owned by the
//! tree and destroyed when the tree drops; the `HIMAGELIST` itself never
//! appears in the public API.

use windows::Win32::UI::Controls::HIMAGELIST;

use crate::error::Result;
use crate::sys;

/// A Win32 image list of square, 32-bpp RGBA images.
///
/// Create one with [`new`](ImageList::new), add images with
/// [`add_rgba`](ImageList::add_rgba), and give it to a
/// [`TreeView`](super::TreeView) with `images`. Dropping the list releases the
/// underlying `HIMAGELIST`.
pub struct ImageList {
    handle: HIMAGELIST,
}

impl ImageList {
    /// Creates an empty list whose images are `icon` square device pixels.
    pub fn new(icon: i32) -> Result<ImageList> {
        Ok(ImageList {
            handle: sys::image_list::create(icon)?,
        })
    }

    /// Adds a top-down RGBA image (`width * height * 4` bytes, four bytes per
    /// pixel) and returns its index. Extra bytes past the image are ignored;
    /// `None` means the image could not be added.
    pub fn add_rgba(&mut self, width: i32, height: i32, rgba: &[u8]) -> Option<usize> {
        sys::image_list::add_rgba(self.handle, width, height, rgba)
    }

    /// The number of images added so far.
    pub fn len(&self) -> usize {
        // `ImageList_GetImageCount` is not wrapped; track it through the query.
        sys::image_list::count(self.handle)
    }

    /// Whether no image has been added yet.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub(crate) fn raw(&self) -> HIMAGELIST {
        self.handle
    }
}

impl Drop for ImageList {
    fn drop(&mut self) {
        sys::image_list::destroy(self.handle);
    }
}
