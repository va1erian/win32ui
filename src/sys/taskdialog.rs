//! Task dialogs (Common Controls v6).
//!
//! `TaskDialogIndirect` is resolved dynamically so the crate still loads on a
//! system whose `comctl32.dll` is the v5 one: calling it then returns a clear
//! [`Error::TaskDialogUnavailable`] instead of failing at process start. All
//! `unsafe` lives here; the safe builder is `controls::taskdialog`.

use core::mem::size_of;

use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::UI::Controls::{
    TASKDIALOG_BUTTON, TASKDIALOG_COMMON_BUTTON_FLAGS, TASKDIALOGCONFIG, TASKDIALOGCONFIG_0,
    TASKDIALOGCONFIG_1, TD_ERROR_ICON, TD_INFORMATION_ICON, TD_SHIELD_ICON, TD_WARNING_ICON,
    TDF_ALLOW_DIALOG_CANCELLATION, TDF_POSITION_RELATIVE_TO_WINDOW, TDF_SIZE_TO_CONTENT,
};
use windows::core::{BOOL, HRESULT, PCWSTR, s, w};

use crate::controls::taskdialog::TaskDialogIcon;
use crate::error::{Error, Result};
use crate::hwnd::Hwnd;

use super::raw_hwnd;

/// The documented signature of `TaskDialogIndirect`.
type TaskDialogIndirectFn =
    unsafe extern "system" fn(*const TASKDIALOGCONFIG, *mut i32, *mut i32, *mut BOOL) -> HRESULT;

/// One task dialog to show, in win32ui terms (no `windows` types).
pub(crate) struct Request<'a> {
    /// The owner window; the dialog is modal to it.
    pub owner: Hwnd,
    /// The main instruction (shown in bold).
    pub title: &'a str,
    /// The optional body text.
    pub content: Option<&'a str>,
    /// The buttons, in order; ids are `1..=len`.
    pub buttons: Vec<&'a str>,
    /// The index of the default button.
    pub default_index: usize,
    /// The standard icon.
    pub icon: TaskDialogIcon,
    /// The optional verification-checkbox label.
    pub verification: Option<&'a str>,
}

/// What the dialog returned.
pub(crate) struct Outcome {
    /// The chosen button id (`IDCANCEL` when dismissed).
    pub button_id: i32,
    /// Whether the verification checkbox was checked when dismissed.
    pub checked: bool,
}

/// Shows `request` modally and reports the chosen button.
pub(crate) fn show(request: &Request<'_>) -> Result<Outcome> {
    let task_dialog = resolve()?;

    let title = to_wide(request.title);
    let content = request.content.map(to_wide);
    let verification = request.verification.map(to_wide);
    let labels: Vec<Vec<u16>> = request.buttons.iter().map(|label| to_wide(label)).collect();
    let (config, _buttons) = make_config(
        request,
        &title,
        content.as_deref(),
        verification.as_deref(),
        &labels,
    );

    let mut button_id: i32 = 0;
    let mut checked = BOOL(0);
    let verification_ptr = if verification.is_some() {
        &mut checked as *mut BOOL
    } else {
        core::ptr::null_mut()
    };
    // SAFETY: `config` and the wide buffers it points into stay alive for the
    // call; `button_id`/`checked` are valid out-pointers (the verification
    // pointer is null exactly when there is no checkbox, as the API expects).
    let result = unsafe {
        task_dialog(
            &config,
            &mut button_id,
            core::ptr::null_mut(),
            verification_ptr,
        )
    };
    result.ok().map_err(super::win32_error)?;
    Ok(Outcome {
        button_id,
        checked: checked.as_bool(),
    })
}

/// Resolves `TaskDialogIndirect` from the loaded `comctl32.dll`, which only
/// exports it at version 6 (i.e. when the manifest requests it).
fn resolve() -> Result<TaskDialogIndirectFn> {
    super::control::init_common_controls()?;
    // SAFETY: the module was just initialised above and the name is a static
    // nul-terminated wide string.
    let module = unsafe { GetModuleHandleW(w!("comctl32.dll")) }
        .map_err(|_| Error::TaskDialogUnavailable)?;
    // SAFETY: `module` is live and the name is a static nul-terminated ANSI
    // string; the call only looks the symbol up.
    let Some(proc) = (unsafe { GetProcAddress(module, s!("TaskDialogIndirect")) }) else {
        return Err(Error::TaskDialogUnavailable);
    };
    // SAFETY: `proc` is the address of the exported `TaskDialogIndirect`, whose
    // documented signature is exactly `TaskDialogIndirectFn`; both are
    // `extern "system"` function pointers.
    Ok(unsafe {
        core::mem::transmute::<unsafe extern "system" fn() -> isize, TaskDialogIndirectFn>(proc)
    })
}

/// Nul-terminated UTF-16 for a Win32 string parameter.
fn to_wide(value: &str) -> Vec<u16> {
    let mut wide: Vec<u16> = value.encode_utf16().collect();
    wide.push(0);
    wide
}

