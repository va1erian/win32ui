//! The slider driven by synthetic input: the value mapping at both ends and far
//! outside, mouse capture, coalesced events, `set_value` during a drag,
//! keyboard and wheel steps, orientation, the disabled state and the idle
//! timer. Input is sent with `SendMessageW`, so the real window procedure,
//! painting and event queue are exercised.

#![cfg(windows)]

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use common::run_app_with_watchdog;
use win32ui::column;
use win32ui::prelude::*;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetCapture, VK_END, VK_HOME, VK_LEFT, VK_NEXT, VK_PRIOR, VK_RIGHT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN, SendMessageW, SetCursorPos, WM_KEYDOWN,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_PAINT,
};

const MAX: f64 = 1000.0;
/// The padding at each end of the track, in dip (`END_PADDING` in the widget).
const PAD: f64 = 12.0;

#[derive(Clone, Debug, PartialEq)]
enum Msg {
    Start,
    Change(f64),
    Commit(f64),
    Hover(f64),
    Late,
}

/// What a test's script does on each message; it returns `true` to finish.
type Script = Box<dyn Fn(&Rig, &Msg, &Ui<Msg>) -> bool>;

/// A slider in a window plus the helpers that drive it.
struct Rig {
    slider: Slider<Msg>,
    dpi: u32,
}

impl Rig {
    fn hwnd(&self) -> HWND {
        HWND(self.slider.hwnd().raw() as *mut core::ffi::c_void)
    }

    /// Client x for `dip` along the horizontal track, in device pixels.
    fn px(&self, dip: f64) -> i32 {
        (dip * f64::from(self.dpi) / 96.0).round() as i32
    }

    /// The x of `fraction` of a horizontal track, in device pixels.
    fn at(&self, fraction: f64) -> i32 {
        let width = f64::from(self.slider.bounds().width()) * 96.0 / f64::from(self.dpi);
        self.px(PAD + fraction * (width - 2.0 * PAD))
    }

    fn send(&self, message: u32, wparam: usize, x: i32, y: i32) {
        let lparam = ((y as i16 as u16 as isize) << 16) | (x as i16 as u16 as isize);
        // SAFETY: a synchronous message to the slider's own live window.
        unsafe {
            SendMessageW(
                self.hwnd(),
                message,
                Some(WPARAM(wparam)),
                Some(LPARAM(lparam)),
            );
        }
    }

    fn down(&self, x: i32) {
        self.send(WM_LBUTTONDOWN, 1, x, 10);
    }

    fn drag(&self, x: i32) {
        self.send(WM_MOUSEMOVE, 1, x, 10);
    }

    fn up(&self, x: i32) {
        self.send(WM_LBUTTONUP, 0, x, 10);
    }

    fn key(&self, key: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY) {
        self.send(WM_KEYDOWN, usize::from(key.0), 1, 0);
    }

    fn value(&self) -> f64 {
        self.slider.current_value()
    }
}

/// Runs `script` against a horizontal slider over `0.0..=MAX` in a window,
/// returning every message the app received. `None` when the session cannot
/// create windows.
/// Parks the real pointer in the far corner of the primary screen. These tests
/// inject their own `WM_MOUSEMOVE`s and count the resulting events; a real pointer
/// resting over the test window (a CI runner, a developer at the machine) would
/// add genuine moves and hovers to the log. Harmless when there is no interactive
/// desktop.
fn park_real_pointer() {
    // SAFETY: plain integer arguments; a failure (no interactive desktop) is ignored.
    unsafe {
        let _ = SetCursorPos(
            GetSystemMetrics(SM_CXSCREEN) - 1,
            GetSystemMetrics(SM_CYSCREEN) - 1,
        );
    }
}

fn run(name: &str, configure: fn(Slider<Msg>) -> Slider<Msg>, script: Script) -> Option<Vec<Msg>> {
    park_real_pointer();
    let log = Rc::new(RefCell::new(Vec::new()));
    let log_for_make = Rc::clone(&log);
    let run = run_app_with_watchdog(name, move |ui| {
        let slider = configure(
            Slider::new(ui, 0.0..=MAX)
                .expect("slider")
                .on_change(|v| Some(Msg::Change(v)))
                .on_commit(|v| Some(Msg::Commit(v)))
                .on_hover(|v| Some(Msg::Hover(v))),
        );
        ui.set_layout(column![slider.fill(1)]);
        let proxy = ui.proxy();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            let _ = proxy.send(Msg::Late);
        });
        ui.emit(Msg::Start);
        Harness {
            rig: Rig {
                slider,
                dpi: ui.dpi(),
            },
            log: log_for_make,
            script,
        }
    })?;
    assert!(!run.timed_out, "the watchdog fired before the script ended");
    let log = log.borrow().clone();
    Some(log)
}

struct Harness {
    rig: Rig,
    log: Rc<RefCell<Vec<Msg>>>,
    script: Script,
}

impl App for Harness {
    type Msg = Msg;

