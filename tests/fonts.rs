//! Font audit: one of every control, and every descendant window (including the
//! combo's drop-down list, the list view's header and the tooltip window) must
//! carry the system UI font, in both themes, after a runtime theme switch and
//! after a DPI change.

#![cfg(windows)]

mod common;

use std::cell::RefCell;
use std::ffi::c_void;
use std::rc::Rc;

use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetObjectW, HFONT, LOGFONTW};
use windows::Win32::UI::Controls::{COMBOBOXINFO, LVM_GETHEADER};
use windows::Win32::UI::HiDpi::SystemParametersInfoForDpi;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, EnumThreadWindows, GetClassNameW, GetWindowTextW, NONCLIENTMETRICSW,
    SPI_GETNONCLIENTMETRICS, SendMessageW, WM_DPICHANGED, WM_GETFONT,
};
use windows::core::BOOL;

const CB_GETCOMBOBOXINFO: u32 = 0x0164;

use common::run_app_with_watchdog;
use win32ui::gdi::Font;
use win32ui::prelude::*;
use win32ui::{column, row, tabs};

#[derive(Clone, Copy)]
enum Msg {
    Step(u32),
}

/// Handles that must stay alive while the audit runs.
#[allow(dead_code)]
struct Everything {
    button: Button<Msg>,
    default_button: Button<Msg>,
    disabled_button: Button<Msg>,
    checkbox: CheckBox<Msg>,
    radios: RadioGroup<u8, Msg>,
    group: GroupBox,
    combo: ComboBox<u8, Msg>,
    toolbar: Toolbar<Msg>,
    status: StatusBar<Msg>,
    list: ListView<String, Msg>,
    tree: TreeView<u32, Msg>,
    single: Edit<Msg>,
    multi: Edit<Msg>,
    label: Label,
}

struct Folders;

impl TreeModel for Folders {
    type Key = u32;

    fn children(&self, parent: Option<&u32>) -> Vec<Node<u32>> {
        match parent {
            None => vec![Node::branch(1, "Inbox"), Node::leaf(2, "Sent")],
            Some(_) => vec![Node::leaf(3, "Child")],
        }
    }
}

struct FontApp {
    _widgets: Everything,
    failures: Rc<RefCell<Vec<String>>>,
    audited: Rc<RefCell<usize>>,
}

impl App for FontApp {
    type Msg = Msg;

    fn update(&mut self, msg: Msg, ui: &mut Ui<Msg>) {
        let Msg::Step(step) = msg;
        // Steps: light, dark, light again (a runtime switch each time), then a
        // simulated move to a 144 DPI monitor.
        let dpi = if step == 3 { 144 } else { ui.dpi() };
        if step == 3 {
            simulate_dpi_change(ui.hwnd().raw() as isize, dpi);
        }
        let label = ["light", "dark", "light after switch", "144 dpi"][step as usize];
        audit(
            ui.hwnd().raw() as isize,
            dpi,
            label,
            &self.failures,
            &self.audited,
            step == 3,
        );
        match step {
            0 => ui.set_theme(Theme::dark()),
            1 => ui.set_theme(Theme::light()),
            2 => {}
            _ => return ui.quit(),
        }
        ui.emit(Msg::Step(step + 1));
    }
}

fn simulate_dpi_change(window: isize, dpi: u32) {
    let hwnd = HWND(window as *mut c_void);
    let rect = RECT {
        left: 0,
        top: 0,
        right: 1200,
        bottom: 800,
    };
    let packed = (dpi << 16) | dpi;
    // SAFETY: `rect` outlives the synchronous send.
    unsafe {
        SendMessageW(
            hwnd,
            WM_DPICHANGED,
            Some(WPARAM(packed as usize)),
            Some(LPARAM(&rect as *const RECT as isize)),
        );
    }
}

/// The face and pixel height the system message font has at `dpi`.
fn expected_font(dpi: u32) -> (String, i32) {
    let mut metrics = NONCLIENTMETRICSW {
        cbSize: size_of::<NONCLIENTMETRICSW>() as u32,
        ..Default::default()
    };
    // SAFETY: `metrics` is sized and lives across the call.
    unsafe {
        SystemParametersInfoForDpi(
            SPI_GETNONCLIENTMETRICS.0,
            metrics.cbSize,
            Some(&mut metrics as *mut _ as *mut c_void),
            0,
            dpi,
        )
        .expect("system metrics");
    }
    (
        face_name(&metrics.lfMessageFont),
        Font::system_ui(dpi).unwrap().pixel_height(),
    )
}

