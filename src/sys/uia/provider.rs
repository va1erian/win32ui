//! The COM providers UI Automation talks to. [`Element`] is any node of a
//! window's accessibility tree; [`RootElement`] is the same plus the fragment
//! root interface, and is the only one the window hands to UI Automation.
//!
//! Both implement the same interfaces through one macro, so the two can never
//! drift apart. Everything is answered from a fresh snapshot (see
//! [`Target`]), and calls arrive on the window's own thread because the
//! provider options request COM threading.

use windows::Win32::Foundation::{E_FAIL, HWND};
use windows::Win32::System::Com::SAFEARRAY;
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::*;
use windows::core::{
    BOOL, BSTR, Error, IUnknown, IUnknownImpl, Interface, PCWSTR, Result, implement,
};

use super::props;
use super::runtime_id;
use super::target::Target;
use crate::accessibility::Action;

/// `UIA_E_ELEMENTNOTAVAILABLE`: the element no longer exists.
fn not_available() -> Error {
    Error::from_hresult(windows::core::HRESULT(0x8004_0201_u32 as i32))
}
/// `UIA_E_NOTSUPPORTED`: the element does not do that.
fn not_supported() -> Error {
    Error::from_hresult(windows::core::HRESULT(0x8004_0204_u32 as i32))
}

/// A non-root node of a window's tree.
#[implement(
    IRawElementProviderSimple,
    IRawElementProviderFragment,
    IInvokeProvider,
    IToggleProvider,
    IValueProvider,
    IRangeValueProvider,
    ISelectionItemProvider
)]
pub(super) struct Element {
    target: Target,
}

/// The root node of a window's tree: an [`Element`] that is also the
/// fragment root.
#[implement(
    IRawElementProviderSimple,
    IRawElementProviderFragment,
    IRawElementProviderFragmentRoot,
    IInvokeProvider,
    IToggleProvider,
    IValueProvider,
    IRangeValueProvider,
    ISelectionItemProvider
)]
pub(super) struct RootElement {
    target: Target,
}

impl RootElement {
    pub(super) fn new(target: Target) -> RootElement {
        RootElement { target }
    }
}

/// The provider for `target`: the root for an empty path, else an [`Element`].
fn fragment(target: Target) -> IRawElementProviderFragment {
    if target.is_root() {
        RootElement::new(target).into()
    } else {
        Element { target }.into()
    }
}

/// A `null` result: `S_OK` with no object, which is how a provider says
/// "there is nothing there".
fn nothing<T>() -> Result<T> {
    Err(Error::empty())
}

fn navigate(target: &Target, direction: NavigateDirection) -> Result<IRawElementProviderFragment> {
    let node = target.node().ok_or_else(not_available)?;
    let found = match direction {
        NavigateDirection_Parent => target.parent(),
        NavigateDirection_FirstChild => (!node.children.is_empty()).then(|| target.child(0)),
        NavigateDirection_LastChild => node
            .children
            .len()
            .checked_sub(1)
            .map(|last| target.child(last)),
        NavigateDirection_NextSibling => sibling(target, 1),
        NavigateDirection_PreviousSibling => sibling(target, -1),
        _ => None,
    };
    found.map(fragment).map_or_else(nothing, Ok)
}

fn sibling(target: &Target, step: isize) -> Option<Target> {
    let (index, count) = target.sibling_context()?;
    let next = index.checked_add_signed(step)?;
    (next < count).then(|| target.parent().map(|parent| parent.child(next)))?
}

/// Runs `action`, mapping a refusal to the error UI Automation expects.
fn act(target: &Target, action: Action) -> Result<()> {
    let node = target.node().ok_or_else(not_available)?;
    if !node.enabled {
        return Err(Error::from_hresult(windows::core::HRESULT(
            0x8004_0200_u32 as i32,
        )));
    }
    if target.perform(action) {
        Ok(())
    } else {
        Err(not_supported())
    }
}

