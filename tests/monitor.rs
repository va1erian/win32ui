//! Monitor enumeration and the `WM_DISPLAYCHANGE` notification.
//!
//! The enumeration tests need a desktop but no OpenGL; the `WM_DISPLAYCHANGE`
//! tests post the message themselves, so they do not rely on a real monitor
//! being unplugged. A watchdog makes a stuck message loop fail instead of hang.

#![cfg(windows)]

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::run_app_with_watchdog;
use win32ui::prelude::*;

/// Every monitor has a device name, sane rectangles and a primary exactly once.
#[test]
fn monitors_report_the_desktop() {
    win32ui::init();

    let monitors = monitors();
    assert!(!monitors.is_empty(), "no monitors were reported");
    for monitor in &monitors {
        assert!(
            monitor.device_name.starts_with("\\\\.\\DISPLAY"),
            "unexpected device name {:?}",
            monitor.device_name
        );
        assert!(!monitor.rect.is_empty(), "empty rect {:?}", monitor.rect);
        assert!(
            !monitor.work_area.is_empty(),
            "empty work area {:?}",
            monitor.work_area
        );
        assert!(
            monitor.work_area.width() <= monitor.rect.width()
                && monitor.work_area.height() <= monitor.rect.height(),
            "work area {:?} is larger than rect {:?}",
            monitor.work_area,
            monitor.rect
        );
        assert!(
            (48..=960).contains(&monitor.dpi),
            "suspicious dpi {}",
            monitor.dpi
        );
    }
    assert_eq!(
        monitors.iter().filter(|monitor| monitor.primary).count(),
        1,
        "exactly one monitor should be primary"
    );
}

/// A handler that quits the loop on the first paint, so the helper returns
/// without waiting for the watchdog.
struct QuitOnPaint;

impl WindowHandler for QuitOnPaint {
    fn message(&self, _window: &Window, message: Message) -> Option<LResult> {
        if matches!(message, Message::Paint) {
            win32ui::quit(0);
            return Some(0);
        }
        None
    }
}

/// `monitor_of` and `Window::monitor` report the monitor a window is on.
#[test]
fn monitor_of_reports_the_windows_monitor() {
    let Some(run) = common::run_with_watchdog("win32ui.monitor.of", || QuitOnPaint) else {
        return;
    };
    assert!(!run.timed_out, "the watchdog fired before the app quit");

    let monitor = monitor_of(run.window.hwnd()).expect("the window's monitor");
    assert!(
        overlaps(monitor.rect, run.window.window_rect()),
        "the reported monitor {:?} does not overlap the window {:?}",
        monitor.rect,
        run.window.window_rect()
    );
    let via_window = run.window.monitor().expect("Window::monitor");
    assert_eq!(
        via_window, monitor,
        "Window::monitor disagrees with monitor_of"
    );
}

/// A handler that records the decoded `WM_DISPLAYCHANGE` posted to it.
struct DisplayProbe {
    got: Rc<RefCell<Option<(u32, u32, u32)>>>,
    posted: Cell<bool>,
}

impl WindowHandler for DisplayProbe {
    fn message(&self, window: &Window, message: Message) -> Option<LResult> {
        match message {
            Message::Paint => {
                if !self.posted.replace(true) {
                    // SAFETY: `window` is the live window this handler is
                    // serving; posting to it only enqueues a message.
                    let _ = window.post_message(DISPLAY_CHANGE, 32, 1920 | (1080 << 16));
                }
                Some(0)
            }
            Message::DisplayChange {
                width,
                height,
                bits_per_pixel,
            } => {
                self.got.replace(Some((width, height, bits_per_pixel)));
                window.destroy();
                win32ui::quit(0);
                Some(0)
            }
            _ => None,
        }
    }
}

/// `WM_DISPLAYCHANGE` decodes into `Message::DisplayChange` with the packed
/// width/height and the bits per pixel.
#[test]
fn display_change_decodes_width_height_and_depth() {
    let got = Rc::new(RefCell::new(None));
    let got_for_handler = Rc::clone(&got);
    let Some(run) = common::run_with_watchdog("win32ui.monitor.change", move || DisplayProbe {
        got: got_for_handler,
        posted: Cell::new(false),
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    assert_eq!(
        got.borrow().as_ref().copied(),
        Some((1920, 1080, 32)),
        "WM_DISPLAYCHANGE did not decode as expected"
    );
}

enum ChangeMsg {
    Post,
    Changed,
}

struct ChangeApp {
    got: Rc<Cell<bool>>,
}

impl App for ChangeApp {
    type Msg = ChangeMsg;

    fn update(&mut self, msg: ChangeMsg, ui: &mut Ui<ChangeMsg>) {
        match msg {
            ChangeMsg::Post => {
                // SAFETY: `ui.hwnd()` is the live app window; posting to it only
                // enqueues a message.
                let hwnd =
                    windows::Win32::Foundation::HWND(ui.hwnd().raw() as *mut core::ffi::c_void);
                let _ = unsafe {
                    windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                        Some(hwnd),
                        DISPLAY_CHANGE,
                        windows::Win32::Foundation::WPARAM(32),
                        windows::Win32::Foundation::LPARAM(1280 | (720 << 16)),
                    )
                };
            }
            ChangeMsg::Changed => {
                self.got.set(true);
                ui.quit();
            }
        }
    }
}

/// `Ui::on_display_change` maps the notification to an app message.
#[test]
fn ui_maps_display_change_to_a_message() {
    let got = Rc::new(Cell::new(false));
    let got_for_make = Rc::clone(&got);
    let Some(run) = run_app_with_watchdog("win32ui.monitor.ui", move |ui| {
        ui.on_display_change(|| Some(ChangeMsg::Changed));
        let timer = ui.set_timer(150).ok();
        ui.on_timer(move |id| (Some(id) == timer).then_some(ChangeMsg::Post));
        ChangeApp { got: got_for_make }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    assert!(
        got.get(),
        "Ui::on_display_change did not map WM_DISPLAYCHANGE to a message"
    );
}

/// `WM_DISPLAYCHANGE`, mirrored from `Winuser.h`, for tests that post it.
const DISPLAY_CHANGE: u32 = 0x007E;

/// Whether the two rectangles share any area.
fn overlaps(a: Rect, b: Rect) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}