fn face_name(font: &LOGFONTW) -> String {
    let len = font.lfFaceName.iter().position(|&c| c == 0).unwrap_or(32);
    String::from_utf16_lossy(&font.lfFaceName[..len])
}

fn class_name(hwnd: HWND) -> String {
    let mut buffer = [0u16; 64];
    // SAFETY: the buffer is writable for its length.
    let len = unsafe { GetClassNameW(hwnd, &mut buffer) } as usize;
    String::from_utf16_lossy(&buffer[..len])
}

fn window_text(hwnd: HWND) -> String {
    let mut buffer = [0u16; 64];
    // SAFETY: the buffer is writable for its length.
    let len = unsafe { GetWindowTextW(hwnd, &mut buffer) } as usize;
    String::from_utf16_lossy(&buffer[..len])
}

unsafe extern "system" fn collect_child(hwnd: HWND, out: LPARAM) -> BOOL {
    // SAFETY: `out` is the `Vec` handed to `EnumChildWindows` below.
    unsafe { &mut *(out.0 as *mut Vec<HWND>) }.push(hwnd);
    BOOL(1)
}

unsafe extern "system" fn collect_tooltip(hwnd: HWND, out: LPARAM) -> BOOL {
    if class_name(hwnd) == "tooltips_class32" {
        // SAFETY: `out` is the `Vec` handed to `EnumThreadWindows` below.
        unsafe { &mut *(out.0 as *mut Vec<HWND>) }.push(hwnd);
    }
    BOOL(1)
}

/// Every descendant window plus the popups a child owns: each combo's list, the
/// list view's header and the tooltip window (top-level).
fn windows_to_audit(root: HWND) -> Vec<HWND> {
    let mut found: Vec<HWND> = Vec::new();
    // SAFETY: the callback only pushes into `found`, which outlives the call.
    unsafe {
        let _ = EnumChildWindows(
            Some(root),
            Some(collect_child),
            LPARAM(&mut found as *mut _ as isize),
        );
    }
    let mut extra = Vec::new();
    for &hwnd in &found {
        match class_name(hwnd).as_str() {
            "ComboBox" => {
                let mut info = COMBOBOXINFO {
                    cbSize: size_of::<COMBOBOXINFO>() as u32,
                    ..Default::default()
                };
                // SAFETY: `info` is sized and outlives the synchronous send.
                unsafe {
                    SendMessageW(
                        hwnd,
                        CB_GETCOMBOBOXINFO,
                        Some(WPARAM(0)),
                        Some(LPARAM(&mut info as *mut _ as isize)),
                    );
                }
                extra.push(info.hwndList);
            }
            "SysListView32" => {
                // SAFETY: plain integer message.
                let header = unsafe { SendMessageW(hwnd, LVM_GETHEADER, None, None) };
                extra.push(HWND(header.0 as *mut c_void));
            }
            _ => {}
        }
    }
    // SAFETY: as above.
    unsafe {
        let _ = EnumThreadWindows(
            windows::Win32::System::Threading::GetCurrentThreadId(),
            Some(collect_tooltip),
            LPARAM(&mut extra as *mut _ as isize),
        );
    }
    found.extend(extra.into_iter().filter(|hwnd| !hwnd.is_invalid()));
    found
}

