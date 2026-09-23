//! Writes the slider's documentation screenshots: a gallery of its states
//! (hover, idle, pressed, focused, disabled, buffered, vertical) and 4x
//! nearest-neighbour crops of the thumb to show its anti-aliasing, in the light
//! and dark themes, at the system DPI and at 100%.
//!
//! Ignored by default; it needs a desktop session and a target directory:
//!
//! ```text
//! WIN32UI_SLIDER_SHOTS=docs/screenshots cargo test --test slider_shots -- --ignored
//! ```

#![cfg(windows)]

mod common;

use std::cell::RefCell;
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use common::{run_app_spec_with_watchdog_ms, screen_rect};
use win32ui::prelude::*;
use win32ui::{column, row};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_UNAWARE, SetThreadDpiAwarenessContext};
use windows::Win32::UI::WindowsAndMessaging::{SendMessageW, SetCursorPos, WM_LBUTTONDOWN};

#[derive(Debug)]
enum Msg {
    /// Park the real pointer over the hover row's thumb.
    Hover,
    /// Capture the window with the hover row hovered.
    ShotHover,
    /// Park the pointer over the pressed row's thumb and press it.
    Press,
    /// Capture the rest of the states and write the images.
    Capture,
}

/// The gallery's sliders, in row order, then the vertical volume slider.
struct Gallery {
    rows: Vec<Slider<Msg>>,
    volume: Slider<Msg>,
    out: PathBuf,
    theme: &'static str,
    dpi_label: String,
    /// Whether to write the gallery too. A DPI-unaware window has virtualised
    /// coordinates, so the real pointer cannot be parked on the thumbs there.
    gallery: bool,
    done: Rc<RefCell<bool>>,
    /// The window as captured while the hover row was hovered.
    hover_shot: Option<RgbaImage>,
}

fn send(slider: &Slider<Msg>, message: u32, wparam: usize, x: i32, y: i32) {
    let hwnd = HWND(slider.hwnd().raw() as *mut core::ffi::c_void);
    // SAFETY: a synchronous message to the slider's own live window.
    unsafe {
        SendMessageW(
            hwnd,
            message,
            Some(WPARAM(wparam)),
            Some(LPARAM(((y as isize) << 16) | (x as isize & 0xffff))),
        );
    }
}

/// The client x of `fraction` of a horizontal slider's track, in pixels.
fn track_x(slider: &Slider<Msg>, dpi: u32, fraction: f64) -> i32 {
    let pad = 12.0 * f64::from(dpi) / 96.0;
    let width = f64::from(slider.bounds().width());
    (pad + fraction * (width - 2.0 * pad)).round() as i32
}

impl App for Gallery {
    type Msg = Msg;

    fn update(&mut self, msg: Msg, ui: &mut Ui<Msg>) {
        let dpi = ui.dpi();
        match msg {
            // A synthetic move would be followed at once by a WM_MOUSELEAVE (the
            // real pointer is elsewhere), so the pointer is really moved. It
            // rests on the thumb it will press, so the captured drag keeps its value.
            Msg::Hover => park_pointer(&self.rows[0], dpi),
            Msg::ShotHover => self.hover_shot = ui.capture().ok(),
            Msg::Press => {
                let pressed = &self.rows[2];
                park_pointer(pressed, dpi);
                send(pressed, WM_LBUTTONDOWN, 1, track_x(pressed, dpi, 0.4), 12);
                self.rows[3].focus();
            }
            Msg::Capture => self.write(ui),
        }
    }
}

/// Moves the real pointer onto the thumb (value 0.4) of a horizontal `slider`.
fn park_pointer(slider: &Slider<Msg>, dpi: u32) {
    let rect = screen_rect(slider.hwnd()).expect("slider rect");
    let x = rect.left + track_x(slider, dpi, 0.4);
    let y = (rect.top + rect.bottom) / 2;
    // SAFETY: moves the cursor; no pointers involved.
    unsafe { SetCursorPos(x, y) }.expect("move the cursor");
}

impl Gallery {
    fn write(&self, ui: &Ui<Msg>) {
        let dpi = ui.dpi();
        let mut image = ui.capture().expect("capture");
        let window = ui.window_rect();
        let region = |slider: &Slider<Msg>| {
            let rect = screen_rect(slider.hwnd()).expect("slider rect");
            Rect::new(
                rect.left - window.left,
                rect.top - window.top,
                rect.right - window.left,
                rect.bottom - window.top,
            )
        };
        let mut all = region(&self.rows[0]);
        for slider in self.rows.iter().chain([&self.volume]) {
            let rect = region(slider);
            all = Rect::new(
                all.left.min(rect.left),
                all.top.min(rect.top),
                all.right.max(rect.right),
                all.bottom.max(rect.bottom),
            );
        }
        if let Some(shot) = &self.hover_shot {
            paste(&mut image, shot, region(&self.rows[0]));
        }
        if self.gallery {
            let states = crop(&image, all).expect("gallery region");
            save(
                &scale(&states, 2),
                &self.out.join(format!(
                    "slider-states-{}-{}.png",
                    self.theme, self.dpi_label
                )),
            );
        }

        // The thumb of the idle row (value 0.4) is 40 dip square around its centre.
        let row = region(&self.rows[1]);
        let centre_x = row.left + track_x(&self.rows[1], dpi, 0.4);
        let centre_y = (row.top + row.bottom) / 2;
        let half = (20.0 * f64::from(dpi) / 96.0).round() as i32;
        let thumb = crop(
            &image,
            Rect::new(
                centre_x - half,
                centre_y - half,
                centre_x + half,
                centre_y + half,
            ),
        )
        .expect("thumb region");
        save(
            &scale(&thumb, 4),
            &self.out.join(format!(
                "slider-thumb-{}-{}.png",
                self.theme, self.dpi_label
            )),
        );
        *self.done.borrow_mut() = true;
        ui.quit();
    }
}

