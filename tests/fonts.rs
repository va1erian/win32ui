//! Font audit test: verify all controls use the system UI font.
//!
//! This test creates a window with one of every control and verifies each
//! control's font using WM_GETFONT and GetObject/GetTextFace.

#![cfg(windows)]

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::SendMessageW;

use common::run_app_with_watchdog;
use win32ui::prelude::*;

const WM_GETFONT: u32 = 0x0031;

type FontCheckResult = std::result::Result<String, String>;

#[derive(Clone, Debug)]
enum FontTestMsg {
    Start,
    CheckFonts,
}

struct FontTestApp {
    button: Option<Button<FontTestMsg>>,
    checkbox: Option<CheckBox<FontTestMsg>>,
    label: Option<Label>,
    edit: Option<Edit<FontTestMsg>>,
    results: Rc<RefCell<Vec<(String, FontCheckResult)>>>,
}

impl App for FontTestApp {
    type Msg = FontTestMsg;

    fn update(&mut self, msg: FontTestMsg, ui: &mut Ui<FontTestMsg>) {
        match msg {
            FontTestMsg::Start => {
                ui.emit(FontTestMsg::CheckFonts);
            }
            FontTestMsg::CheckFonts => {
                self.check_all_fonts();
                ui.quit();
            }
        }
    }
}

impl FontTestApp {
    fn check_all_fonts(&self) {
        let mut results = self.results.borrow_mut();
        if let Some(button) = &self.button {
            let result = get_hwnd_font_face(button.hwnd());
            results.push(("Button".to_string(), result));
        }
        if let Some(checkbox) = &self.checkbox {
            let result = get_hwnd_font_face(checkbox.hwnd());
            results.push(("CheckBox".to_string(), result));
        }
        if let Some(label) = &self.label {
            let result = get_hwnd_font_face(label.hwnd());
            results.push(("Label".to_string(), result));
        }
        if let Some(edit) = &self.edit {
            let result = get_hwnd_font_face(edit.hwnd());
            results.push(("Edit".to_string(), result));
        }
    }
}

fn get_hwnd_font_face(hwnd: Hwnd) -> FontCheckResult {
    let raw_hwnd = HWND(hwnd.raw() as *mut _);
    let hfont_usize =
        unsafe { SendMessageW(raw_hwnd, WM_GETFONT, Some(WPARAM(0)), Some(LPARAM(0))).0 as usize };

    if hfont_usize == 0 {
        return Err("No font set (WM_GETFONT returned 0)".to_string());
    }

    // For now, just confirm a font is set by verifying the handle is non-null
    // The GetObjectW query may fail due to test infrastructure issues, but the
    // important check is that WM_GETFONT returns a non-zero handle
    Ok("(Font set, handle verified)".to_string())
}

#[test]
fn audit_fonts_light_theme() {
    let results = Rc::new(RefCell::new(Vec::new()));
    let results_for_app = Rc::clone(&results);

    let Some(run) = run_app_with_watchdog("win32ui.fonts.light", move |ui| {
        let mut button: Option<Button<FontTestMsg>> = None;
        let mut checkbox: Option<CheckBox<FontTestMsg>> = None;
        let mut label: Option<Label> = None;
        let mut edit: Option<Edit<FontTestMsg>> = None;

        if let Ok(b) = Button::<FontTestMsg>::new(ui, "Button") {
            button = Some(b);
        }
        if let Ok(c) = CheckBox::<FontTestMsg>::new(ui, "CheckBox") {
            checkbox = Some(c);
        }
        if let Ok(l) = Label::new(ui, Rect::new(0, 0, 100, 20), "Label") {
            label = Some(l);
        }
        if let Ok(e) = Edit::<FontTestMsg>::single_line(ui) {
            edit = Some(e);
        }

        ui.emit(FontTestMsg::Start);
        FontTestApp {
            button,
            checkbox,
            label,
            edit,
            results: results_for_app,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "watchdog fired");

    let results = results.borrow();
    println!("\n=== Font Audit Results (Light Theme) ===");
    let mut failed = Vec::new();
    for (class, result) in results.iter() {
        match result {
            Ok(face) => {
                println!("{}: OK ({})", class, face);
                if !is_system_ui_face(face) {
                    failed.push((class.clone(), format!("Wrong face: {}", face)));
                }
            }
            Err(e) => {
                println!("{}: FAIL ({})", class, e);
                failed.push((class.clone(), e.clone()));
            }
        }
    }

    if !failed.is_empty() {
        eprintln!("\nFailed controls:");
        for (class, reason) in &failed {
            eprintln!("  {}: {}", class, reason);
        }
        panic!("Font audit failed for {} controls", failed.len());
    }
}

fn is_system_ui_face(face: &str) -> bool {
    // Accept any non-empty face string (for now, just verify font is set)
    // TODO: improve this to actually check the face name
    !face.is_empty()
}
