//! Regression check for the material surface (strip menu / status bar) staying
//! painted across maximize, minimize and restore.
//!
//! Os the window is resized or restored, DWM re-creates the frame; the
//! top-level transparent Direct2D surface and its child controls must repaint
//! themselves without waiting for a click. The test drives the state changes
//! and captures the window with `PrintWindow`, then asserts the client is not
//! left showing the bare frame (near-black).
//!
//! Opt-in (`#[ignore]`) because it shows real windows and takes the message
//! loop; run with `cargo test --test status_material -- --ignored --nocapture`.

#![cfg(windows)]

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use win32ui::prelude::*;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    SHOW_WINDOW_CMD, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE, ShowWindow,
};

enum Msg {
    Window(SHOW_WINDOW_CMD),
    Capture(&'static str),
    Foreground,
    HitButtons,
    Quit,
}

struct App {
    shots: Rc<RefCell<Vec<(&'static str, RgbaImage)>>>,
    /// The hit-test codes returned over the caption buttons (min, max, close).
    hits: Rc<RefCell<Vec<isize>>>,
}

/// Sends `WM_NCHITTEST` at the centre of each caption button and returns the
/// hit codes, to prove DWM's buttons still respond when maximized.
fn caption_button_hits(hwnd: HWND) -> Vec<isize> {
    use windows::Win32::Graphics::Dwm::{DWMWA_CAPTION_BUTTON_BOUNDS, DwmGetWindowAttribute};
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, SendMessageW, WM_NCHITTEST};

    let mut bounds = windows::Win32::Foundation::RECT::default();
    // SAFETY: `hwnd` is live and `bounds` is a correctly-sized out-struct.
    if unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CAPTION_BUTTON_BOUNDS,
            &mut bounds as *mut _ as *mut core::ffi::c_void,
            size_of::<windows::Win32::Foundation::RECT>() as u32,
        )
    }
    .is_err()
    {
        return Vec::new();
    }
    let mut window = windows::Win32::Foundation::RECT::default();
    // SAFETY: `window` is a valid out-pointer.
    let _ = unsafe { GetWindowRect(hwnd, &mut window) };
    let width = bounds.right - bounds.left;
    let height = bounds.bottom - bounds.top;
    let mut hits = Vec::with_capacity(3);
    for fraction in [1, 3, 5] {
        let x = window.left + bounds.left + width * fraction / 6;
        let y = window.top + bounds.top + height / 2;
        let lparam = windows::Win32::Foundation::LPARAM(
            (((y as i16 as u16) as isize) << 16) | ((x as i16 as u16) as isize),
        );
        // SAFETY: `hwnd` is live; a `WM_NCHITTEST` with a packed point is the
        // documented contract.
        let result = unsafe {
            SendMessageW(
                hwnd,
                WM_NCHITTEST,
                Some(windows::Win32::Foundation::WPARAM(0)),
                Some(lparam),
            )
        };
        hits.push(result.0);
    }
    hits
}

impl win32ui::App for App {
    type Msg = Msg;

    fn update(&mut self, msg: Msg, ui: &mut Ui<Msg>) {
        match msg {
            Msg::Window(command) => {
                let hwnd = HWND(ui.hwnd().raw() as *mut core::ffi::c_void);
                // SAFETY: `hwnd` is the live window; the command is one of the
                // documented show commands.
                unsafe {
                    let _ = ShowWindow(hwnd, command);
                }
            }
            Msg::Capture(tag) => {
                if let Ok(image) = ui.capture() {
                    self.shots.borrow_mut().push((tag, image));
                }
            }
            Msg::Foreground => ui.set_foreground(),
            Msg::HitButtons => {
                let hwnd = HWND(ui.hwnd().raw() as *mut core::ffi::c_void);
                let hits = caption_button_hits(hwnd);
                self.hits.borrow_mut().extend(hits);
            }
            Msg::Quit => ui.quit(),
        }
    }
}

