//! Secondary windows: a non-modal child is destroyed with its owner, a modal
//! disables and re-enables its owner, and both run their own `App` on the
//! shared message loop.

#![cfg(windows)]

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::run_app_with_watchdog;
use win32ui::column;
use win32ui::prelude::*;

/// A child app that records delivery of `Ping` and closes itself.
#[derive(Debug)]
enum ChildMsg {
    Ping,
}

struct ChildApp {
    got: Rc<Cell<bool>>,
}

impl App for ChildApp {
    type Msg = ChildMsg;

    fn update(&mut self, msg: ChildMsg, ui: &mut Ui<ChildMsg>) {
        match msg {
            ChildMsg::Ping => {
                self.got.set(true);
                ui.close();
            }
        }
    }
}

/// A child app that does nothing (used by the owner-destroy test).
struct NoopApp;

impl App for NoopApp {
    type Msg = ();

    fn update(&mut self, _msg: (), _ui: &mut Ui<()>) {}
}

enum ParentMsg {
    Start,
    Send,
    Check,
}

struct ParentApp {
    child: Option<WindowHandle<ChildMsg>>,
    child_alive_after_close: Rc<Cell<bool>>,
    got: Rc<Cell<bool>>,
}

impl App for ParentApp {
    type Msg = ParentMsg;

    fn update(&mut self, msg: ParentMsg, ui: &mut Ui<ParentMsg>) {
        match msg {
            ParentMsg::Start => {
                let got = Rc::clone(&self.got);
                let handle = ui
                    .open_window(
                        WindowSpec::new("Child").size(dip(260.0), dip(120.0)),
                        move |_ui| ChildApp { got },
                    )
                    .expect("child window");
                self.child = Some(handle);
                let timer = ui.set_timer(100).ok();
                if let Some(timer) = timer {
                    ui.on_timer(move |id| (id == timer).then_some(ParentMsg::Check));
                }
                ui.emit(ParentMsg::Send);
            }
            ParentMsg::Send => {
                self.child
                    .as_ref()
                    .expect("child")
                    .send(ChildMsg::Ping)
                    .expect("child alive");
            }
            ParentMsg::Check => {
                self.child_alive_after_close
                    .set(self.child.as_ref().is_some_and(|child| child.is_alive()));
                ui.quit();
            }
        }
    }
}

