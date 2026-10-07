//! Tool-window and no-activate top-level windows: the roles requested in the
//! `WindowSpec` are applied, survive a fullscreen round trip and can be changed
//! at run time. A watchdog makes a stuck message loop fail instead of hang.

#![cfg(windows)]

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::run_app_spec_with_watchdog;
use win32ui::prelude::*;

/// `(tool_window, no_activate)` as read back from the window.
type Roles = (bool, bool);

/// What the probe saw at each step.
#[derive(Default)]
struct Seen {
    created: Option<Roles>,
    fullscreen: Option<Roles>,
    restored: Option<Roles>,
    cleared: Option<Roles>,
}

struct Probe {
    seen: Rc<RefCell<Seen>>,
}

fn roles(ui: &Ui<()>) -> Roles {
    (ui.is_tool_window(), ui.is_no_activate())
}

impl App for Probe {
    type Msg = ();

    fn update(&mut self, _msg: (), ui: &mut Ui<()>) {
        let mut seen = self.seen.borrow_mut();
        seen.created = Some(roles(ui));
        if let Some(monitor) = monitor_of(ui.hwnd()) {
            ui.enter_fullscreen(&monitor).expect("enter fullscreen");
            seen.fullscreen = Some(roles(ui));
            ui.leave_fullscreen().expect("leave fullscreen");
            seen.restored = Some(roles(ui));
        }
        ui.set_tool_window(false);
        ui.set_no_activate(false);
        seen.cleared = Some(roles(ui));
        ui.quit();
    }
}

/// The spec's roles are applied, kept through fullscreen, and clearable live.
#[test]
fn tool_window_and_no_activate_survive_fullscreen() {
    let seen = Rc::new(RefCell::new(Seen::default()));
    let spec = WindowSpec::new("win32ui.window_role")
        .theme(Theme::light())
        .tool_window(true)
        .no_activate(true);
    let probe_seen = Rc::clone(&seen);
    let Some(run) = run_app_spec_with_watchdog(spec, move |ui| {
        ui.emit(());
        Probe { seen: probe_seen }
    }) else {
        return;
    };
    assert!(!run.timed_out, "the watchdog fired before the app quit");

    let seen = seen.borrow();
    assert_eq!(seen.created, Some((true, true)), "the spec's roles");
    assert_eq!(
        seen.fullscreen,
        Some((true, true)),
        "roles while fullscreen"
    );
    assert_eq!(seen.restored, Some((true, true)), "roles after fullscreen");
    assert_eq!(seen.cleared, Some((false, false)), "roles cleared live");
}