    fn update(&mut self, msg: Msg, ui: &mut Ui<Msg>) {
        self.log.borrow_mut().push(msg.clone());
        if (self.script)(&self.rig, &msg, ui) {
            ui.quit();
        }
    }
}

fn plain(slider: Slider<Msg>) -> Slider<Msg> {
    slider
}

#[test]
fn a_drag_maps_both_ends_and_far_outside_with_the_mouse_captured() {
    let script: Script = Box::new(|rig, msg, _| {
        if *msg != Msg::Start {
            return matches!(msg, Msg::Commit(_));
        }
        rig.down(rig.at(0.5));
        assert_eq!(rig.value(), MAX / 2.0, "click jumps to the pointer");
        assert!(rig.slider.is_dragging());
        // SAFETY: reads the calling thread's capture window.
        assert_eq!(
            unsafe { GetCapture() },
            rig.hwnd(),
            "the drag captures the mouse"
        );
        rig.drag(30_000);
        assert_eq!(rig.value(), MAX, "far right");
        rig.drag(-500);
        assert_eq!(rig.value(), 0.0, "far left");
        rig.drag(rig.at(1.0));
        assert_eq!(rig.value(), MAX, "the right end");
        rig.up(rig.at(1.0));
        assert!(!rig.slider.is_dragging());
        // SAFETY: as above.
        assert_ne!(
            unsafe { GetCapture() },
            rig.hwnd(),
            "release frees the mouse"
        );
        false
    });
    let Some(log) = run("win32ui.slider.ends", plain, script) else {
        return;
    };
    assert_eq!(log.last(), Some(&Msg::Commit(MAX)));
}

#[test]
fn moves_within_a_frame_produce_one_change_and_a_commit() {
    let script: Script = Box::new(|rig, msg, _| {
        match msg {
            Msg::Start => {
                rig.down(rig.at(0.0));
                for step in 1..=50 {
                    rig.drag(rig.at(f64::from(step) / 100.0));
                }
            }
            Msg::Change(_) => rig.up(rig.at(0.5)),
            _ => {}
        }
        matches!(msg, Msg::Commit(_))
    });
    let Some(log) = run("win32ui.slider.coalesce", plain, script) else {
        return;
    };
    let changes: Vec<_> = log.iter().filter(|m| matches!(m, Msg::Change(_))).collect();
    assert_eq!(changes.len(), 1, "51 inputs, one on_change: {log:?}");
    let Msg::Change(value) = changes[0] else {
        unreachable!()
    };
    assert!((0.0..=MAX).contains(value), "a value in range: {value}");
    assert!(matches!(log.last(), Some(Msg::Commit(_))));
}

#[test]
fn set_value_is_ignored_while_dragging_and_applied_after() {
    let script: Script = Box::new(|rig, msg, _| {
        if *msg != Msg::Start {
            return matches!(msg, Msg::Commit(_));
        }
        rig.down(rig.at(0.25));
        rig.slider.set_value(900.0);
        assert_eq!(
            rig.value(),
            MAX / 4.0,
            "the widget owns the value while dragging"
        );
        rig.up(rig.at(0.25));
        rig.slider.set_value(900.0);
        assert_eq!(rig.value(), 900.0, "applied once the drag is over");
        false
    });
    run("win32ui.slider.playback", plain, script);
}

#[test]
fn keys_and_wheel_step_in_value_units_and_commit() {
    fn steps(slider: Slider<Msg>) -> Slider<Msg> {
        slider.key_steps(1.0, 100.0).wheel_step(50.0)
    }
    let script: Script = Box::new(|rig, msg, _| {
        if *msg == Msg::Start {
            rig.slider.set_value(500.0);
            rig.key(VK_RIGHT);
            assert_eq!(rig.value(), 501.0);
            rig.key(VK_LEFT);
            rig.key(VK_LEFT);
            assert_eq!(rig.value(), 499.0);
            rig.key(VK_PRIOR);
            assert_eq!(rig.value(), 599.0);
            rig.key(VK_NEXT);
            assert_eq!(rig.value(), 499.0);
            rig.key(VK_END);
            assert_eq!(rig.value(), MAX);
            rig.key(VK_HOME);
            assert_eq!(rig.value(), 0.0);
            rig.send(WM_MOUSEWHEEL, 120 << 16, 0, 0);
            assert_eq!(rig.value(), 50.0, "one notch up");
            rig.send(WM_MOUSEWHEEL, (-120i16 as u16 as usize) << 16, 0, 0);
            assert_eq!(rig.value(), 0.0, "one notch down");
        }
        matches!(msg, Msg::Commit(v) if *v == 0.0)
    });
    let Some(log) = run("win32ui.slider.keys", steps, script) else {
        return;
    };
    let commits = log.iter().filter(|m| matches!(m, Msg::Commit(_))).count();
    assert_eq!(
        commits, 9,
        "every keyboard step and wheel notch commits: {log:?}"
    );
}