/// Copies `rect` of `source` over the same place in `target`.
fn paste(target: &mut RgbaImage, source: &RgbaImage, rect: Rect) {
    for y in rect.top.max(0) as u32..(rect.bottom as u32).min(target.height) {
        let start = ((y * target.width + rect.left.max(0) as u32) * 4) as usize;
        let end = ((y * target.width + (rect.right as u32).min(target.width)) * 4) as usize;
        target.pixels[start..end].copy_from_slice(&source.pixels[start..end]);
    }
}

fn crop(image: &RgbaImage, rect: Rect) -> Option<RgbaImage> {
    let left = rect.left.clamp(0, image.width as i32) as u32;
    let top = rect.top.clamp(0, image.height as i32) as u32;
    let right = rect.right.clamp(0, image.width as i32) as u32;
    let bottom = rect.bottom.clamp(0, image.height as i32) as u32;
    if left >= right || top >= bottom {
        return None;
    }
    let mut pixels = Vec::new();
    for y in top..bottom {
        let start = ((y * image.width + left) * 4) as usize;
        let end = ((y * image.width + right) * 4) as usize;
        pixels.extend_from_slice(&image.pixels[start..end]);
    }
    Some(RgbaImage {
        width: right - left,
        height: bottom - top,
        pixels,
    })
}

/// Enlarges by whole pixels, with no smoothing, so every device pixel stays visible.
fn scale(image: &RgbaImage, factor: u32) -> RgbaImage {
    let mut pixels = Vec::with_capacity((image.pixels.len() as u32 * factor * factor) as usize);
    for y in 0..image.height {
        let mut line = Vec::with_capacity((image.width * factor * 4) as usize);
        for x in 0..image.width {
            let start = ((y * image.width + x) * 4) as usize;
            for _ in 0..factor {
                line.extend_from_slice(&image.pixels[start..start + 4]);
            }
        }
        for _ in 0..factor {
            pixels.extend_from_slice(&line);
        }
    }
    RgbaImage {
        width: image.width * factor,
        height: image.height * factor,
        pixels,
    }
}

fn save(image: &RgbaImage, path: &Path) {
    let file = File::create(path).expect("create screenshot");
    let mut encoder = png::Encoder::new(BufWriter::new(file), image.width, image.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("png header");
    writer.write_image_data(&image.pixels).expect("png data");
}

/// Builds the gallery in `theme`, drives each row into its state, waits for the
/// easing to settle, and writes the images. `unaware` runs the window at 96 DPI.
fn shoot(out: &Path, theme: Theme, unaware: bool) -> bool {
    let name = if theme.is_dark { "dark" } else { "light" };
    let done = Rc::new(RefCell::new(false));
    let done_for_make = Rc::clone(&done);
    let out = out.to_path_buf();
    // SAFETY: only changes how this thread's next windows are DPI-scaled.
    let previous =
        unaware.then(|| unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_UNAWARE) });
    let spec = WindowSpec::new("win32ui slider gallery")
        .size(dip(440.0), dip(330.0))
        .theme(theme);
    let ran = run_app_spec_with_watchdog_ms(spec, 20_000, move |ui| {
        let dpi = ui.dpi();
        let new =
            |ui: &mut Ui<Msg>, value: f64| Slider::new(ui, 0.0..=1.0).expect("slider").value(value);
        let hover = new(ui, 0.4);
        let idle = new(ui, 0.4);
        let pressed = new(ui, 0.4);
        let focused = new(ui, 0.4);
        let disabled = new(ui, 0.4);
        let buffered = new(ui, 0.4);
        buffered.set_buffered(0.0..0.75);
        let volume = new(ui, 0.7).vertical();
        disabled.set_enabled(false);
        ui.set_layout(row![
            column![
                hover.height(dip(32.0)),
                idle.height(dip(32.0)),
                pressed.height(dip(32.0)),
                focused.height(dip(32.0)),
                disabled.height(dip(32.0)),
                buffered.height(dip(32.0)),
            ]
            .spacing(dip(8.0))
            .fill(1),
            volume.width(dip(40.0)),
        ]);
        let proxy = ui.proxy();
        std::thread::spawn(move || {
            let script = [
                (400, Msg::Hover),
                (700, Msg::ShotHover),
                (100, Msg::Press),
                (900, Msg::Capture),
            ];
            for (wait, msg) in script {
                std::thread::sleep(Duration::from_millis(wait));
                let _ = proxy.send(msg);
            }
        });
        Gallery {
            rows: vec![hover, idle, pressed, focused, disabled, buffered],
            volume,
            out,
            theme: name,
            dpi_label: if unaware {
                "96dpi".into()
            } else {
                format!("{dpi}dpi")
            },
            done: done_for_make,
            hover_shot: None,
            gallery: !unaware,
        }
    });
    if let Some(previous) = previous {
        // SAFETY: restores the context saved above.
        unsafe { SetThreadDpiAwarenessContext(previous) };
    }
    ran.is_some() && *done.borrow()
}

#[test]
#[ignore = "writes screenshots; needs a desktop session and WIN32UI_SLIDER_SHOTS=<dir>"]
fn write_slider_screenshots() {
    let Ok(dir) = std::env::var("WIN32UI_SLIDER_SHOTS") else {
        return;
    };
    let dir = Path::new(&dir);
    for theme in [Theme::light(), Theme::dark()] {
        for unaware in [false, true] {
            assert!(shoot(dir, theme, unaware), "the gallery was not captured");
        }
    }
}
