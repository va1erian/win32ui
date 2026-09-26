//! The window backdrop material and themed caption: spec wiring, the
//! `backdrop_active` report, and that backdrop-aware widgets build.
//!
//! The material itself is a machine-dependent DWM feature (unsupported on
//! Windows 10, off in high-contrast mode), so these tests assert the fallback
//! contract rather than that Mica is on. The pure fallback rule is unit-tested
//! in `sys::dwm`.
//!
//! Window-creating tests use the shared watchdog helper so failures fail
//! instead of hanging.

#![cfg(windows)]

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::run_app_spec_with_watchdog;
use win32ui::prelude::*;

/// The default spec requests no material, so `backdrop_active` is `false`.
#[test]
fn default_spec_reports_no_backdrop() {
    struct App {
        active: Rc<Cell<bool>>,
    }

    impl win32ui::App for App {
        type Msg = ();
        fn update(&mut self, _msg: (), ui: &mut Ui<()>) {
            self.active.set(ui.backdrop_active());
            ui.quit();
        }
    }

    let active = Rc::new(Cell::new(true));
    let active_for_make = Rc::clone(&active);
    let Some(run) = run_app_spec_with_watchdog(WindowSpec::new("backdrop.none"), move |ui| {
        ui.emit(());
        App {
            active: active_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    assert!(!active.get(), "Backdrop::None must not report as active");
}

/// Requesting Mica and a themed caption builds and reports a bool. On a machine
/// that supports the material this exercises the active path; on one that does
/// not (Windows 10, high contrast, transparency off) it exercises the fallback.
#[test]
fn mica_and_colored_caption_build() {
    struct App;

    impl win32ui::App for App {
        type Msg = ();
        fn update(&mut self, _msg: (), ui: &mut Ui<()>) {
            ui.quit();
        }
    }

    let Some(run) = run_app_spec_with_watchdog(
        WindowSpec::new("backdrop.mica")
            .backdrop(Backdrop::Mica)
            .title_bar(TitleBar::Colored),
        move |ui| {
            let _ = ui.backdrop_active();
            ui.emit(());
            App
        },
    ) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
}

/// Requesting an accent-tinted material builds and reports a bool. The tint is
/// machine-dependent (it needs the undocumented `user32` export, transparency
/// on and no high contrast), so the test asserts the spec path runs rather than
/// that the tint is on.
#[test]
fn accent_tinted_backdrop_builds() {
    struct App;

    impl win32ui::App for App {
        type Msg = ();
        fn update(&mut self, _msg: (), ui: &mut Ui<()>) {
            let _ = ui.backdrop_active();
            ui.quit();
        }
    }

    let Some(run) = run_app_spec_with_watchdog(
        WindowSpec::new("backdrop.accent")
            .backdrop(Backdrop::Acrylic)
            .accent_tint(true)
            .title_bar(TitleBar::Extended),
        move |ui| {
            let _ = ui.backdrop_active();
            ui.emit(());
            App
        },
    ) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
}
/// misses. With an extended title bar, the caption-button region — read from
/// `DWMWA_CAPTION_BUTTON_BOUNDS` and aligned with the image through
/// `DWMWA_EXTENDED_FRAME_BOUNDS` — must contain real button glyphs (more than
/// one colour) and differ from the `PrintWindow` image of the same window.
///
/// The backdrop material is machine-dependent, so the test does not require it:
/// the caption buttons of an extended frame are drawn by DWM on any Windows
/// 10/11, material or not. Read-only — no focus or pointer movement. Skipped
/// when the platform cannot create a window, when `Windows.Graphics.Capture` is
/// unavailable, or when the capture device was lost.
#[cfg(feature = "wgc")]
#[test]
fn composited_capture_includes_dwm_caption_chrome() {
    use std::cell::RefCell;
    use std::rc::Rc;

    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Dwm::{
        DWM_WINDOW_CORNER_PREFERENCE, DWMWA_CAPTION_BUTTON_BOUNDS, DWMWA_EXTENDED_FRAME_BOUNDS,
        DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DWMWINDOWATTRIBUTE,
        DwmGetWindowAttribute,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;

    /// The DWM geometry needed to locate the caption buttons in a capture.
    #[derive(Clone, Copy)]
    struct Chrome {
        /// The caption-button rectangle, in window-relative coordinates.
        button: Rect,
        /// `GetWindowRect` minus `DWMWA_EXTENDED_FRAME_BOUNDS`: the offset
        /// between the `PrintWindow` and composited image origins.
        inset: (i32, i32),
        /// `DWMWA_WINDOW_CORNER_PREFERENCE`, when the OS reports it.
        corner_preference: Option<i32>,
    }

    /// A DWM rectangle attribute, or `None` if DWM rejects it.
    fn dwm_rect(raw: HWND, attribute: DWMWINDOWATTRIBUTE) -> Option<Rect> {
        let mut rect = RECT::default();
        // SAFETY: `raw` is the live window; `rect` is a correctly-sized
        // out-struct for the attribute.
        unsafe {
            DwmGetWindowAttribute(
                raw,
                attribute,
                &mut rect as *mut _ as *mut core::ffi::c_void,
                size_of::<RECT>() as u32,
            )
        }
        .is_ok()
        .then(|| Rect::new(rect.left, rect.top, rect.right, rect.bottom))
    }

    fn read_chrome(hwnd: Hwnd) -> Option<Chrome> {
        let raw = HWND(hwnd.raw() as *mut core::ffi::c_void);
        let button = dwm_rect(raw, DWMWA_CAPTION_BUTTON_BOUNDS)?;
        let extended = dwm_rect(raw, DWMWA_EXTENDED_FRAME_BOUNDS)?;
        let mut window = RECT::default();
        // SAFETY: `raw` is live; `window` is a valid out-pointer.
        if unsafe { GetWindowRect(raw, &mut window) }.is_err() {
            return None;
        }
        let mut preference = DWM_WINDOW_CORNER_PREFERENCE(0);
        // SAFETY: `raw` is live; `preference` is a correctly-sized out-struct.
        let corner_preference = unsafe {
            DwmGetWindowAttribute(
                raw,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &mut preference as *mut _ as *mut core::ffi::c_void,
                size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
            )
        }
        .is_ok()
        .then_some(preference.0);
        Some(Chrome {
            button,
            inset: (window.left - extended.left, window.top - extended.top),
            corner_preference,
        })
    }

    #[derive(Clone)]
    enum Msg {
        Go,
        Capture,
    }

    struct App {
        composited: Rc<RefCell<Option<Result<RgbaImage>>>>,
        printed: Rc<RefCell<Option<RgbaImage>>>,
        chrome: Rc<RefCell<Option<Chrome>>>,
    }

    impl win32ui::App for App {
        type Msg = Msg;

        fn update(&mut self, msg: Msg, ui: &mut Ui<Msg>) {
            if let Msg::Capture = msg {
                *self.composited.borrow_mut() = Some(ui.capture_composited());
                *self.printed.borrow_mut() = ui.capture().ok();
                *self.chrome.borrow_mut() = read_chrome(ui.hwnd());
                ui.quit();
            }
        }
    }

    let composited = Rc::new(RefCell::new(None));
    let printed = Rc::new(RefCell::new(None));
    let chrome = Rc::new(RefCell::new(None));
    let (c, p, ch) = (
        Rc::clone(&composited),
        Rc::clone(&printed),
        Rc::clone(&chrome),
    );

    let Some(run) = run_app_spec_with_watchdog(
        WindowSpec::new("capture.caption")
            .backdrop(Backdrop::Acrylic)
            .title_bar(TitleBar::Extended),
        move |ui| {
            // Let DWM compose the frame and its caption before capturing.
            let id = ui.set_timer(600).ok();
            ui.on_timer(move |fired| (Some(fired) == id).then_some(Msg::Capture));
            ui.emit(Msg::Go);
            App {
                composited: c,
                printed: p,
                chrome: ch,
            }
        },
    ) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    let composited = composited
        .borrow_mut()
        .take()
        .expect("the capture timer never fired");
    let composited = match composited {
        Ok(image) => image,
        // A machine without a usable capture device cannot run this test; the
        // crate's other wgc tests skip on the same errors.
        Err(Error::Capture(CaptureError::Unavailable | CaptureError::DeviceLost)) => return,
        Err(error) => panic!("the composited capture failed: {error:?}"),
    };

    let Some(printed) = printed.borrow_mut().take() else {
        return;
    };
    let Some(chrome) = chrome.borrow_mut().take() else {
        eprintln!("skipping: DWM did not report the caption geometry");
        return;
    };

    // The button rectangle is window-relative; the image starts at the extended
    // frame origin, so shift by the window/extended-frame origin difference.
    let (inset_x, inset_y) = chrome.inset;
    let button = chrome.button;
    let mut colours = std::collections::BTreeSet::new();
    let mut differing = 0usize;
    for y in button.top..button.bottom {
        for x in button.left..button.right {
            let (ix, iy) = (x + inset_x, y + inset_y);
            if ix < 0 || iy < 0 {
                continue;
            }
            let composited_pixel = composited.pixel(ix as u32, iy as u32);
            let printed_pixel = printed.pixel(x as u32, y as u32);
            if let Some(pixel) = composited_pixel {
                colours.insert([pixel[0], pixel[1], pixel[2], pixel[3]]);
            }
            if composited_pixel != printed_pixel {
                differing += 1;
            }
        }
    }
    assert!(
        colours.len() > 2,
        "the caption-button region of the composited capture is flat; DWM's \
         min/max/close glyphs are missing ({} colours)",
        colours.len()
    );
    assert!(
        differing > 0,
        "the composited capture's caption-button region must differ from PrintWindow's"
    );

    // Rounded corners are the visible evidence that the composited capture kept
    // (and un-premultiplied) the alpha channel. They only exist when DWM
    // reports a corner preference other than "do not round"; older Windows does
    // not report the attribute at all. GitHub's Windows Server runners report
    // the default preference but draw square, opaque corners, so the corner
    // assertion is skipped there (`GITHUB_ACTIONS`); it runs on a desktop.
    let rounded = chrome
        .corner_preference
        .is_some_and(|value| value != DWMWCP_DONOTROUND.0)
        && std::env::var_os("GITHUB_ACTIONS").is_none();
    if rounded {
        let corners = [
            (0, 0),
            (composited.width - 1, 0),
            (0, composited.height - 1),
            (composited.width - 1, composited.height - 1),
        ];
        assert!(
            corners
                .iter()
                .any(|&(x, y)| composited.pixel(x, y).is_some_and(|p| p[3] < 255)),
            "a rounded window's corner pixels must be non-opaque"
        );
    }
}
