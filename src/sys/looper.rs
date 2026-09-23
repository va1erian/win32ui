//! The message pump and the keyboard translation that runs before dispatch:
//! per-window accelerator tables and `IsDialogMessageW` Tab navigation.
//!
//! Everything here is raw Win32 and lives in `sys`; the widget layer only
//! registers shortcuts and a "this is an app window" flag.

use std::cell::RefCell;
use std::collections::HashMap;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    ACCEL, CreateAcceleratorTableW, DestroyAcceleratorTable, DispatchMessageW, FALT, FCONTROL,
    FSHIFT, FVIRTKEY, GA_ROOT, GetAncestor, GetMessageW, HACCEL, IsDialogMessageW, MSG,
    PostQuitMessage, TranslateAcceleratorW, TranslateMessage,
};

use crate::accel::Shortcut;
use crate::error::Result;
use crate::hwnd::Hwnd;

use super::{hwnd_from, raw_hwnd, win32_error};

/// First command id handed to an accelerator, so accelerator `WM_COMMAND`s do
/// not collide with the low ids controls use for their own notifications.
const COMMAND_BASE: u16 = 0xF000;

/// The outcome of pumping one message.
pub(crate) enum Pumped {
    /// A message was retrieved (and dispatched, unless translated).
    Message,
    /// `WM_QUIT` was received; carries the exit code.
    Quit(i32),
    /// `GetMessageW` failed.
    Error,
}

/// A window's registered keyboard state: an optional accelerator table plus
/// whether `IsDialogMessageW` should give it Tab/Shift+Tab navigation.
#[derive(Default)]
struct Keyboard {
    table: Option<HACCEL>,
    dialog_nav: bool,
}

thread_local! {
    /// Per-window keyboard state, keyed by `HWND`. Windows are thread-affine,
    /// so the owning thread's map is the one that serves their messages.
    static WINDOWS: RefCell<HashMap<usize, Keyboard>> = RefCell::new(HashMap::new());
}

/// Enables Tab/Shift+Tab navigation (`IsDialogMessageW`) for the app window
/// `hwnd`.
pub(crate) fn enable_dialog_nav(hwnd: Hwnd) {
    WINDOWS.with(|windows| {
        windows
            .borrow_mut()
            .entry(hwnd.raw())
            .or_default()
            .dialog_nav = true;
    });
}

/// Builds (or replaces) `hwnd`'s accelerator table from `shortcuts`. An empty
/// slice removes the table; the dialog-navigation flag is untouched.
pub(crate) fn set_accelerators(hwnd: Hwnd, shortcuts: &[Shortcut]) -> Result<()> {
    let table = if shortcuts.is_empty() {
        None
    } else {
        Some(build_table(shortcuts)?)
    };

    let previous = WINDOWS.with(|windows| {
        let mut windows = windows.borrow_mut();
        let state = windows.entry(hwnd.raw()).or_default();
        std::mem::replace(&mut state.table, table)
    });
    destroy(previous);
    Ok(())
}

/// Forgets `hwnd`'s keyboard state and destroys its accelerator table. Called
/// when the window is destroyed.
pub(crate) fn forget_keyboard(hwnd: Hwnd) {
    let state = WINDOWS.with(|windows| windows.borrow_mut().remove(&hwnd.raw()));
    if let Some(state) = state {
        destroy(state.table);
    }
}

/// The index of the accelerator that owns `id`, if `id` is an accelerator
/// command rather than a control or menu id.
pub(crate) fn accelerator_index(id: u16) -> Option<usize> {
    id.checked_sub(COMMAND_BASE).map(usize::from)
}

/// Creates an accelerator table from `shortcuts`; the caller destroys it.
fn build_table(shortcuts: &[Shortcut]) -> Result<HACCEL> {
    let entries: Vec<ACCEL> = shortcuts
        .iter()
        .enumerate()
        .map(|(index, shortcut)| accel_of(shortcut, command_id(index)))
        .collect();
    // SAFETY: `entries` is a live slice for the duration of the call, and
    // `CreateAcceleratorTableW` copies the data it needs.
    unsafe { CreateAcceleratorTableW(&entries) }.map_err(win32_error)
}

/// One table row: the key and the Ctrl/Shift/Alt flags `ACCEL` can express.
fn accel_of(shortcut: &Shortcut, command: u16) -> ACCEL {
    let modifiers = shortcut.modifiers();
    let mut flags = FVIRTKEY;
    if modifiers.ctrl {
        flags |= FCONTROL;
    }
    if modifiers.shift {
        flags |= FSHIFT;
    }
    if modifiers.alt {
        flags |= FALT;
    }
    ACCEL {
        fVirt: flags,
        key: shortcut.key().code(),
        cmd: command,
    }
}

/// The command id assigned to the accelerator at `index`.
pub(crate) fn command_id(index: usize) -> u16 {
    COMMAND_BASE.wrapping_add(index as u16)
}