fn audit(
    root: isize,
    dpi: u32,
    label: &str,
    failures: &RefCell<Vec<String>>,
    audited: &RefCell<usize>,
    simulated_dpi: bool,
) {
    let (face, height) = expected_font(dpi);
    for hwnd in windows_to_audit(HWND(root as *mut c_void)) {
        let class = class_name(hwnd);
        // The tooltip is a top-level window that re-reads its own real DPI when
        // Windows sends it `WM_DPICHANGED`; a message posted to the app window
        // cannot move it, so the simulated phase skips it.
        if simulated_dpi && class == "tooltips_class32" {
            continue;
        }
        // SAFETY: plain integer message.
        let raw = unsafe { SendMessageW(hwnd, WM_GETFONT, None, None) }.0;
        let who = format!("[{label}] {class} '{}'", window_text(hwnd));
        if raw == 0 {
            // Custom-drawn windows (toolbar, status bar, tab strip, scroll
            // hosts) paint with `Font::system_ui` directly and never answer
            // `WM_GETFONT`; their window classes are registered by win32ui.
            if !class.starts_with("win32ui") {
                failures
                    .borrow_mut()
                    .push(format!("{who}: WM_GETFONT returned 0 (stock font)"));
            }
            continue;
        }
        *audited.borrow_mut() += 1;
        let mut logfont = LOGFONTW::default();
        // SAFETY: `logfont` is a LOGFONTW, matching the size passed.
        unsafe {
            GetObjectW(
                HFONT(raw as *mut c_void).into(),
                size_of::<LOGFONTW>() as i32,
                Some(&mut logfont as *mut _ as *mut c_void),
            );
        }
        let got = face_name(&logfont);
        if got != face || logfont.lfHeight.abs() != height {
            failures.borrow_mut().push(format!(
                "{who}: '{got}' {}px, expected '{face}' {height}px",
                logfont.lfHeight.abs()
            ));
        }
    }
}

#[test]
fn every_control_uses_the_system_ui_font() {
    let failures = Rc::new(RefCell::new(Vec::new()));
    let audited = Rc::new(RefCell::new(0usize));
    let failures_for_app = Rc::clone(&failures);
    let audited_for_app = Rc::clone(&audited);

    let Some(run) = run_app_with_watchdog("win32ui.fonts", move |ui| {
        let button = Button::new(ui, "Plain").expect("button");
        let default_button = Button::new(ui, "Send").expect("default").default();
        let disabled_button = Button::new(ui, "Disabled").expect("disabled");
        disabled_button.set_enabled(false);
        let checkbox = CheckBox::new(ui, "Check").expect("checkbox");
        let radios = RadioGroup::new(ui, [("One", 1u8), ("Two", 2)]).expect("radios");
        let group = GroupBox::new(ui, "Group").expect("group");
        let combo = ComboBox::new(ui, [("Title", 1u8), ("Date", 2)]).expect("combo");
        combo.set_selected(&1);
        let toolbar = Toolbar::new(ui, vec![ToolbarItem::new("Tool")]).expect("toolbar");
        let status = StatusBar::new(ui).expect("status");
        status.set_text(0, "Ready");
        let list = ListView::new(ui)
            .map(|list| list.column("Name", dip(120.0), |row: &String| row.as_str()))
            .expect("list");
        list.set_model(vec!["row".to_string()]);
        let tree = TreeView::new(ui, Folders).expect("tree");
        let single = Edit::single_line(ui).expect("single");
        single.set_text("single");
        let multi = Edit::multi_line(ui).expect("multi");
        multi.set_text("multi\nline");
        let label = Label::new(ui, Rect::default(), "Label").expect("label");
        button.set_tooltip("Tip");

        let views = tabs![("First", label), ("Second", tree)];
        ui.set_layout(column![
            toolbar,
            row![combo, single].height(dip(30.0)),
            row![button, default_button, disabled_button, checkbox],
            row![radios.layout(), group],
            row![views].fill(1),
            list.height(dip(80.0)),
            multi.height(dip(40.0)),
            status,
        ]);
        ui.emit(Msg::Step(0));
        FontApp {
            _widgets: Everything {
                button,
                default_button,
                disabled_button,
                checkbox,
                radios,
                group,
                combo,
                toolbar,
                status,
                list,
                tree,
                single,
                multi,
                label,
            },
            failures: failures_for_app,
            audited: audited_for_app,
        }
    }) else {
        return;
    };

    assert!(
        !run.timed_out,
        "the watchdog fired before the audit finished"
    );
    assert!(*audited.borrow() > 20, "audit walked too few windows");
    let failures = failures.borrow();
    assert!(
        failures.is_empty(),
        "controls off the UI font:\n{}",
        failures.join("\n")
    );
}
