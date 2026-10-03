//! The drop target: an `IDropTarget` registered per window that forwards to a
//! [`TargetSink`], and the shell drop-target helper that paints the drag image.

use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use windows::Win32::Foundation::{E_INVALIDARG, HWND, POINT, POINTL};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, IDataObject};
use windows::Win32::System::Ole::{
    DROPEFFECT, IDropTarget, IDropTarget_Impl, RegisterDragDrop, RevokeDragDrop,
};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::Shell::{CLSID_DragDropHelper, IDropTargetHelper};
use windows::core::{Ref, implement};

use crate::error::Result;
use crate::hwnd::Hwnd;

use super::super::{raw_hwnd, win32_error};
use super::{DataObj, ole};

/// Where the pointer is during a drag, in the target window's client pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DragPoint {
    pub(crate) x: i32,
    pub(crate) y: i32,
    /// The `MK_*` key and button state.
    pub(crate) keys: u32,
    /// The `DROPEFFECT_*` bits the source allows.
    pub(crate) allowed: u32,
}

/// What a registered window does with a drag. Each call returns the raw
/// `DROPEFFECT_*` bits it would apply (the target masks them with what the
/// source allows).
pub(crate) trait TargetSink {
    fn enter(&self, point: DragPoint, data: &DataObj) -> u32;
    fn over(&self, point: DragPoint, data: &DataObj) -> u32;
    fn leave(&self);
    fn dropped(&self, point: DragPoint, data: &DataObj) -> u32;
}

#[implement(IDropTarget)]
struct DropTarget {
    hwnd: HWND,
    sink: Rc<dyn TargetSink>,
    helper: Option<IDropTargetHelper>,
    data: RefCell<Option<DataObj>>,
}

impl DropTarget {
    fn point(&self, keys: MODIFIERKEYS_FLAGS, screen: &POINTL, allowed: u32) -> DragPoint {
        let mut point = POINT {
            x: screen.x,
            y: screen.y,
        };
        // SAFETY: `point` is a valid in/out pointer and `hwnd` is the window
        // this target is registered for.
        unsafe {
            let _ = ScreenToClient(self.hwnd, &mut point);
        }
        DragPoint {
            x: point.x,
            y: point.y,
            keys: keys.0,
            allowed,
        }
    }
}

/// Runs `call` shielded from panics: unwinding into OLE is undefined.
fn guarded(call: impl FnOnce() -> u32) -> u32 {
    catch_unwind(AssertUnwindSafe(call)).unwrap_or(0)
}

/// Reads the effects the source allows from OLE's in/out pointer.
///
/// # Safety
/// `effect` must be the valid pointer OLE passed to the callback.
unsafe fn allowed_effects(effect: *mut DROPEFFECT) -> u32 {
    // SAFETY: guaranteed by the caller.
    unsafe { (*effect).0 }
}

/// Writes the chosen effect back through OLE's in/out pointer.
///
/// # Safety
/// `effect` must be the valid pointer OLE passed to the callback.
unsafe fn set_effect(effect: *mut DROPEFFECT, value: u32) {
    // SAFETY: guaranteed by the caller.
    unsafe { *effect = DROPEFFECT(value) };
}

#[allow(non_snake_case)]
impl IDropTarget_Impl for DropTarget_Impl {
    fn DragEnter(
        &self,
        pdataobj: Ref<IDataObject>,
        grfkeystate: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        let Some(object) = pdataobj.as_ref() else {
            return Err(E_INVALIDARG.into());
        };
        let data = DataObj::from_com(object.clone());
        // SAFETY: OLE passes a valid in/out effect pointer.
        let allowed = unsafe { allowed_effects(pdweffect) };
        let point = self.point(grfkeystate, pt, allowed);
        let effect = guarded(|| self.sink.enter(point, &data)) & allowed;
        // SAFETY: as above.
        unsafe { set_effect(pdweffect, effect) };
        if let Some(helper) = &self.helper {
            let screen = POINT { x: pt.x, y: pt.y };
            // SAFETY: the helper only reads `screen` and the data object.
            let _ = unsafe { helper.DragEnter(self.hwnd, object, &screen, DROPEFFECT(effect)) };
        }
        *self.data.borrow_mut() = Some(data);
        Ok(())
    }