/// `open_window` runs a child with its own `App`; `send` delivers to it and the
/// child can close itself.
#[test]
fn child_runs_its_own_app_and_receives_sends() {
    let got = Rc::new(Cell::new(false));
    let child_alive_after_close = Rc::new(Cell::new(true));
    let got_for_make = Rc::clone(&got);
    let alive_for_make = Rc::clone(&child_alive_after_close);
    let Some(run) = run_app_with_watchdog("win32ui.app.child", move |ui| {
        ui.emit(ParentMsg::Start);
        ParentApp {
            child: None,
            child_alive_after_close: alive_for_make,
            got: got_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    assert!(got.get(), "the child never received the sent message");
    assert!(
        !child_alive_after_close.get(),
        "the child was still alive after closing itself"
    );
}

enum OwnerMsg {
    Start,
}

struct OwnerApp {
    disabled: Rc<Cell<bool>>,
    enabled_after: Rc<Cell<bool>>,
}

impl App for OwnerApp {
    type Msg = OwnerMsg;

    fn update(&mut self, msg: OwnerMsg, ui: &mut Ui<OwnerMsg>) {
        match msg {
            OwnerMsg::Start => {
                let owner = ui.clone();
                let disabled = Rc::clone(&self.disabled);
                let result: Option<bool> = ui.open_modal(
                    WindowSpec::new("Confirm").size(dip(260.0), dip(120.0)),
                    move |ui| {
                        ui.emit(ConfirmMsg::Check);
                        ConfirmApp { owner, disabled }
                    },
                );
                assert_eq!(result, Some(true), "the modal returned its result");
                self.enabled_after.set(ui.is_enabled());
                ui.quit();
            }
        }
    }
}

enum ConfirmMsg {
    Check,
}

struct ConfirmApp {
    owner: Ui<OwnerMsg>,
    disabled: Rc<Cell<bool>>,
}

impl App for ConfirmApp {
    type Msg = ConfirmMsg;

    fn update(&mut self, msg: ConfirmMsg, ui: &mut Ui<ConfirmMsg>) {
        match msg {
            ConfirmMsg::Check => {
                self.disabled.set(!self.owner.is_enabled());
                ui.close_with_result(true);
            }
        }
    }
}

/// `open_modal` disables its owner while the child runs and re-enables it
/// before returning the child's result.
#[test]
fn modal_disables_then_reenables_its_owner() {
    let disabled = Rc::new(Cell::new(false));
    let enabled_after = Rc::new(Cell::new(false));
    let disabled_for_make = Rc::clone(&disabled);
    let enabled_for_make = Rc::clone(&enabled_after);
    let Some(run) = run_app_with_watchdog("win32ui.app.modal", move |ui| {
        ui.emit(OwnerMsg::Start);
        OwnerApp {
            disabled: disabled_for_make,
            enabled_after: enabled_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    assert!(
        disabled.get(),
        "the owner was not disabled while the modal ran"
    );
    assert!(
        enabled_after.get(),
        "the owner was not re-enabled after the modal closed"
    );
}

enum CloseMsg {
    Close,
}

struct ParentCloseApp;

impl App for ParentCloseApp {
    type Msg = CloseMsg;

    fn update(&mut self, msg: CloseMsg, ui: &mut Ui<CloseMsg>) {
        match msg {
            CloseMsg::Close => ui.close(),
        }
    }
}

#[derive(Debug)]
enum ChildLayoutMsg {
    SetText,
}

struct ChildLayoutApp {
    label: Label,
    label_text: Rc<Cell<String>>,
}

impl App for ChildLayoutApp {
    type Msg = ChildLayoutMsg;

    fn update(&mut self, msg: ChildLayoutMsg, _ui: &mut Ui<ChildLayoutMsg>) {
        match msg {
            ChildLayoutMsg::SetText => {
                self.label.set_text("Counter: 1");
                self.label_text.set(self.label.text());
            }
        }
    }
}

enum ParentLayoutMsg {
    Start,
    Check,
}

struct ParentLayoutApp {
    child: Option<WindowHandle<ChildLayoutMsg>>,
    child_client: Rc<Cell<Rect>>,
    label_bounds: Rc<Cell<Rect>>,
    child_alive: Rc<Cell<bool>>,
    label_text: Rc<Cell<String>>,
}

impl App for ParentLayoutApp {
    type Msg = ParentLayoutMsg;

    fn update(&mut self, msg: ParentLayoutMsg, ui: &mut Ui<ParentLayoutMsg>) {
        match msg {
            ParentLayoutMsg::Start => {
                let child_client = Rc::clone(&self.child_client);
                let label_bounds = Rc::clone(&self.label_bounds);
                let label_text = Rc::clone(&self.label_text);
                let handle = ui
                    .open_window(
                        WindowSpec::new("Child layout").size(dip(260.0), dip(120.0)),
                        move |ui| {
                            let label =
                                Label::new(ui, Rect::default(), "Counter: 0").expect("child label");
                            ui.set_layout(column![label.fill(1)]);
                            label_bounds.set(label.bounds());
                            child_client.set(ui.client_rect());
                            ChildLayoutApp { label, label_text }
                        },
                    )
                    .expect("child window");
                self.child = Some(handle.clone());
                handle.send(ChildLayoutMsg::SetText).expect("child alive");
                let timer = ui.set_timer(100).ok();
                if let Some(timer) = timer {
                    ui.on_timer(move |id| (id == timer).then_some(ParentLayoutMsg::Check));
                }
            }
            ParentLayoutMsg::Check => {
                self.child_alive
                    .set(self.child.as_ref().is_some_and(|child| child.is_alive()));
                ui.quit();
            }
        }
    }
}

/// A child lays out its own widgets at creation: a label in a column! layout is
/// sized and placed inside the child's client rect, a `send` delivers to its
/// app and changes the label, and it stays alive for its parent.
#[test]
fn child_lays_out_its_widgets_and_receives_sends() {
    let child_client = Rc::new(Cell::new(Rect::default()));
    let label_bounds = Rc::new(Cell::new(Rect::default()));
    let child_alive = Rc::new(Cell::new(false));
    let label_text = Rc::new(Cell::new(String::new()));

    let child_client_for_make = Rc::clone(&child_client);
    let label_bounds_for_make = Rc::clone(&label_bounds);
    let child_alive_for_make = Rc::clone(&child_alive);
    let label_text_for_make = Rc::clone(&label_text);

    let Some(run) = run_app_with_watchdog("win32ui.app.child.layout", move |ui| {
        ui.emit(ParentLayoutMsg::Start);
        ParentLayoutApp {
            child: None,
            child_client: child_client_for_make,
            label_bounds: label_bounds_for_make,
            child_alive: child_alive_for_make,
            label_text: label_text_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    assert!(
        child_alive.get(),
        "the child window was not alive after one loop turn"
    );
    assert_eq!(
        label_text.take(),
        "Counter: 1",
        "the sent message did not change the label text"
    );

    let client = child_client.get();
    let bounds = label_bounds.get();
    assert!(
        !bounds.is_empty(),
        "the label was not laid out (empty bounds)"
    );
    assert!(
        bounds.left >= client.left
            && bounds.top >= client.top
            && bounds.right <= client.right
            && bounds.bottom <= client.bottom,
        "the label ({bounds:?}) is outside the child client rect ({client:?})"
    );
}

/// Closing the owner destroys an owned child.
#[test]
fn closing_the_parent_destroys_the_child() {
    let child_hwnd = Rc::new(Cell::new(None));
    let child_hwnd_for_make = Rc::clone(&child_hwnd);
    let Some(run) = run_app_with_watchdog("win32ui.app.child.destroy", move |ui| {
        let handle = ui
            .open_window(
                WindowSpec::new("Child").size(dip(260.0), dip(120.0)),
                |_ui| NoopApp,
            )
            .expect("child window");
        child_hwnd_for_make.set(Some(handle.hwnd()));
        ui.emit(CloseMsg::Close);
        ParentCloseApp
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    if let Some(hwnd) = child_hwnd.get() {
        assert!(!hwnd.is_alive(), "the child survived the parent closing");
    }
}
