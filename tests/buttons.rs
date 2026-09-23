//! Buttons (#13): clicks, toggles, typed radio selection, text and teardown.
//!
//! Window-creating tests use the shared watchdog helper so failures fail
//! instead of hanging.

#![cfg(windows)]

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::run_app_with_watchdog;
use win32ui::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq)]
enum BtnMsg {
    Start,
    Clicked,
}

struct BtnApp {
    button: Option<Button<BtnMsg>>,
    log: Rc<RefCell<Vec<BtnMsg>>>,
}

impl App for BtnApp {
    type Msg = BtnMsg;

    fn update(&mut self, msg: BtnMsg, ui: &mut Ui<BtnMsg>) {
        self.log.borrow_mut().push(msg.clone());
        match msg {
            BtnMsg::Start => {
                if let Some(button) = &self.button {
                    // Fires `BN_CLICKED` synchronously; the mapped message must
                    // still arrive after this `update` returns, never nested.
                    button.click();
                }
            }
            BtnMsg::Clicked => ui.quit(),
        }
    }
}

/// A programmatic click maps to the app's message through `on_click`.
#[test]
fn button_click_maps_to_msg() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let created = Rc::new(Cell::new(false));
    let log_for_make = Rc::clone(&log);
    let created_for_make = Rc::clone(&created);
    let Some(run) = run_app_with_watchdog("win32ui.buttons.click", move |ui| {
        let button = Button::new(ui, "Send")
            .ok()
            .map(|button| button.on_click(|| Some(BtnMsg::Clicked)));
        created_for_make.set(button.is_some());
        ui.emit(BtnMsg::Start);
        BtnApp {
            button,
            log: log_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    if !created.get() {
        return;
    }
    assert_eq!(
        *log.borrow(),
        vec![BtnMsg::Start, BtnMsg::Clicked],
        "the click message did not arrive after Start returned"
    );
}

/// The default flag round-trips through the native button style.
#[test]
fn default_button_flag_round_trips() {
    let was_default = Rc::new(Cell::new(false));
    let cleared = Rc::new(Cell::new(false));
    let created = Rc::new(Cell::new(false));
    let was_default_for_make = Rc::clone(&was_default);
    let cleared_for_make = Rc::clone(&cleared);
    let created_for_make = Rc::clone(&created);

    struct FlagApp;
    impl App for FlagApp {
        type Msg = ();
        fn update(&mut self, _msg: (), ui: &mut Ui<()>) {
            ui.quit();
        }
    }

    let Some(run) = run_app_with_watchdog("win32ui.buttons.default", move |ui| {
        if let Ok(button) = Button::<()>::new(ui, "Send") {
            let button = button.default();
            was_default_for_make.set(button.is_default());
            button.set_default(false);
            cleared_for_make.set(!button.is_default());
            created_for_make.set(true);
        }
        ui.emit(());
        FlagApp
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    if !created.get() {
        return;
    }
    assert!(was_default.get(), ".default() did not mark the button");
    assert!(cleared.get(), "set_default(false) did not clear the mark");
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ToggleMsg {
    Start,
    Toggled(bool),
}

struct ToggleApp {
    check: Option<CheckBox<ToggleMsg>>,
    log: Rc<RefCell<Vec<ToggleMsg>>>,
}

impl App for ToggleApp {
    type Msg = ToggleMsg;

    fn update(&mut self, msg: ToggleMsg, ui: &mut Ui<ToggleMsg>) {
        self.log.borrow_mut().push(msg.clone());
        match msg {
            ToggleMsg::Start => {
                if let Some(check) = &self.check {
                    check.click();
                }
            }
            ToggleMsg::Toggled(_) => ui.quit(),
        }
    }
}

/// Clicking an unchecked box reports `true` through `on_toggle`.
#[test]
fn checkbox_toggle_reports_new_state() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let created = Rc::new(Cell::new(false));
    let log_for_make = Rc::clone(&log);
    let created_for_make = Rc::clone(&created);
    let Some(run) = run_app_with_watchdog("win32ui.buttons.toggle", move |ui| {
        let check = CheckBox::new(ui, "Load remote images").ok().map(|check| {
            check
                .checked(false)
                .on_toggle(|on| Some(ToggleMsg::Toggled(on)))
        });
        created_for_make.set(check.is_some());
        ui.emit(ToggleMsg::Start);
        ToggleApp {
            check,
            log: log_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    if !created.get() {
        return;
    }
    assert_eq!(
        *log.borrow(),
        vec![ToggleMsg::Start, ToggleMsg::Toggled(true)],
        "the toggle did not report the new checked state"
    );
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Choice {
    Light,
    Dark,
    System,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum RadioMsg {
    Start,
    Chose(Choice),
}

struct RadioApp {
    group: Option<RadioGroup<Choice, RadioMsg>>,
    log: Rc<RefCell<Vec<RadioMsg>>>,
}

impl App for RadioApp {
    type Msg = RadioMsg;

    fn update(&mut self, msg: RadioMsg, ui: &mut Ui<RadioMsg>) {
        self.log.borrow_mut().push(msg.clone());
        match msg {
            RadioMsg::Start => {
                if let Some(group) = &self.group {
                    group.click(0);
                }
            }
            RadioMsg::Chose(_) => ui.quit(),
        }
    }
}

/// Selection is typed by value: clicking the first button reports its value
/// and updates the group's selection.
#[test]
fn radiogroup_selects_by_value() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let initial = Rc::new(Cell::new(None));
    let created = Rc::new(Cell::new(false));
    let log_for_make = Rc::clone(&log);
    let initial_for_make = Rc::clone(&initial);
    let created_for_make = Rc::clone(&created);
    let Some(run) = run_app_with_watchdog("win32ui.buttons.radio", move |ui| {
        let group = RadioGroup::new(
            ui,
            [
                ("Light", Choice::Light),
                ("Dark", Choice::Dark),
                ("System", Choice::System),
            ],
        )
        .ok()
        .map(|group| {
            group
                .selected(Choice::Dark)
                .on_select(|choice| Some(RadioMsg::Chose(*choice)))
        });
        initial_for_make.set(group.as_ref().and_then(|group| group.selected_value()));
        created_for_make.set(group.is_some());
        ui.emit(RadioMsg::Start);
        RadioApp {
            group,
            log: log_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    if !created.get() {
        return;
    }
    assert_eq!(
        initial.get(),
        Some(Choice::Dark),
        ".selected() did not pick the initial value"
    );
    assert_eq!(
        *log.borrow(),
        vec![RadioMsg::Start, RadioMsg::Chose(Choice::Light)],
        "clicking the first button did not report its value"
    );
}

/// `HasText` round-trips on every button-like widget.
#[test]
fn button_text_round_trips() {
    let matches = Rc::new(Cell::new(false));
    let created = Rc::new(Cell::new(false));
    let matches_for_make = Rc::clone(&matches);
    let created_for_make = Rc::clone(&created);

    struct TextApp;
    impl App for TextApp {
        type Msg = ();
        fn update(&mut self, _msg: (), ui: &mut Ui<()>) {
            ui.quit();
        }
    }

    let Some(run) = run_app_with_watchdog("win32ui.buttons.text", move |ui| {
        let ok = (|| {
            let button = Button::<()>::new(ui, "Send")?;
            let check = CheckBox::<()>::new(ui, "Remote")?;
            let group =
                RadioGroup::<&'static str, ()>::new(ui, [("Light", "light"), ("Dark", "dark")])?;
            let frame = GroupBox::new(ui, "Theme")?;
            button.set_text("Sent");
            let matched = button.text() == "Sent"
                && check.text() == "Remote"
                && group.options().len() == 2
                && group.options()[1].text() == "Dark"
                && frame.text() == "Theme";
            matches_for_make.set(matched);
            created_for_make.set(true);
            win32ui::Result::Ok(())
        })()
        .is_ok();
        if !ok {
            created_for_make.set(false);
        }
        ui.emit(());
        TextApp
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    if !created.get() {
        return;
    }
    assert!(matches.get(), "text did not round-trip");
}

enum DropBtnMsg {
    Start,
    Drop,
    Check,
}

struct DropBtnApp {
    button: Option<Button<DropBtnMsg>>,
    check: Option<CheckBox<DropBtnMsg>>,
    group: Option<RadioGroup<u32, DropBtnMsg>>,
    frame: Option<GroupBox>,
    hwnds: Vec<Hwnd>,
    alive_after_drop: Rc<Cell<bool>>,
}

impl App for DropBtnApp {
    type Msg = DropBtnMsg;

    fn update(&mut self, msg: DropBtnMsg, ui: &mut Ui<DropBtnMsg>) {
        match msg {
            DropBtnMsg::Start => ui.emit(DropBtnMsg::Drop),
            DropBtnMsg::Drop => {
                self.button = None;
                self.check = None;
                self.group = None;
                self.frame = None;
                ui.emit(DropBtnMsg::Check);
            }
            DropBtnMsg::Check => {
                self.alive_after_drop
                    .set(self.hwnds.iter().any(|hwnd| hwnd.is_alive()));
                ui.quit();
            }
        }
    }
}

/// Dropping the widgets destroys every `HWND` they own.
#[test]
fn dropping_buttons_destroys_them() {
    let alive = Rc::new(Cell::new(true));
    let created = Rc::new(Cell::new(false));
    let alive_for_make = Rc::clone(&alive);
    let created_for_make = Rc::clone(&created);
    let Some(run) = run_app_with_watchdog("win32ui.buttons.drop", move |ui| {
        let button = Button::new(ui, "Send").ok();
        let check = CheckBox::new(ui, "Remote").ok();
        let group = RadioGroup::new(ui, [("A", 1u32), ("B", 2u32)]).ok();
        let frame = GroupBox::new(ui, "Theme").ok();
        let mut hwnds = Vec::new();
        if let Some(button) = &button {
            hwnds.push(button.hwnd());
        }
        if let Some(check) = &check {
            hwnds.push(check.hwnd());
        }
        if let Some(group) = &group {
            for option in group.options() {
                hwnds.push(option.hwnd());
            }
        }
        if let Some(frame) = &frame {
            hwnds.push(frame.hwnd());
        }
        created_for_make
            .set(button.is_some() && check.is_some() && group.is_some() && frame.is_some());
        if button.is_none() {
            ui.quit();
        } else {
            ui.emit(DropBtnMsg::Start);
        }
        DropBtnApp {
            button,
            check,
            group,
            frame,
            hwnds,
            alive_after_drop: alive_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    if !created.get() {
        return;
    }
    assert!(
        !alive.get(),
        "a button HWND survived its widget being dropped"
    );
}