macro_rules! element_impls {
    ($imp:ident) => {
        impl IRawElementProviderSimple_Impl for $imp {
            fn ProviderOptions(&self) -> Result<ProviderOptions> {
                Ok(ProviderOptions(
                    ProviderOptions_ServerSideProvider.0 | ProviderOptions_UseComThreading.0,
                ))
            }

            fn GetPatternProvider(&self, pattern: UIA_PATTERN_ID) -> Result<IUnknown> {
                let node = self.target.node().ok_or_else(not_available)?;
                match pattern {
                    UIA_InvokePatternId if node.invokable => {
                        Ok(self.to_interface::<IInvokeProvider>().cast()?)
                    }
                    UIA_TogglePatternId if node.checked.is_some() => {
                        Ok(self.to_interface::<IToggleProvider>().cast()?)
                    }
                    UIA_ValuePatternId if node.value.is_some() && node.range.is_none() => {
                        Ok(self.to_interface::<IValueProvider>().cast()?)
                    }
                    UIA_RangeValuePatternId if node.range.is_some() => {
                        Ok(self.to_interface::<IRangeValueProvider>().cast()?)
                    }
                    UIA_SelectionItemPatternId if node.selected.is_some() => {
                        Ok(self.to_interface::<ISelectionItemProvider>().cast()?)
                    }
                    _ => nothing(),
                }
            }

            fn GetPropertyValue(&self, property: UIA_PROPERTY_ID) -> Result<VARIANT> {
                props::property(&self.target, property).map_or_else(|| Ok(VARIANT::default()), Ok)
            }

            fn HostRawElementProvider(&self) -> Result<IRawElementProviderSimple> {
                if self.target.is_root() {
                    // SAFETY: `hwnd` is the live window this provider serves.
                    unsafe { UiaHostProviderFromHwnd(HWND(self.target.hwnd.raw() as *mut _)) }
                } else {
                    nothing()
                }
            }
        }

        impl IRawElementProviderFragment_Impl for $imp {
            fn Navigate(
                &self,
                direction: NavigateDirection,
            ) -> Result<IRawElementProviderFragment> {
                navigate(&self.target, direction)
            }

            fn GetRuntimeId(&self) -> Result<*mut SAFEARRAY> {
                if self.target.is_root() {
                    return nothing();
                }
                runtime_id::array(&self.target)
            }

            fn BoundingRectangle(&self) -> Result<UiaRect> {
                let bounds = self.target.screen_bounds().ok_or_else(not_available)?;
                Ok(UiaRect {
                    left: f64::from(bounds.left),
                    top: f64::from(bounds.top),
                    width: f64::from(bounds.width()),
                    height: f64::from(bounds.height()),
                })
            }

            fn GetEmbeddedFragmentRoots(&self) -> Result<*mut SAFEARRAY> {
                nothing()
            }

            fn SetFocus(&self) -> Result<()> {
                act(&self.target, Action::Focus)
            }

            fn FragmentRoot(&self) -> Result<IRawElementProviderFragmentRoot> {
                Ok(RootElement::new(Target::root(self.target.hwnd)).into())
            }
        }

        impl IInvokeProvider_Impl for $imp {
            fn Invoke(&self) -> Result<()> {
                act(&self.target, Action::Invoke)
            }
        }

        impl IToggleProvider_Impl for $imp {
            fn Toggle(&self) -> Result<()> {
                act(&self.target, Action::Toggle)
            }

            fn ToggleState(&self) -> Result<ToggleState> {
                let node = self.target.node().ok_or_else(not_available)?;
                Ok(match node.checked {
                    Some(true) => ToggleState_On,
                    _ => ToggleState_Off,
                })
            }
        }

        impl IValueProvider_Impl for $imp {
            fn SetValue(&self, value: &PCWSTR) -> Result<()> {
                // SAFETY: UI Automation passes a NUL-terminated string.
                let text = unsafe { value.to_string() }.map_err(|_| Error::from(E_FAIL))?;
                act(&self.target, Action::SetText(text))
            }

            fn Value(&self) -> Result<BSTR> {
                let node = self.target.node().ok_or_else(not_available)?;
                Ok(BSTR::from(node.value.unwrap_or_default()))
            }

            fn IsReadOnly(&self) -> Result<BOOL> {
                let node = self.target.node().ok_or_else(not_available)?;
                Ok(BOOL::from(node.role != crate::accessibility::Role::Edit))
            }
        }

        impl IRangeValueProvider_Impl for $imp {
            fn SetValue(&self, value: f64) -> Result<()> {
                act(&self.target, Action::SetRange(value))
            }

            fn Value(&self) -> Result<f64> {
                Ok(self.range()?.value)
            }

            fn IsReadOnly(&self) -> Result<BOOL> {
                Ok(BOOL::from(!self.range()?.settable))
            }

            fn Maximum(&self) -> Result<f64> {
                Ok(self.range()?.max)
            }

            fn Minimum(&self) -> Result<f64> {
                Ok(self.range()?.min)
            }

            fn LargeChange(&self) -> Result<f64> {
                let range = self.range()?;
                Ok((range.max - range.min) / 10.0)
            }

            fn SmallChange(&self) -> Result<f64> {
                Ok(self.range()?.step)
            }
        }

        impl ISelectionItemProvider_Impl for $imp {
            fn Select(&self) -> Result<()> {
                act(&self.target, Action::Select)
            }

            fn AddToSelection(&self) -> Result<()> {
                act(&self.target, Action::Select)
            }

            fn RemoveFromSelection(&self) -> Result<()> {
                Err(not_supported())
            }

            fn IsSelected(&self) -> Result<BOOL> {
                let node = self.target.node().ok_or_else(not_available)?;
                Ok(BOOL::from(node.selected == Some(true)))
            }

            fn SelectionContainer(&self) -> Result<IRawElementProviderSimple> {
                match self.target.parent() {
                    Some(parent) => Ok(fragment(parent).cast()?),
                    None => nothing(),
                }
            }
        }

        impl $imp {
            fn range(&self) -> Result<crate::accessibility::RangeValue> {
                self.target
                    .node()
                    .and_then(|node| node.range)
                    .ok_or_else(not_available)
            }
        }
    };
}

element_impls!(Element_Impl);
element_impls!(RootElement_Impl);

impl IRawElementProviderFragmentRoot_Impl for RootElement_Impl {
    fn ElementProviderFromPoint(&self, x: f64, y: f64) -> Result<IRawElementProviderFragment> {
        match self.target.hit_test(x as i32, y as i32) {
            Some(target) => Ok(fragment(target)),
            None => nothing(),
        }
    }

    fn GetFocus(&self) -> Result<IRawElementProviderFragment> {
        match self.target.focused() {
            Some(target) => Ok(fragment(target)),
            None => nothing(),
        }
    }
}