/// Destroys a table, ignoring a null handle.
fn destroy(table: Option<HACCEL>) {
    if let Some(table) = table {
        // SAFETY: the table was created by `CreateAcceleratorTableW` and is no
        // longer used by any window once removed from `WINDOWS`.
        unsafe {
            let _ = DestroyAcceleratorTable(table);
        }
    }
}

/// Retrieves and dispatches a single message. Blocks until one is available.
///
/// Key messages for a widget-layer window are first offered to its accelerator
/// table and then to `IsDialogMessageW`; a translated message is not passed to
/// `TranslateMessage`/`DispatchMessageW`.
pub(crate) fn pump() -> Pumped {
    let mut msg = MSG::default();
    // SAFETY: `msg` is a valid, aligned out-pointer and `GetMessageW` fully
    // initialises it before returning a positive value.
    let result = unsafe { GetMessageW(&mut msg, None, 0, 0) };
    match result.0 {
        0 => return Pumped::Quit(msg.wParam.0 as i32),
        -1 => return Pumped::Error,
        _ => {}
    }
    if !translate(&msg) {
        // SAFETY: the message just retrieved is translated and dispatched under
        // the standard WndProc contract; both calls only read `msg`.
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Pumped::Message
}

/// Offers `msg` to the owning app window's accelerator table and dialog
/// navigation, returning whether it was consumed.
fn translate(msg: &MSG) -> bool {
    let Some(hwnd) = app_window(msg.hwnd) else {
        return false;
    };
    let (table, dialog_nav) = WINDOWS.with(|windows| {
        let windows = windows.borrow();
        let state = windows.get(&hwnd.raw());
        (
            state.and_then(|state| state.table),
            state.is_some_and(|state| state.dialog_nav),
        )
    });

    if let Some(table) = table {
        // SAFETY: `table` is a live accelerator table owned by `WINDOWS`, and
        // `msg` is the message being pumped.
        if unsafe { TranslateAcceleratorW(raw_hwnd(hwnd), table, msg) } != 0 {
            return true;
        }
    }
    if dialog_nav {
        // SAFETY: `msg` is the message being pumped; `IsDialogMessageW` only
        // reads it and moves the focus among `hwnd`'s children.
        return unsafe { IsDialogMessageW(raw_hwnd(hwnd), msg).as_bool() };
    }
    false
}

/// The registered app window that owns `hwnd`: itself or its root ancestor (a
/// focused child control delivers key messages with the child's `HWND`).
fn app_window(hwnd: HWND) -> Option<Hwnd> {
    if hwnd.0.is_null() {
        return None;
    }
    let hwnd = hwnd_from(hwnd);
    if WINDOWS.with(|windows| windows.borrow().contains_key(&hwnd.raw())) {
        return Some(hwnd);
    }
    // SAFETY: `GetAncestor` only inspects the handle.
    let root = unsafe { GetAncestor(raw_hwnd(hwnd), GA_ROOT) };
    if root.0.is_null() {
        return None;
    }
    let root = hwnd_from(root);
    WINDOWS
        .with(|windows| windows.borrow().contains_key(&root.raw()))
        .then_some(root)
}

/// Ends the message loop with `code`.
pub(crate) fn post_quit(code: i32) {
    // SAFETY: `PostQuitMessage` takes no pointers and only touches the calling
    // thread's message queue.
    unsafe { PostQuitMessage(code) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{Key, Modifiers};

    #[test]
    fn accel_flags_carry_the_modifiers() {
        let ctrl_shift = accel_of(&Shortcut::ctrl(Key::N).with_shift(), 7);
        assert_eq!(ctrl_shift.key, Key::N.code());
        assert_eq!(ctrl_shift.cmd, 7);
        assert!(ctrl_shift.fVirt.contains(FVIRTKEY));
        assert!(ctrl_shift.fVirt.contains(FCONTROL));
        assert!(ctrl_shift.fVirt.contains(FSHIFT));
        assert!(!ctrl_shift.fVirt.contains(FALT));

        let plain = accel_of(&Shortcut::new(Key::F5, Modifiers::NONE), 0);
        assert!(plain.fVirt.contains(FVIRTKEY));
        assert!(!plain.fVirt.contains(FCONTROL));
    }

    #[test]
    fn command_ids_round_trip_through_the_base() {
        for index in [0usize, 1, 5, 255] {
            assert_eq!(accelerator_index(command_id(index)), Some(index));
        }
        assert_eq!(accelerator_index(COMMAND_BASE - 1), None);
        assert_eq!(accelerator_index(1), None);
    }

    #[test]
    fn builds_a_table_from_shortcuts() {
        let shortcuts = [
            Shortcut::ctrl(Key::N),
            Shortcut::shift(Key::F5),
            Shortcut::alt(Key::F4),
        ];
        let table = build_table(&shortcuts).expect("accelerator table");
        assert!(!table.is_invalid());
        destroy(Some(table));
    }
}