#[test]
fn hovering_reports_the_value_under_the_pointer() {
    let script: Script = Box::new(|rig, msg, _| {
        if *msg == Msg::Start {
            for step in 0..20 {
                rig.send(WM_MOUSEMOVE, 0, rig.at(0.1 + f64::from(step) / 100.0), 10);
            }
            rig.send(WM_MOUSEMOVE, 0, rig.at(0.5), 10);
            // The real pointer is elsewhere, so Windows queues a WM_MOUSELEAVE
            // for these moves; painting now flushes the hover before it is handled.
            rig.send(WM_PAINT, 0, 0, 0);
        }
        *msg == Msg::Late
    });
    let Some(log) = run("win32ui.slider.hover", plain, script) else {
        return;
    };
    let hovers: Vec<_> = log.iter().filter(|m| matches!(m, Msg::Hover(_))).collect();
    assert_eq!(hovers, [&Msg::Hover(MAX / 2.0)], "21 moves, one on_hover");
    assert!(
        !log.iter()
            .any(|m| matches!(m, Msg::Change(_) | Msg::Commit(_)))
    );
}

#[test]
fn a_disabled_slider_ignores_input() {
    let script: Script = Box::new(|rig, msg, _| {
        if *msg == Msg::Start {
            rig.slider.set_enabled(false);
            rig.down(rig.at(0.5));
            assert!(!rig.slider.is_dragging());
            assert_eq!(rig.value(), 0.0);
            rig.key(VK_END);
            assert_eq!(rig.value(), 0.0);
        }
        *msg == Msg::Late
    });
    let Some(log) = run("win32ui.slider.disabled", plain, script) else {
        return;
    };
    assert!(
        !log.iter()
            .any(|m| matches!(m, Msg::Change(_) | Msg::Commit(_)))
    );
}

#[test]
fn vertical_puts_the_minimum_at_the_bottom_and_rtl_at_the_right() {
    fn vertical(slider: Slider<Msg>) -> Slider<Msg> {
        slider.vertical()
    }
    let script: Script = Box::new(|rig, msg, _| {
        if *msg == Msg::Start {
            // The layout gives the slider the full window height, so the
            // vertical track spans the widget's own height.
            let height = rig.slider.bounds().height();
            rig.send(WM_LBUTTONDOWN, 1, 10, height + 4000);
            assert_eq!(rig.value(), 0.0, "below the bottom is the minimum");
            rig.send(WM_MOUSEMOVE, 1, 10, -4000);
            assert_eq!(rig.value(), MAX, "above the top is the maximum");
            rig.up(0);
        }
        matches!(msg, Msg::Commit(_))
    });
    run("win32ui.slider.vertical", vertical, script);

    fn rtl(slider: Slider<Msg>) -> Slider<Msg> {
        slider.right_to_left(true)
    }
    let script: Script = Box::new(|rig, msg, _| {
        if *msg == Msg::Start {
            rig.down(rig.at(0.0));
            assert_eq!(rig.value(), MAX, "the left end is the maximum");
            rig.drag(rig.at(1.0));
            assert_eq!(rig.value(), 0.0, "the right end is the minimum");
            rig.up(0);
        }
        matches!(msg, Msg::Commit(_))
    });
    run("win32ui.slider.rtl", rtl, script);
}

#[test]
fn the_easing_timer_runs_only_while_animating() {
    let script: Script = Box::new(|rig, msg, _| match msg {
        Msg::Start => {
            assert!(!rig.slider.is_animating(), "idle is timer-free");
            rig.slider.set_value(10.0);
            rig.slider.set_buffered(0.0..20.0);
            assert!(!rig.slider.is_animating(), "set_value animates nothing");
            rig.down(rig.at(0.5));
            rig.up(rig.at(0.5));
            false
        }
        Msg::Late => {
            assert!(
                !rig.slider.is_animating(),
                "the timer stopped after the easing"
            );
            true
        }
        _ => false,
    });
    run("win32ui.slider.idle", plain, script);
}

/// The thumb is drawn at fractional pixel positions: a value a quarter of a
/// device pixel away paints different (anti-aliased) pixels.
#[test]
fn a_sub_pixel_value_change_repaints_the_thumb_edge() {
    let script: Script = Box::new(|rig, msg, ui| {
        if *msg != Msg::Late {
            return false;
        }
        let track_px =
            f64::from(rig.slider.bounds().width()) - 2.0 * 12.0 * f64::from(rig.dpi) / 96.0;
        let quarter_pixel = MAX / track_px / 4.0;
        let shot = |value: f64| {
            rig.slider.set_value(value);
            rig.send(WM_PAINT, 0, 0, 0);
            ui.capture().expect("capture")
        };
        let before = shot(MAX / 2.0);
        let after = shot(MAX / 2.0 + quarter_pixel);
        assert_ne!(
            before, after,
            "a quarter-pixel move must change some pixels"
        );
        let differing = before
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .zip(after.pixels.as_chunks::<4>().0.iter())
            .filter(|(a, b)| a != b)
            .count();
        assert!(
            differing < 400,
            "only the thumb's edge moves: {differing} pixels"
        );
        true
    });
    run("win32ui.slider.subpixel", plain, script);
}
