//! Platform-level window API: icons, placement, size limits, activation and
//! modal dialogs. The tests that need a message loop use the shared watchdog so
//! they fail instead of hanging; the rest are pure.

#![cfg(windows)]

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{run_app_spec_with_watchdog, run_with_watchdog};
use win32ui::prelude::*;
use win32ui::{App, Size, Ui, WindowSpec, dip};

/// A 2x2 opaque-red icon.
const RED_RGBA: [u8; 16] = [
    0xFF, 0x00, 0x00, 0xFF, 0xFF, 0x00, 0x00, 0xFF, 0xFF, 0x00, 0x00, 0xFF, 0xFF, 0x00, 0x00, 0xFF,
];

struct IconHandler {
    icon: Rc<RefCell<Option<Icon>>>,
}

impl WindowHandler for IconHandler {
    fn message(&self, window: &Window, message: Message) -> Option<LResult> {
        match message {
            Message::Create => {
                window.set_timer(1).ok();
            }
            Message::Timer { id } => {
                window.kill_timer(id);
                if let Ok(icon) = Icon::from_rgba(2, 2, &RED_RGBA) {
                    window.set_icon(&icon);
                    *self.icon.borrow_mut() = Some(icon);
                }
                window.destroy();
                win32ui::quit(0);
            }
            _ => {}
        }
        None
    }
}

#[test]
fn icon_is_built_and_installed() {
    let icon = Rc::new(RefCell::new(None));
    let Some(run) = run_with_watchdog("win32ui.icon", || IconHandler {
        icon: Rc::clone(&icon),
    }) else {
        return;
    };
    assert!(!run.timed_out, "the watchdog fired");
    let icon = icon.borrow();
    let icon = icon.as_ref().expect("CreateIconIndirect failed");
    assert_eq!(icon.size(), Size::new(2, 2));
}

#[test]
fn icon_rejects_bad_buffers() {
    assert!(Icon::from_rgba(0, 4, &[]).is_err());
    assert!(Icon::from_rgba(4, 4, &[0; 8]).is_err());
}

struct PlacementHandler {
    got: Rc<Cell<Option<Placement>>>,
}

impl WindowHandler for PlacementHandler {
    fn message(&self, window: &Window, message: Message) -> Option<LResult> {
        match message {
            Message::Create => {
                window.set_timer(1).ok();
            }
            Message::Timer { id } => {
                window.kill_timer(id);
                let target = Placement {
                    normal: Rect::new(120, 140, 520, 440),
                    show: ShowState::Normal,
                };
                let _ = window.set_placement(&target);
                self.got.set(Some(window.placement()));
                window.destroy();
                win32ui::quit(0);
            }
            _ => {}
        }
        None
    }
}

#[test]
fn placement_round_trips() {
    let got = Rc::new(Cell::new(None));
    let Some(run) = run_with_watchdog("win32ui.placement", || PlacementHandler {
        got: Rc::clone(&got),
    }) else {
        return;
    };
    assert!(!run.timed_out, "the watchdog fired");
    let placement = got.get().expect("the handler never ran");
    assert_eq!(placement.show, ShowState::Normal);
    assert_eq!(
        placement.normal.size(),
        Size::new(400, 300),
        "the normal bounds did not round-trip"
    );
}

#[test]
fn clamp_brings_an_off_screen_placement_back() {
    // `init` opts the process into per-monitor-v2 awareness; without it the
    // monitor rectangles come back in logical coordinates and can differ
    // between the two enumerations below.
    win32ui::init();
    let areas = monitor_work_areas();
    if areas.is_empty() {
        return;
    }

    let visible = |rect: Rect| {
        areas.iter().any(|area| {
            area.left < rect.right
                && rect.left < area.right
                && area.top < rect.bottom
                && rect.top < area.bottom
        })
    };

    let first = areas[0];
    let on_screen = Placement {
        normal: Rect::new(
            first.left + 10,
            first.top + 10,
            first.left + 110,
            first.top + 110,
        ),
        show: ShowState::Maximized,
    };
    assert_eq!(
        on_screen.clamp_to_work_areas(),
        on_screen,
        "a visible placement was moved"
    );

    let off_screen = Placement {
        normal: Rect::new(100_000, 100_000, 100_400, 100_300),
        show: ShowState::Normal,
    };
    let clamped = off_screen.clamp_to_work_areas();
    assert!(
        visible(clamped.normal),
        "the clamped placement is still off screen"
    );
}

struct OpsHandler {
    ran: Rc<Cell<bool>>,
}

impl WindowHandler for OpsHandler {
    fn message(&self, window: &Window, message: Message) -> Option<LResult> {
        match message {
            Message::Create => {
                window.set_timer(1).ok();
            }
            Message::Timer { id } => {
                window.kill_timer(id);
                window.set_cursor(CursorShape::Hand);
                window.set_cursor(CursorShape::SizeHorizontal);
                window.set_capture();
                window.release_capture();
                window.set_enabled(false);
                assert!(!window.is_enabled(), "the window stayed enabled");
                window.set_enabled(true);
                assert!(window.is_enabled(), "the window stayed disabled");
                window.focus();
                window.set_foreground();
                self.ran.set(true);
                window.destroy();
                win32ui::quit(0);
            }
            _ => {}
        }
        None
    }
}