/// Maps `request` and its live wide buffers to a raw `TASKDIALOGCONFIG`.
///
/// The returned button array must outlive `config` (it holds `config.pButtons`).
fn make_config(
    request: &Request<'_>,
    title: &[u16],
    content: Option<&[u16]>,
    verification: Option<&[u16]>,
    labels: &[Vec<u16>],
) -> (TASKDIALOGCONFIG, Vec<TASKDIALOG_BUTTON>) {
    let buttons: Vec<TASKDIALOG_BUTTON> = labels
        .iter()
        .enumerate()
        .map(|(index, label)| TASKDIALOG_BUTTON {
            nButtonID: index as i32 + 1,
            pszButtonText: PCWSTR(label.as_ptr()),
        })
        .collect();
    let default_button = if request.buttons.is_empty() {
        0
    } else {
        request.default_index as i32 + 1
    };
    let config = TASKDIALOGCONFIG {
        cbSize: size_of::<TASKDIALOGCONFIG>() as u32,
        hwndParent: raw_hwnd(request.owner),
        hInstance: Default::default(),
        dwFlags: TDF_ALLOW_DIALOG_CANCELLATION
            | TDF_POSITION_RELATIVE_TO_WINDOW
            | TDF_SIZE_TO_CONTENT,
        dwCommonButtons: TASKDIALOG_COMMON_BUTTON_FLAGS(0),
        pszWindowTitle: PCWSTR::null(),
        Anonymous1: TASKDIALOGCONFIG_0 {
            pszMainIcon: icon_resource(request.icon),
        },
        pszMainInstruction: PCWSTR(title.as_ptr()),
        pszContent: content.map_or(PCWSTR::null(), |value| PCWSTR(value.as_ptr())),
        cButtons: buttons.len() as u32,
        pButtons: buttons.as_ptr(),
        nDefaultButton: default_button,
        cRadioButtons: 0,
        pRadioButtons: core::ptr::null(),
        nDefaultRadioButton: 0,
        pszVerificationText: verification.map_or(PCWSTR::null(), |value| PCWSTR(value.as_ptr())),
        pszExpandedInformation: PCWSTR::null(),
        pszExpandedControlText: PCWSTR::null(),
        pszCollapsedControlText: PCWSTR::null(),
        Anonymous2: TASKDIALOGCONFIG_1 {
            pszFooterIcon: PCWSTR::null(),
        },
        pszFooter: PCWSTR::null(),
        pfCallback: None,
        lpCallbackData: 0,
        cxWidth: 0,
    };
    (config, buttons)
}

/// The `TD_*` resource id for a system icon (`None` means no icon).
fn icon_resource(icon: TaskDialogIcon) -> PCWSTR {
    match icon {
        TaskDialogIcon::None => PCWSTR::null(),
        TaskDialogIcon::Information => TD_INFORMATION_ICON,
        TaskDialogIcon::Warning => TD_WARNING_ICON,
        TaskDialogIcon::Error => TD_ERROR_ICON,
        TaskDialogIcon::Shield => TD_SHIELD_ICON,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> Request<'static> {
        Request {
            owner: Hwnd::from_raw(0x1234),
            title: "Delete 3 messages?",
            content: Some("They will be moved to Trash."),
            buttons: vec!["Delete", "Cancel"],
            default_index: 1,
            icon: TaskDialogIcon::Warning,
            verification: Some("Don't ask again"),
        }
    }

    #[test]
    fn builder_maps_to_raw_config() {
        let request = request();
        let title = to_wide(request.title);
        let content = request.content.map(to_wide);
        let verification = request.verification.map(to_wide);
        let labels: Vec<Vec<u16>> = request.buttons.iter().map(|label| to_wide(label)).collect();
        let (config, buttons) = make_config(
            &request,
            &title,
            content.as_deref(),
            verification.as_deref(),
            &labels,
        );

        // `TASKDIALOGCONFIG`/`TASKDIALOG_BUTTON` are packed, so copy each field
        // to a local before comparing (never take a reference to one).
        let cb_size = config.cbSize;
        let flags = config.dwFlags;
        let c_buttons = config.cButtons;
        let default_button = config.nDefaultButton;
        let verification_text = config.pszVerificationText;
        let first_id = buttons[0].nButtonID;
        let second_id = buttons[1].nButtonID;
        // SAFETY: the union is the main-icon arm; this only reads it.
        let main_icon = unsafe { config.Anonymous1.pszMainIcon };

        assert_eq!(cb_size, size_of::<TASKDIALOGCONFIG>() as u32);
        assert!(flags.contains(TDF_ALLOW_DIALOG_CANCELLATION));
        assert!(flags.contains(TDF_POSITION_RELATIVE_TO_WINDOW));
        assert!(flags.contains(TDF_SIZE_TO_CONTENT));
        assert_eq!(c_buttons, 2);
        assert_eq!(default_button, 2);
        assert_eq!(buttons.len(), 2);
        assert_eq!(first_id, 1);
        assert_eq!(second_id, 2);
        assert_eq!(main_icon, TD_WARNING_ICON);
        assert_eq!(
            verification_text,
            PCWSTR(verification.as_ref().expect("verification").as_ptr())
        );
    }

    #[test]
    fn icon_resource_maps_each_variant() {
        assert_eq!(icon_resource(TaskDialogIcon::None), PCWSTR::null());
        assert_eq!(
            icon_resource(TaskDialogIcon::Information),
            TD_INFORMATION_ICON
        );
        assert_eq!(icon_resource(TaskDialogIcon::Warning), TD_WARNING_ICON);
        assert_eq!(icon_resource(TaskDialogIcon::Error), TD_ERROR_ICON);
        assert_eq!(icon_resource(TaskDialogIcon::Shield), TD_SHIELD_ICON);
    }

    #[test]
    fn buttons_are_numbered_from_one() {
        let request = request();
        let labels: Vec<Vec<u16>> = request.buttons.iter().map(|label| to_wide(label)).collect();
        let (config, buttons) = make_config(&request, &[0u16], None, None, &labels);
        let default_button = config.nDefaultButton;
        let ids: Vec<i32> = buttons.iter().map(|button| button.nButtonID).collect();
        assert_eq!(default_button, 2);
        assert_eq!(ids, vec![1, 2]);
    }
}
