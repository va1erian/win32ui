//! Demo hook: when `WIN32UI_DEMO_SCREENSHOT` names a path, write a PNG of the
//! main window there once the message loop has stopped, just before the demo
//! exits.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use win32ui::prelude::*;

/// Captures `window` and writes it as a PNG if `WIN32UI_DEMO_SCREENSHOT` is
/// set. Failures are reported on stderr and never abort the demo.
pub(crate) fn capture_if_requested(window: &Window) {
    let Ok(path) = std::env::var("WIN32UI_DEMO_SCREENSHOT") else {
        return;
    };
    // A manual close destroys the window before the loop returns.
    if !window.is_alive() {
        return;
    }
    match write_screenshot(window, Path::new(&path)) {
        Ok(()) => eprintln!("demo: wrote screenshot to {path}"),
        Err(error) => eprintln!("demo: screenshot failed: {error}"),
    }
}

fn write_screenshot(
    window: &Window,
    path: &Path,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let image = window.capture()?;
    let file = File::create(path)?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), image.width, image.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&image.pixels)?;
    Ok(())
}
