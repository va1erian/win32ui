//! Property values a provider reports, as `VARIANT`s.

use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::*;
use windows::core::BSTR;

use crate::accessibility::Role;

use super::target::{Target, control_type};

/// The value of `property` for `target`, or `None` when it is not reported
/// (UI Automation then falls back to the window's own defaults).
pub(super) fn property(target: &Target, property: UIA_PROPERTY_ID) -> Option<VARIANT> {
    let node = target.node()?;
    // A window's own name and type belong to Windows (the host provider), and
    // an unnamed root leaves its name to the host as well.
    let host_owned = node.role == Role::Window && target.is_root();
    match property {
        UIA_NamePropertyId if host_owned || (node.name.is_empty() && target.is_root()) => None,
        UIA_NamePropertyId => Some(VARIANT::from(BSTR::from(node.name))),
        UIA_ControlTypePropertyId if host_owned => None,
        UIA_ControlTypePropertyId => Some(VARIANT::from(control_type(node.role))),
        UIA_IsControlElementPropertyId | UIA_IsContentElementPropertyId => {
            Some(VARIANT::from(true))
        }
        UIA_IsEnabledPropertyId => Some(VARIANT::from(node.enabled)),
        UIA_IsKeyboardFocusablePropertyId => Some(VARIANT::from(node.focusable)),
        UIA_HasKeyboardFocusPropertyId => Some(VARIANT::from(node.focused)),
        UIA_AutomationIdPropertyId => node.id.map(|id| VARIANT::from(BSTR::from(id))),
        UIA_HelpTextPropertyId => node.help.map(|help| VARIANT::from(BSTR::from(help))),
        UIA_IsOffscreenPropertyId => Some(VARIANT::from(false)),
        UIA_FrameworkIdPropertyId => Some(VARIANT::from(BSTR::from("win32ui"))),
        _ => None,
    }
}
