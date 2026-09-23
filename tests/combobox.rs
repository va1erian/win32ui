//! ComboBox behaviour that needs a real window: the label/value mapping, the
//! UTF-16 item round trip and selection preservation across `set_items`.

#![cfg(windows)]

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::{
    capture_screen, is_near_white, run_app_spec_with_watchdog, run_app_with_watchdog, screen_rect,
};
use win32ui::prelude::*;

enum Msg {
    Start,
}

struct MappedApp {
    _combo: Option<ComboBox<u32, Msg>>,
}

impl App for MappedApp {
    type Msg = Msg;

    fn update(&mut self, msg: Msg, ui: &mut Ui<Msg>) {
        let Msg::Start = msg;
        ui.quit();
    }
}

#[test]
fn items_and_values_round_trip() {
    let passed = Rc::new(Cell::new(false));
    let created = Rc::new(Cell::new(false));

    let passed_for_make = Rc::clone(&passed);
    let created_for_make = Rc::clone(&created);
    let Some(run) = run_app_with_watchdog("win32ui.combo", move |ui| {
        let mut combo = ComboBox::new(ui, [("Alpha", 10u32), ("Beta", 20), ("Gamma", 30)]).ok();

        if let Some(combo) = &mut combo {
            let mut ok = true;

            // Index <-> value mapping.
            ok &= combo.len() == 3;
            ok &= !combo.is_empty();
            ok &= combo.selected().is_none();
            combo.set_selected(&20);
            ok &= combo.selected() == Some(&20);
            ok &= combo.selected_index() == Some(1);
            ok &= combo.label(2) == Some("Gamma");
            ok &= combo.label(9).is_none();

            // Selecting a value that is not present clears the selection.
            combo.set_selected(&99);
            ok &= combo.selected().is_none();

            // UTF-16 labels survive the native round trip.
            combo.set_items([("日本語", 1u32), ("🎵 emoji", 2), ("e\u{301}", 3)]);
            ok &= combo.item_text(0) == "日本語";
            ok &= combo.item_text(1) == "🎵 emoji";
            ok &= combo.item_text(2) == "e\u{301}";

            // A selected value survives set_items when it is still present,
            // and is cleared when it is gone.
            combo.set_selected(&2);
            combo.set_items([("New", 5u32), ("Kept", 2), ("Other", 6)]);
            ok &= combo.selected() == Some(&2);
            combo.set_items([("Only", 5u32)]);
            ok &= combo.selected().is_none();

            // A live theme switch leaves the mapping intact.
            ui.set_theme(Theme::dark());
            combo.set_selected(&5);
            ok &= combo.selected() == Some(&5);

            passed_for_make.set(ok);
        }

        if combo.is_none() {
            ui.quit();
        } else {
            created_for_make.set(true);
            ui.emit(Msg::Start);
        }
        MappedApp { _combo: combo }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    if !created.get() {
        return;
    }
    assert!(passed.get(), "the combo box item/value mapping was wrong");
}

/// #67 regression: in the dark theme the combo's closed field and its dropped
/// list must be dark, not the bright white that `DarkMode_Explorer` produced.
/// Popups are not captured by `PrintWindow`, so the check blits the screen
/// region of the combo and of its dropped list (`GetComboBoxInfo`).
#[test]
fn dark_combo_field_and_dropped_list_are_dark() {
    enum DarkMsg {
        Start,
        Check,
    }

    struct DarkComboApp {
        combo: Option<ComboBox<u32, DarkMsg>>,
        field: Rc<Cell<Option<[u8; 4]>>>,
        list: Rc<Cell<Option<[u8; 4]>>>,
    }

    impl App for DarkComboApp {
        type Msg = DarkMsg;

        fn update(&mut self, msg: DarkMsg, ui: &mut Ui<DarkMsg>) {
            let Some(combo) = &self.combo else {
                ui.quit();
                return;
            };
            match msg {
                // The window has painted by now; drop the list down and let the
                // worker thread ask for the capture once it is on screen.
                DarkMsg::Start => combo.show_drop_down(true),
                DarkMsg::Check => {
                    self.field.set(sample_combo_field(combo.hwnd()));
                    self.list.set(sample_combo_list(combo.hwnd()));
                    ui.quit();
                }
            }
        }
    }

    let field = Rc::new(Cell::new(None));
    let list = Rc::new(Cell::new(None));
    let field_for_make = Rc::clone(&field);
    let list_for_make = Rc::clone(&list);

    let Some(run) = run_app_spec_with_watchdog(
        WindowSpec::new("win32ui.combo.dark").theme(Theme::dark()),
        move |ui| {
            let combo = ComboBox::new(ui, [("Title", 1u32), ("Artist", 2), ("Album", 3)]).ok();
            // Give the control a real size; a combo with no layout stays
            // zero-width and cannot be sampled.
            if let Some(combo) = &combo {
                ui.set_layout(win32ui::column![*combo]);
            }
            let proxy = ui.proxy();
            // The shared harness owns the single timer mapping (the watchdog),
            // so the worker thread nudges the UI queue instead.
            let check = std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(500));
                let _ = proxy.send(DarkMsg::Check);
            });
            let _ = check;
            ui.emit(DarkMsg::Start);
            DarkComboApp {
                combo,
                field: field_for_make,
                list: list_for_make,
            }
        },
    ) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    // A headless session cannot blit the screen; skip rather than fail.
    let Some(field) = field.get() else {
        return;
    };
    assert!(
        !is_near_white(field),
        "the dark combo's closed field is near-white: {field:?}"
    );
    assert!(
        field[0] < 120 && field[1] < 120 && field[2] < 120,
        "the dark combo's closed field is too bright: {field:?}"
    );

    let Some(list) = list.get() else {
        return;
    };
    assert!(
        !is_near_white(list) && list[0] < 120 && list[1] < 120 && list[2] < 120,
        "the dark combo's dropped list is too bright: {list:?}"
    );
}

/// A blank pixel near the right of the combo's closed field (right of the
/// selected text, left of the drop-down arrow button).
fn sample_combo_field(combo: win32ui::Hwnd) -> Option<[u8; 4]> {
    let rect = screen_rect(combo)?;
    let image = capture_screen(rect)?;
    image.pixel((rect.width() as u32 * 3) / 4, rect.height() as u32 / 2)
}

/// A blank pixel in the middle of the combo's dropped list.
fn sample_combo_list(combo: win32ui::Hwnd) -> Option<[u8; 4]> {
    let list = combo_list_hwnd(combo)?;
    let rect = screen_rect(list)?;
    let image = capture_screen(rect)?;
    image.pixel(rect.width() as u32 / 2, rect.height() as u32 / 2)
}

/// The `hwndList` of a combo box, found through the documented
/// `GetComboBoxInfo`.
fn combo_list_hwnd(combo: win32ui::Hwnd) -> Option<win32ui::Hwnd> {
    use core::ffi::c_void;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Controls::{COMBOBOXINFO, GetComboBoxInfo};

    let mut info = COMBOBOXINFO {
        cbSize: size_of::<COMBOBOXINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `combo` is a live combo box and `info` is a valid out-pointer.
    unsafe {
        GetComboBoxInfo(HWND(combo.raw() as *mut c_void), &mut info).ok()?;
    }
    (!info.hwndList.0.is_null()).then(|| win32ui::Hwnd::from_raw(info.hwndList.0 as usize))
}
