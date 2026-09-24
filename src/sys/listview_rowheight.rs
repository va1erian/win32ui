//! Sets a report-mode list view's row height via the "1px image list" trick.

use windows::Win32::UI::Controls::{
    HIMAGELIST, IMAGELIST_CREATION_FLAGS, ImageList_Create, ImageList_Destroy, LVM_SETIMAGELIST,
    LVSIL_SMALL,
};

use crate::hwnd::Hwnd;

use super::control::send;

// `ILC_COLOR32`, from `commctrl.h`: a 32-bit-colour (with alpha) image list.
const ILC_COLOR32: IMAGELIST_CREATION_FLAGS = IMAGELIST_CREATION_FLAGS(0x0000_0020);

/// Sets a fixed row height (in device pixels) on a report-mode list view via
/// the classic "1px image list" trick, then destroys `previous` (if any) now
/// that it is no longer attached.
///
/// This control is `LVS_OWNERDATA` and paints every row itself in
/// `NM_CUSTOMDRAW`, so `LVS_OWNERDRAWFIXED` + `WM_MEASUREITEM` is not an
/// option: that style pair targets a plain (non-virtual) owner-draw list, and
/// `WM_MEASUREITEM` is never sent for `LVS_OWNERDATA` — a virtual list never
/// gets to answer it. ListView instead derives its report-mode row height
/// from the tallest of the font's line height and the state/small image
/// list's image height; attaching a 1×`height` image list with no images
/// (`LVSIL_SMALL`) is the documented, allocation-free way to raise that floor
/// without displaying anything, and it composes with the existing
/// `NM_CUSTOMDRAW` painting unchanged.
///
/// Returns the new image list handle (to store and pass back in as
/// `previous` on the next call, and to destroy in `Drop` with
/// [`lv_destroy_image_list`]), or `previous` unchanged if creation failed.
pub(crate) fn lv_set_row_height(
    hwnd: Hwnd,
    height: i32,
    previous: Option<HIMAGELIST>,
) -> Option<HIMAGELIST> {
    // SAFETY: creates a 1x`height` image list with no images; ownership
    // transfers to the caller, which stores the handle and destroys it
    // (either by replacing it here or via `lv_destroy_image_list`).
    let list = unsafe { ImageList_Create(1, height.max(1), ILC_COLOR32, 0, 1) };
    if list.0 == 0 {
        return previous;
    }
    send(
        hwnd,
        LVM_SETIMAGELIST,
        LVSIL_SMALL as usize,
        list.0 as isize,
    );
    if let Some(previous) = previous {
        lv_destroy_image_list(previous);
    }
    Some(list)
}

/// Destroys a row-height image list created by [`lv_set_row_height`].
pub(crate) fn lv_destroy_image_list(list: HIMAGELIST) {
    // SAFETY: `list` was created by `lv_set_row_height`, is owned by the
    // caller, and this runs at most once per handle (a fresh
    // `lv_set_row_height` call consumes `previous`, or `Drop` reclaims the
    // last one).
    unsafe {
        let _ = ImageList_Destroy(Some(list));
    }
}