    fn DragOver(
        &self,
        grfkeystate: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        let data = self.data.borrow().clone();
        let Some(data) = data else {
            // SAFETY: OLE passes a valid in/out effect pointer.
            unsafe { set_effect(pdweffect, 0) };
            return Ok(());
        };
        // SAFETY: as above.
        let allowed = unsafe { allowed_effects(pdweffect) };
        let point = self.point(grfkeystate, pt, allowed);
        let effect = guarded(|| self.sink.over(point, &data)) & allowed;
        // SAFETY: as above.
        unsafe { set_effect(pdweffect, effect) };
        if let Some(helper) = &self.helper {
            let screen = POINT { x: pt.x, y: pt.y };
            // SAFETY: the helper only reads `screen`.
            let _ = unsafe { helper.DragOver(&screen, DROPEFFECT(effect)) };
        }
        Ok(())
    }

    fn DragLeave(&self) -> windows::core::Result<()> {
        if let Some(helper) = &self.helper {
            // SAFETY: the helper takes no arguments.
            let _ = unsafe { helper.DragLeave() };
        }
        self.data.borrow_mut().take();
        guarded(|| {
            self.sink.leave();
            0
        });
        Ok(())
    }

    fn Drop(
        &self,
        pdataobj: Ref<IDataObject>,
        grfkeystate: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        let entered = self.data.borrow_mut().take();
        let data = match pdataobj.as_ref() {
            Some(object) => DataObj::from_com(object.clone()),
            None => entered.ok_or_else(|| windows::core::Error::from(E_INVALIDARG))?,
        };
        // SAFETY: OLE passes a valid in/out effect pointer.
        let allowed = unsafe { allowed_effects(pdweffect) };
        let point = self.point(grfkeystate, pt, allowed);
        // The helper first: it must hide the drag image before the sink does
        // anything slow (a confirmation dialog, say).
        if let Some(helper) = &self.helper {
            let screen = POINT { x: pt.x, y: pt.y };
            // SAFETY: the helper only reads `screen` and the data object.
            let _ = unsafe { helper.Drop(data.com(), &screen, DROPEFFECT(allowed)) };
        }
        let effect = guarded(|| self.sink.dropped(point, &data)) & allowed;
        // SAFETY: as above.
        unsafe { set_effect(pdweffect, effect) };
        Ok(())
    }
}

/// A registered drop target. Dropping it revokes the registration; do that
/// before the window is destroyed.
pub(crate) struct Registration {
    hwnd: Hwnd,
}

impl Drop for Registration {
    fn drop(&mut self) {
        // SAFETY: revokes the registration made in `register_target`; it fails
        // harmlessly when the window is already gone (OLE then dropped it).
        let _ = unsafe { RevokeDragDrop(raw_hwnd(self.hwnd)) };
    }
}

/// Creates the in-process shell drop-target helper that paints the drag
/// image. Without it drags still work, just without the image.
fn drag_image_helper() -> Option<IDropTargetHelper> {
    // SAFETY: creates an in-process COM object; the returned interface
    // releases it when dropped.
    unsafe {
        CoCreateInstance::<_, IDropTargetHelper>(&CLSID_DragDropHelper, None, CLSCTX_INPROC_SERVER)
    }
    .ok()
}

/// Builds the COM drop target for `hwnd` without registering it, painting
/// the drag image through `helper` when there is one.
fn make_target(
    hwnd: HWND,
    sink: Rc<dyn TargetSink>,
    helper: Option<IDropTargetHelper>,
) -> IDropTarget {
    DropTarget {
        hwnd,
        sink,
        helper,
        data: RefCell::new(None),
    }
    .into()
}

/// Registers `sink` as the drop target of `hwnd`.
pub(crate) fn register_target(hwnd: Hwnd, sink: Rc<dyn TargetSink>) -> Result<Registration> {
    ole::ensure()?;
    let target = make_target(raw_hwnd(hwnd), sink, drag_image_helper());
    // SAFETY: `hwnd` is a live window of this thread; OLE takes its own
    // reference to `target`, released by `RevokeDragDrop`.
    unsafe { RegisterDragDrop(raw_hwnd(hwnd), &target) }.map_err(win32_error)?;
    Ok(Registration { hwnd })
}

#[cfg(test)]
mod tests;
