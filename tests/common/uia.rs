//! A tiny UI Automation *client*, used by the accessibility tests to inspect
//! and drive what a win32ui window exposes to assistive technology.
//!
//! UI Automation calls back into the provider on the window's UI thread, so
//! the client must run on another thread while the test's message loop keeps
//! pumping. Start it with [`spawn_client`] from inside the test's `make`
//! closure; it finds the window by title, so the window must already exist.

#![allow(dead_code)]

use std::thread::JoinHandle;
use std::time::Duration;

use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationInvokePattern,
    IUIAutomationRangeValuePattern, IUIAutomationSelectionItemPattern, IUIAutomationTogglePattern,
    IUIAutomationTreeWalker, IUIAutomationValuePattern, UIA_InvokePatternId,
    UIA_RangeValuePatternId, UIA_SelectionItemPatternId, UIA_TogglePatternId, UIA_ValuePatternId,
};
use windows::Win32::UI::WindowsAndMessaging::FindWindowW;
use windows::core::{BSTR, Interface, PCWSTR};

/// One element of the UIA tree, flattened in document order.
#[derive(Clone, Debug)]
pub struct UiaNode {
    pub depth: usize,
    pub name: String,
    pub control_type: i32,
    pub class_name: String,
    pub automation_id: String,
}

/// A connected client: the automation object and the window's root element.
pub struct Client {
    automation: IUIAutomation,
    pub root: IUIAutomationElement,
}

/// Runs `f` on a fresh client thread once the window titled `title` exists,
/// then calls `done` (which should make the app quit, for example through a
/// [`Proxy`](win32ui::Proxy)). Returns the thread's result.
pub fn spawn_client<T: Send + 'static>(
    title: &'static str,
    done: impl FnOnce() + Send + 'static,
    f: impl FnOnce(&Client) -> T + Send + 'static,
) -> JoinHandle<Option<T>> {
    std::thread::spawn(move || {
        // SAFETY: plain COM initialisation on this dedicated client thread.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        // The window may not be up yet on a loaded machine: retry for a few
        // seconds instead of trusting one fixed sleep.
        let mut client = None;
        for _ in 0..50 {
            std::thread::sleep(Duration::from_millis(100));
            client = Client::connect(title);
            if client.is_some() {
                break;
            }
        }
        let result = client.map(|client| f(&client));
        // SAFETY: balances the initialisation above.
        unsafe { CoUninitialize() };
        done();
        result
    })
}

impl Client {
    fn connect(title: &str) -> Option<Client> {
        let wide: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
        // SAFETY: `wide` is NUL-terminated and outlives the call.
        let hwnd = unsafe { FindWindowW(PCWSTR::null(), PCWSTR(wide.as_ptr())) }.ok()?;
        // SAFETY: standard in-proc COM creation of the UIA client object.
        let automation: IUIAutomation =
            unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }.ok()?;
        // SAFETY: `hwnd` is a live top-level window found above.
        let root = unsafe { automation.ElementFromHandle(hwnd) }.ok()?;
        Some(Client { automation, root })
    }

    /// The whole subtree, flattened.
    pub fn dump(&self) -> Vec<UiaNode> {
        let mut out = Vec::new();
        // SAFETY: walks the control view with the client's own tree walker.
        unsafe {
            if let Ok(walker) = self.automation.ControlViewWalker() {
                walk(&walker, &self.root, 0, &mut out);
            }
        }
        out
    }

    /// The first element in the subtree with this exact name.
    pub fn find(&self, name: &str) -> Option<IUIAutomationElement> {
        let mut found = None;
        // SAFETY: walks the control view with the client's own tree walker.
        unsafe {
            let walker = self.automation.ControlViewWalker().ok()?;
            find_in(&walker, &self.root, name, &mut found);
        }
        found
    }
}

unsafe fn find_in(
    walker: &IUIAutomationTreeWalker,
    element: &IUIAutomationElement,
    name: &str,
    found: &mut Option<IUIAutomationElement>,
) {
    // SAFETY: `element` is a live UIA element for the duration of the call.
    unsafe {
        if found.is_some() {
            return;
        }
        if element.CurrentName().map(|n| n.to_string()).as_deref() == Ok(name) {
            *found = Some(element.clone());
            return;
        }
        let mut child = walker.GetFirstChildElement(element).ok();
        while let Some(current) = child {
            find_in(walker, &current, name, found);
            child = walker.GetNextSiblingElement(&current).ok();
        }
    }
}

