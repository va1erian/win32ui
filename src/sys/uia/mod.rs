#![allow(non_upper_case_globals)] // the SDK spells UI Automation ids like `UIA_NamePropertyId`

//! UI Automation providers for windows that describe themselves through an
//! accessibility [`Source`](crate::accessibility::registry::Source).
//!
//! The shared window procedure calls [`get_object`] for `WM_GETOBJECT`; a
//! window with a registered source answers with a root provider, and UI
//! Automation walks the rest of the tree through the fragment interfaces.

mod native;
mod props;
mod provider;
mod runtime_id;
mod target;

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Accessibility::{IRawElementProviderSimple, UiaReturnRawElementProvider};

use crate::accessibility::registry;
use crate::hwnd::Hwnd;
pub(crate) use native::{
    attach as attach_native, attach_source, attach_state as attach_native_state,
};
use provider::RootElement;
use target::Target;

/// `UiaRootObjectId`: the `WM_GETOBJECT` object id asking for the window's
/// UI Automation provider.
const UIA_ROOT_OBJECT_ID: i32 = -25;

/// Answers `WM_GETOBJECT` for `hwnd`. Returns the `LRESULT` to hand back, or
/// `None` when the window has no accessibility source (or the request is for
/// a different object id), so the default handling proceeds.
pub(crate) fn get_object(hwnd: Hwnd, wparam: usize, lparam: isize) -> Option<isize> {
    if lparam as i32 != UIA_ROOT_OBJECT_ID {
        return None;
    }
    // A window whose widget has nothing to say keeps the default handling.
    registry::source(hwnd)?.snapshot()?;
    let root: IRawElementProviderSimple = RootElement::new(Target::root(hwnd)).into();
    // SAFETY: `root` is a live provider; UI Automation takes its own reference
    // and the handle is the window that received the message.
    let result = unsafe {
        UiaReturnRawElementProvider(
            HWND(hwnd.raw() as *mut _),
            WPARAM(wparam),
            LPARAM(lparam),
            &root,
        )
    };
    Some(result.0)
}
