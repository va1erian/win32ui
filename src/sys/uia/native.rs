//! The accessibility source of a native `BUTTON`-class control that win32ui
//! owner-draws (push buttons, radio options, group boxes). Windows' own proxy
//! cannot tell what an owner-drawn button is, so the control answers UI
//! Automation itself: role from how win32ui created it, name from the window
//! text, state from the button's check state.

use std::rc::Rc;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Shell::DefSubclassProc;
use windows::Win32::UI::WindowsAndMessaging::{WM_GETOBJECT, WM_NCDESTROY};

use crate::accessibility::registry::{self, Source};
use crate::accessibility::{Action, Node, Role};
use crate::hwnd::Hwnd;
use crate::sys;

/// Subclass id ("uia1"): distinct from every other subclass on the control.
const SUBCLASS_ID: usize = 0x7561_6931;

/// A native button the control layer describes with a fixed role.
struct NativeSource {
    hwnd: Hwnd,
    role: Role,
    /// Reads the control's own state when the window cannot report it (an
    /// owner-drawn radio keeps its selection in the control, not in the
    /// button's check state).
    state: Option<Rc<dyn Fn() -> bool>>,
}

impl NativeSource {
    /// The combo box: its selected text as the value, each choice a child.
    fn combo(&self) -> Node {
        let selected = sys::combobox::cb_get_cur_sel(self.hwnd);
        let items = (0..sys::combobox::cb_count(self.hwnd)).map(|index| {
            Node::new(
                Role::ListItem,
                sys::combobox::cb_item_text(self.hwnd, index),
            )
            .selected(selected == Some(index))
        });
        let value = selected.map_or_else(String::new, |index| {
            sys::combobox::cb_item_text(self.hwnd, index)
        });
        Node::new(Role::ComboBox, "").value(value).children(items)
    }

    fn checked(&self) -> bool {
        self.state
            .as_ref()
            .map_or_else(|| sys::button::is_checked(self.hwnd), |state| state())
    }
}

impl Source for NativeSource {
    fn snapshot(&self) -> Option<Node> {
        if !self.hwnd.is_alive() {
            return None;
        }
        let text = sys::window::get_title(self.hwnd);
        let enabled = sys::window_input::is_enabled(self.hwnd);
        let focused = sys::window_input::has_focus(self.hwnd);
        Some(
            match self.role {
                Role::Button => Node::new(self.role, text).invokable(),
                Role::RadioButton => Node::new(self.role, text).selected(self.checked()),
                Role::CheckBox => Node::new(self.role, text).checked(self.checked()),
                // An edit's text is its value; its name is the cue banner, the
                // hint shown while it is empty.
                Role::Edit => Node::new(self.role, sys::edit::cue(self.hwnd))
                    .value(sys::edit::text(self.hwnd)),
                Role::ComboBox => self.combo(),
                _ => Node::new(self.role, text),
            }
            .enabled(enabled)
            .focusable(focused && self.role != Role::Text),
        )
    }

    fn perform(&self, path: &[usize], action: Action) -> bool {
        if !self.hwnd.is_alive() {
            return false;
        }
        if let [choice] = path {
            if self.role != Role::ComboBox || *choice >= sys::combobox::cb_count(self.hwnd) {
                return false;
            }
            if action != Action::Select && action != Action::Invoke {
                return false;
            }
            sys::combobox::cb_set_cur_sel(self.hwnd, Some(*choice));
            sys::combobox::notify_sel_change(self.hwnd);
            return true;
        }
        if !path.is_empty() {
            return false;
        }
        match action {
            Action::SetText(text) if self.role == Role::Edit => {
                sys::edit::set_text(self.hwnd, &text);
                true
            }
            Action::Invoke | Action::Select | Action::Toggle => {
                sys::button::click(self.hwnd);
                true
            }
            Action::Focus => {
                sys::window_input::focus(self.hwnd);
                true
            }
            _ => false,
        }
    }
}

/// Makes the native control `hwnd` answer UI Automation as `role`.
pub(crate) fn attach(hwnd: Hwnd, role: Role) {
    attach_source(
        hwnd,
        Rc::new(NativeSource {
            hwnd,
            role,
            state: None,
        }),
    );
}

/// Makes the native control `hwnd` answer UI Automation from `source`.
pub(crate) fn attach_source(hwnd: Hwnd, source: Rc<dyn Source>) {
    registry::register(hwnd, source);
    sys::window::set_subclass(hwnd, Some(native_proc), SUBCLASS_ID, 0);
}

/// Replaces the source of a control already [`attach`]ed with one that reads
/// its checked/selected state from `state`.
pub(crate) fn attach_state(hwnd: Hwnd, role: Role, state: Rc<dyn Fn() -> bool>) {
    registry::register(
        hwnd,
        Rc::new(NativeSource {
            hwnd,
            role,
            state: Some(state),
        }),
    );
}

/// The subclass procedure that answers the UI Automation root request.
///
/// # Safety
/// Called by Windows for a control subclassed by [`attach`].
unsafe extern "system" fn native_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _refdata: usize,
) -> LRESULT {
    if msg == WM_GETOBJECT
        && let Some(result) = super::get_object(sys::hwnd_from(hwnd), wparam.0, lparam.0)
    {
        return LRESULT(result);
    }
    if msg == WM_NCDESTROY {
        registry::forget(sys::hwnd_from(hwnd));
        sys::window::remove_subclass(sys::hwnd_from(hwnd), Some(native_proc), SUBCLASS_ID);
    }
    // SAFETY: forward to the subclass chain's original window procedure.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}
