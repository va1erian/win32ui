//! The runtime id UI Automation uses to tell fragments apart: a constant
//! prefix, the window handle, then the node's path (shifted by one so index 0
//! is distinct from "no index").

use windows::Win32::System::Com::SAFEARRAY;
use windows::Win32::System::Ole::{SafeArrayCreateVector, SafeArrayPutElement};
use windows::Win32::System::Variant::VT_I4;
use windows::core::{Error, Result};

use super::target::Target;

/// `UiaAppendRuntimeId` from `UIAutomationCore.h`: the first element of a
/// runtime id built by a provider (the SDK header defines it as 3).
const APPEND_RUNTIME_ID: i32 = 3;

/// Builds the runtime-id `SAFEARRAY` for `target`. The caller (UI Automation)
/// owns and frees the returned array.
pub(super) fn array(target: &Target) -> Result<*mut SAFEARRAY> {
    let mut ids: Vec<i32> = vec![APPEND_RUNTIME_ID, target.hwnd.raw() as i32];
    ids.extend(target.path.iter().map(|index| *index as i32 + 1));

    // SAFETY: creates a one-dimensional array of `ids.len()` 32-bit integers.
    let array = unsafe { SafeArrayCreateVector(VT_I4, 0, ids.len() as u32) };
    if array.is_null() {
        return Err(Error::from(windows::Win32::Foundation::E_OUTOFMEMORY));
    }
    for (index, id) in ids.iter().enumerate() {
        let index = index as i32;
        // SAFETY: `index` is within the bounds the array was created with and
        // `id` points at a live `i32`, the element type.
        unsafe { SafeArrayPutElement(array, &index, (id as *const i32).cast()) }?;
    }
    Ok(array)
}