#[test]
fn activation_and_cursor_calls_are_safe() {
    let ran = Rc::new(Cell::new(false));
    let Some(run) = run_with_watchdog("win32ui.ops", || OpsHandler {
        ran: Rc::clone(&ran),
    }) else {
        return;
    };
    assert!(!run.timed_out, "the watchdog fired");
    assert!(ran.get(), "the handler never ran");
}

struct PopupHandler {
    closed: Rc<Cell<bool>>,
}

impl WindowHandler for PopupHandler {
    fn message(&self, window: &Window, message: Message) -> Option<LResult> {
        match message {
            Message::Create => {
                window.set_timer(50).ok();
            }
            Message::Timer { id } => {
                window.kill_timer(id);
                self.closed.set(true);
                window.destroy();
            }
            _ => {}
        }
        None
    }
}

struct OwnerHandler {
    popup_closed: Rc<Cell<bool>>,
    modal_ran: Rc<Cell<bool>>,
    enabled_before: Rc<Cell<bool>>,
    enabled_after: Rc<Cell<bool>>,
}

impl WindowHandler for OwnerHandler {
    fn message(&self, window: &Window, message: Message) -> Option<LResult> {
        match message {
            Message::Create => {
                window.set_timer(1).ok();
            }
            Message::Timer { id } => {
                window.kill_timer(id);
                let theme = Theme::light();
                let Ok(class) = WindowClass::register("win32ui.modal.popup", theme.background)
                else {
                    window.destroy();
                    win32ui::quit(0);
                    return Some(0);
                };
                let Ok(popup) = Window::create_owned(
                    class,
                    window,
                    WindowStyle::new().popup(),
                    WindowExStyle::new(),
                    Rect::new(0, 0, 200, 150),
                    "popup",
                    PopupHandler {
                        closed: Rc::clone(&self.popup_closed),
                    },
                ) else {
                    window.destroy();
                    win32ui::quit(0);
                    return Some(0);
                };

                self.enabled_before.set(window.is_enabled());
                let _ = popup.run_modal(window);
                self.enabled_after.set(window.is_enabled());
                self.modal_ran.set(true);

                window.destroy();
                win32ui::quit(0);
            }
            _ => {}
        }
        None
    }
}

#[test]
fn modal_dialog_runs_and_reactivates_the_owner() {
    let popup_closed = Rc::new(Cell::new(false));
    let modal_ran = Rc::new(Cell::new(false));
    let enabled_before = Rc::new(Cell::new(false));
    let enabled_after = Rc::new(Cell::new(false));
    let Some(run) = run_with_watchdog("win32ui.modal.owner", || OwnerHandler {
        popup_closed: Rc::clone(&popup_closed),
        modal_ran: Rc::clone(&modal_ran),
        enabled_before: Rc::clone(&enabled_before),
        enabled_after: Rc::clone(&enabled_after),
    }) else {
        return;
    };
    assert!(!run.timed_out, "the watchdog fired");
    assert!(popup_closed.get(), "the popup's timer never fired");
    assert!(modal_ran.get(), "run_modal never returned");
    assert!(
        enabled_before.get(),
        "the owner was disabled before the modal"
    );
    assert!(
        enabled_after.get(),
        "the owner was not re-enabled after the modal"
    );
}

/// A widget-layer app that quits on its first message, so a placement probe
/// runs one loop pass and exits.
struct ProbeApp;

impl App for ProbeApp {
    type Msg = ();

    fn update(&mut self, _msg: (), ui: &mut Ui<()>) {
        ui.quit();
    }
}

/// Whether `rect` shares any area with a work area.
fn on_a_monitor(rect: Rect) -> bool {
    monitor_work_areas().iter().any(|area| {
        area.left < rect.right
            && rect.left < area.right
            && area.top < rect.bottom
            && rect.top < area.bottom
    })
}

#[test]
fn widget_windows_default_to_a_centred_placement() {
    win32ui::init();
    let observed = Rc::new(Cell::new(None));
    let handle = Rc::clone(&observed);
    let Some(run) = run_app_spec_with_watchdog(
        WindowSpec::new("win32ui.app.placement").size(dip(320.0), dip(240.0)),
        move |ui| {
            handle.set(Some(ui.placement()));
            ui.emit(());
            ProbeApp
        },
    ) else {
        return;
    };
    assert!(!run.timed_out, "the watchdog fired");
    let normal = observed.get().expect("make never ran").normal;
    assert!(
        normal.left != 0 || normal.top != 0,
        "the window opened at the primary monitor origin: {normal:?}"
    );
    assert!(
        on_a_monitor(normal),
        "the window is not on any monitor: {normal:?}"
    );
}

#[test]
fn ui_set_placement_overrides_the_default() {
    win32ui::init();
    let observed = Rc::new(Cell::new(None));
    let handle = Rc::clone(&observed);
    let target = Placement {
        normal: Rect::new(120, 140, 520, 440),
        show: ShowState::Normal,
    };
    let Some(run) = run_app_spec_with_watchdog(
        WindowSpec::new("win32ui.app.set_placement").size(dip(320.0), dip(240.0)),
        move |ui| {
            let _ = ui.set_placement(&target);
            handle.set(Some(ui.placement()));
            ui.emit(());
            ProbeApp
        },
    ) else {
        return;
    };
    assert!(!run.timed_out, "the watchdog fired");
    let placement = observed.get().expect("make never ran");
    assert_eq!(placement.show, ShowState::Normal);
    assert_eq!(
        placement.normal.size(),
        target.normal.size(),
        "the overridden normal bounds did not round-trip"
    );
}
