//! The drop source and the `DoDragDrop` call.

use windows::Win32::Foundation::{
    DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, POINT, S_OK,
};
use windows::Win32::Graphics::Gdi::{HGDIOBJ, ScreenToClient};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::Ole::{DROPEFFECT, DoDragDrop, IDropSource, IDropSource_Impl};
use windows::Win32::System::SystemServices::{MK_LBUTTON, MK_RBUTTON, MODIFIERKEYS_FLAGS};
use windows::Win32::UI::Shell::{CLSID_DragDropHelper, IDragSourceHelper, SHDRAGIMAGE};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
use windows::core::{BOOL, HRESULT, implement};

use crate::error::Result;
use crate::hwnd::Hwnd;

use super::super::{gdi, raw_hwnd, win32_error};
use super::{DataObj, DragImageBits, ole};

/// `CLR_NONE`: the drag image carries per-pixel alpha, no colour key.
const CLR_NONE: u32 = 0xFFFF_FFFF;

/// How a drag ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Dropped on a target that accepted the effect (raw `DROPEFFECT_*` bits).
    Dropped(u32),
    /// Cancelled with Escape, or dropped where nothing accepted it.
    Cancelled,
}

/// The `IDropSource`: ends the drag when the buttons are released or Escape is
/// pressed, and leaves the cursors to the system (which paints the drag image
/// and the effect glyph).
#[implement(IDropSource)]
struct DropSource;

#[allow(non_snake_case)]
impl IDropSource_Impl for DropSource_Impl {
    fn QueryContinueDrag(&self, fescapepressed: BOOL, grfkeystate: MODIFIERKEYS_FLAGS) -> HRESULT {
        if fescapepressed.as_bool() {
            DRAGDROP_S_CANCEL
        } else if grfkeystate.0 & (MK_LBUTTON.0 | MK_RBUTTON.0) == 0 {
            DRAGDROP_S_DROP
        } else {
            S_OK
        }
    }

    fn GiveFeedback(&self, _dweffect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

/// Attaches a drag image to `data`: `image` when given, else the one the
/// `source` window paints for itself (a list view answers `DI_GETDRAGIMAGE`).
/// Best effort: a drag without an image is still a drag.
fn attach_image(data: &DataObj, source: Hwnd, image: Option<DragImageBits>) {
    // SAFETY: creates the in-process shell drag helper; released on drop.
    let Ok(helper) = (unsafe {
        CoCreateInstance::<_, IDragSourceHelper>(&CLSID_DragDropHelper, None, CLSCTX_INPROC_SERVER)
    }) else {
        return;
    };
    if let Some(image) = image {
        // The DIB helper swaps R and B, so hand it RGBA and let it build BGRA.
        let Ok(bitmap) = gdi::create_dib(image.width, image.height, &image.pixels) else {
            return;
        };
        let info = SHDRAGIMAGE {
            sizeDragImage: windows::Win32::Foundation::SIZE {
                cx: image.width,
                cy: image.height,
            },
            ptOffset: POINT {
                x: image.hotspot.0,
                y: image.hotspot.1,
            },
            hbmpDragImage: bitmap,
            crColorKey: windows::Win32::Foundation::COLORREF(CLR_NONE),
        };
        // SAFETY: `info` is valid for the call; on success the helper owns and
        // frees the bitmap, on failure it is still ours to delete.
        if unsafe { helper.InitializeFromBitmap(&info, data.com()) }.is_err() {
            gdi::delete_object(HGDIOBJ(bitmap.0));
        }
        return;
    }
    if source.is_null() {
        return;
    }
    let mut point = POINT::default();
    // SAFETY: `point` is a valid out pointer; the conversion needs a live
    // window, which `source` is for the duration of the drag.
    let point = unsafe {
        let _ = GetCursorPos(&mut point);
        let _ = ScreenToClient(raw_hwnd(source), &mut point);
        point
    };
    // SAFETY: `point` outlives the call; `source` is a live window.
    let _ =
        unsafe { helper.InitializeFromWindow(Some(raw_hwnd(source)), Some(&point), data.com()) };
}

/// Runs the modal `DoDragDrop` loop for `data` with `allowed` (raw
/// `DROPEFFECT_*` bits), returning how it ended.
///
/// The loop pumps messages: paint, timers and drop-target callbacks keep
/// running, while messages the app is sent are queued until it returns.
pub(crate) fn do_drag_drop(
    data: &DataObj,
    allowed: u32,
    source: Hwnd,
    image: Option<DragImageBits>,
) -> Result<Outcome> {
    ole::ensure()?;
    attach_image(data, source, image);
    let drop_source: IDropSource = DropSource.into();
    let mut effect = DROPEFFECT(0);
    // SAFETY: both COM objects are live for the call; `effect` is a valid out
    // pointer. The call is modal and returns once the drag ends.
    let result = unsafe { DoDragDrop(data.com(), &drop_source, DROPEFFECT(allowed), &mut effect) };
    if result == DRAGDROP_S_DROP {
        Ok(Outcome::Dropped(effect.0))
    } else if result == DRAGDROP_S_CANCEL {
        Ok(Outcome::Cancelled)
    } else {
        result.ok().map_err(win32_error)?;
        Ok(Outcome::Cancelled)
    }
}