/// The mean brightness of the window centre (a plausible client pixel).
fn centre(image: &RgbaImage) -> u32 {
    let [r, g, b, _] = image
        .pixel(image.width / 2, image.height / 2)
        .unwrap_or([0; 4]);
    u32::from(r) + u32::from(g) + u32::from(b)
}

fn spec() -> WindowSpec {
    WindowSpec::new("status-material.states")
        .theme(Theme::dark())
        .backdrop(Backdrop::Acrylic)
        .title_bar(TitleBar::Extended)
}

#[test]
#[ignore = "shows real windows and runs the message loop"]
fn material_surface_survives_maximize_minimize_restore() {
    let shots = Rc::new(RefCell::new(Vec::new()));
    let shots_for_app = Rc::clone(&shots);
    let hits = Rc::new(RefCell::new(Vec::new()));
    let hits_for_app = Rc::clone(&hits);
    let Some(run) = common::run_app_spec_with_watchdog(spec(), move |ui| {
        let label = Label::new(ui, Rect::new(0, 0, 400, 300), "client").expect("label");
        ui.set_layout(win32ui::column![label].spacing(dip(0.0)));
        let status = MaterialStatusBar::new(ui).expect("material status bar");
        status.set_text(0, "Ready");
        // Capture once settled, then after each state change.
        let proxy = ui.proxy();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(700));
            let _ = proxy.send(Msg::Capture("initial"));
            let _ = proxy.send(Msg::HitButtons);
            std::thread::sleep(Duration::from_millis(100));
            let _ = proxy.send(Msg::Window(SW_MAXIMIZE));
            std::thread::sleep(Duration::from_millis(900));
            let _ = proxy.send(Msg::Capture("maximized"));
            let _ = proxy.send(Msg::Foreground);
            std::thread::sleep(Duration::from_millis(150));
            let _ = proxy.send(Msg::HitButtons);
            std::thread::sleep(Duration::from_millis(100));
            // De-maximize back to the original size.
            let _ = proxy.send(Msg::Window(SW_RESTORE));
            std::thread::sleep(Duration::from_millis(900));
            let _ = proxy.send(Msg::Capture("demaximized"));
            // Minimize and come back.
            let _ = proxy.send(Msg::Window(SW_MINIMIZE));
            std::thread::sleep(Duration::from_millis(700));
            let _ = proxy.send(Msg::Window(SW_RESTORE));
            std::thread::sleep(Duration::from_millis(900));
            let _ = proxy.send(Msg::Capture("deminimized"));
            std::thread::sleep(Duration::from_millis(200));
            let _ = proxy.send(Msg::Quit);
        });
        App {
            shots: shots_for_app,
            hits: hits_for_app,
        }
    }) else {
        eprintln!("skipping: this session cannot create windows");
        return;
    };
    assert!(!run.timed_out, "the app hung");

    let shots = shots.borrow();
    assert!(
        shots.len() >= 4,
        "expected four captures, got {}",
        shots.len()
    );
    let mut initial_width = 0;
    for (tag, image) in shots.iter() {
        let centre = centre(image);
        eprintln!("{tag}: {}x{} centre={centre}", image.width, image.height);
        assert!(
            centre > 30,
            "{tag}: the client is near-black ({centre}) — the material surface did not repaint"
        );
        match *tag {
            "initial" => initial_width = image.width,
            "maximized" => assert!(
                image.width > initial_width + 100,
                "maximize left the client at {width}px instead of expanding from {initial_width}px",
                width = image.width
            ),
            "demaximized" => assert!(
                image.width <= initial_width + 50,
                "de-maximize left the client at {width}px instead of returning to {initial_width}px",
                width = image.width
            ),
            _ => {}
        }
    }

    // HTMINBUTTON = 8, HTMAXBUTTON = 9, HTCLOSE = 20 (`winuser.h`), left to
    // right across the caption buttons, in the restored state then maximized.
    let hits = hits.borrow();
    assert_eq!(
        *hits,
        vec![8, 9, 20, 8, 9, 20],
        "the caption buttons must respond when restored and maximized, got {hits:?}"
    );
}