unsafe fn walk(
    walker: &IUIAutomationTreeWalker,
    element: &IUIAutomationElement,
    depth: usize,
    out: &mut Vec<UiaNode>,
) {
    // SAFETY: `element` is a live UIA element for the duration of the call.
    unsafe {
        out.push(UiaNode {
            depth,
            name: element
                .CurrentName()
                .unwrap_or_else(|_| BSTR::new())
                .to_string(),
            control_type: element.CurrentControlType().map(|t| t.0).unwrap_or(0),
            class_name: element
                .CurrentClassName()
                .unwrap_or_else(|_| BSTR::new())
                .to_string(),
            automation_id: element
                .CurrentAutomationId()
                .unwrap_or_else(|_| BSTR::new())
                .to_string(),
        });
        let mut child = walker.GetFirstChildElement(element).ok();
        while let Some(current) = child {
            walk(walker, &current, depth + 1, out);
            child = walker.GetNextSiblingElement(&current).ok();
        }
    }
}

/// Invokes the element through its Invoke pattern.
pub fn invoke(element: &IUIAutomationElement) -> bool {
    // SAFETY: the element is live and the pattern id matches the cast type.
    unsafe {
        element
            .GetCurrentPattern(UIA_InvokePatternId)
            .and_then(|p| p.cast::<IUIAutomationInvokePattern>())
            .and_then(|p| p.Invoke())
            .is_ok()
    }
}

/// Toggles the element and returns its new toggle state (0 off, 1 on).
pub fn toggle(element: &IUIAutomationElement) -> Option<i32> {
    // SAFETY: the element is live and the pattern id matches the cast type.
    unsafe {
        let pattern = element
            .GetCurrentPattern(UIA_TogglePatternId)
            .ok()?
            .cast::<IUIAutomationTogglePattern>()
            .ok()?;
        pattern.Toggle().ok()?;
        pattern.CurrentToggleState().ok().map(|s| s.0)
    }
}

/// Sets a range element's value and reads it back.
pub fn set_range(element: &IUIAutomationElement, value: f64) -> Option<f64> {
    // SAFETY: the element is live and the pattern id matches the cast type.
    unsafe {
        let pattern = element
            .GetCurrentPattern(UIA_RangeValuePatternId)
            .ok()?
            .cast::<IUIAutomationRangeValuePattern>()
            .ok()?;
        pattern.SetValue(value).ok()?;
        pattern.CurrentValue().ok()
    }
}

/// Reads a range element's current value and maximum.
pub fn range(element: &IUIAutomationElement) -> Option<(f64, f64)> {
    // SAFETY: the element is live and the pattern id matches the cast type.
    unsafe {
        let pattern = element
            .GetCurrentPattern(UIA_RangeValuePatternId)
            .ok()?
            .cast::<IUIAutomationRangeValuePattern>()
            .ok()?;
        Some((pattern.CurrentValue().ok()?, pattern.CurrentMaximum().ok()?))
    }
}

/// Selects the element and returns whether it reports selected afterwards.
pub fn select(element: &IUIAutomationElement) -> Option<bool> {
    // SAFETY: the element is live and the pattern id matches the cast type.
    unsafe {
        let pattern = element
            .GetCurrentPattern(UIA_SelectionItemPatternId)
            .ok()?
            .cast::<IUIAutomationSelectionItemPattern>()
            .ok()?;
        pattern.Select().ok()?;
        pattern.CurrentIsSelected().ok().map(|b| b.as_bool())
    }
}

/// Reads the Value pattern's text.
pub fn value(element: &IUIAutomationElement) -> Option<String> {
    // SAFETY: the element is live and the pattern id matches the cast type.
    unsafe {
        let pattern = element
            .GetCurrentPattern(UIA_ValuePatternId)
            .ok()?
            .cast::<IUIAutomationValuePattern>()
            .ok()?;
        pattern.CurrentValue().ok().map(|v| v.to_string())
    }
}

/// Whether the element reports selected (Selection-item pattern).
pub fn is_selected(element: &IUIAutomationElement) -> Option<bool> {
    // SAFETY: the element is live and the pattern id matches the cast type.
    unsafe {
        let pattern = element
            .GetCurrentPattern(UIA_SelectionItemPatternId)
            .ok()?
            .cast::<IUIAutomationSelectionItemPattern>()
            .ok()?;
        pattern.CurrentIsSelected().ok().map(|b| b.as_bool())
    }
}

/// Sets the Value pattern's text.
pub fn set_value(element: &IUIAutomationElement, text: &str) -> bool {
    // SAFETY: the element is live and the pattern id matches the cast type.
    unsafe {
        element
            .GetCurrentPattern(UIA_ValuePatternId)
            .and_then(|p| p.cast::<IUIAutomationValuePattern>())
            .and_then(|p| p.SetValue(&BSTR::from(text)))
            .is_ok()
    }
}
