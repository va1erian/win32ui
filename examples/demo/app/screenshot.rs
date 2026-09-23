//! Demo hooks: when `WIN32UI_DEMO_SCREENSHOT` names a path, write a PNG of the
//! main window there just before the demo exits; `write_composite` stacks the
//! secondary windows into one image for the light/dark screenshots.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use win32ui::prelude::*;
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN, SetCursorPos,
};

/// The gap, in pixels, between two stacked windows in a composite screenshot.
const GAP: u32 = 16;

/// Captures the window behind `ui` and writes it as a PNG if
/// `WIN32UI_DEMO_SCREENSHOT` is set. Failures are reported on stderr and never
/// abort the demo.
pub(crate) fn capture_if_requested<M: 'static>(ui: &Ui<M>) {
    let Ok(path) = std::env::var("WIN32UI_DEMO_SCREENSHOT") else {
        return;
    };
    match ui.capture() {
        Ok(image) => match write_screenshot(&image, Path::new(&path)) {
            Ok(()) => eprintln!("demo: wrote screenshot to {path}"),
            Err(error) => eprintln!("demo: screenshot failed: {error}"),
        },
        Err(error) => eprintln!("demo: screenshot failed: {error}"),
    }
}

/// Captures the window's *screen* rectangle (DWM frame, caption buttons and
/// backdrop included) and writes it as a PNG if `WIN32UI_DEMO_SCREENSHOT_SCREEN`
/// is set. The window must be on screen and unobscured.
pub(crate) fn capture_screen_if_requested<M: 'static>(ui: &Ui<M>) {
    let Ok(path) = std::env::var("WIN32UI_DEMO_SCREENSHOT_SCREEN") else {
        return;
    };
    let path = Path::new(&path);
    match ui.capture_screen() {
        Ok(image) => {
            if let Err(error) = write_screenshot(&image, path) {
                eprintln!("demo: screen screenshot failed: {error}");
                return;
            }
            eprintln!("demo: wrote screen screenshot to {}", path.display());
            write_extended_crops(&image, path, ui.theme());
        }
        Err(error) => eprintln!("demo: screen screenshot failed: {error}"),
    }
}

/// The zoom applied to the strip crops, by nearest-neighbour so pixels stay
/// crisp.
const CROP_ZOOM: u32 = 3;

/// Writes two 330×90 crops of the extended strip's top corners (the caption
/// buttons and the menu bar) next to the screen capture, named by theme and
/// zoomed 3× so the strip can be inspected at full size.
fn write_extended_crops(image: &RgbaImage, path: &Path, theme: Theme) {
    let suffix = if theme.is_dark { "dark" } else { "light" };
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let top_right = crop(
        image,
        Rect::new(image.width as i32 - 330, 0, image.width as i32, 90),
    );
    let top_left = crop(image, Rect::new(0, 0, 330, 90));
    for (cropped, name) in [
        (top_right, format!("extended-{suffix}-topright.png")),
        (top_left, format!("extended-{suffix}-topleft.png")),
    ] {
        let Some(cropped) = cropped else {
            continue;
        };
        let target = dir.join(name);
        match write_screenshot(&zoom(&cropped, CROP_ZOOM), &target) {
            Ok(()) => eprintln!("demo: wrote crop to {}", target.display()),
            Err(error) => eprintln!("demo: crop failed: {error}"),
        }
    }
}

/// Scales `image` up by an integer `factor`, repeating each pixel.
fn zoom(image: &RgbaImage, factor: u32) -> RgbaImage {
    let mut pixels = Vec::with_capacity((image.pixels.len() as u32 * factor * factor) as usize);
    for y in 0..image.height * factor {
        for x in 0..image.width * factor {
            let source = image.pixel(x / factor, y / factor).unwrap_or_default();
            pixels.extend_from_slice(&source);
        }
    }
    RgbaImage {
        width: image.width * factor,
        height: image.height * factor,
        pixels,
    }
}

/// Where the real pointer was before a screen capture parked it.
pub(crate) struct PointerParking(Option<POINT>);

/// When a screen capture is requested, moves the real pointer to the primary
/// screen's far bottom-right corner (so a hover cannot tint the caption buttons)
/// and returns a guard that puts it back when dropped.
pub(crate) fn park_pointer_if_requested() -> Option<PointerParking> {
    std::env::var_os("WIN32UI_DEMO_SCREENSHOT_SCREEN")?;
    let mut saved = POINT::default();
    // SAFETY: `saved` is a valid out-pointer; the other calls take plain
    // integers, and a failure (no interactive desktop) is ignored.
    unsafe {
        let saved = GetCursorPos(&mut saved).ok().map(|()| saved);
        let _ = SetCursorPos(
            GetSystemMetrics(SM_CXSCREEN) - 1,
            GetSystemMetrics(SM_CYSCREEN) - 1,
        );
        Some(PointerParking(saved))
    }
}

impl Drop for PointerParking {
    fn drop(&mut self) {
        if let Some(point) = self.0 {
            // SAFETY: plain integer arguments; a failure is ignored.
            unsafe {
                let _ = SetCursorPos(point.x, point.y);
            }
        }
    }
}

/// Crops `image` to `rect` (in image pixels), clipped to the image bounds.
/// Returns `None` when the intersection is empty.
pub(crate) fn crop(image: &RgbaImage, rect: Rect) -> Option<RgbaImage> {
    let left = rect.left.clamp(0, image.width as i32) as u32;
    let top = rect.top.clamp(0, image.height as i32) as u32;
    let right = rect.right.clamp(0, image.width as i32) as u32;
    let bottom = rect.bottom.clamp(0, image.height as i32) as u32;
    if left >= right || top >= bottom {
        return None;
    }
    let mut pixels = Vec::with_capacity(((right - left) * (bottom - top) * 4) as usize);
    for y in top..bottom {
        let start = (y * image.width + left) * 4;
        let end = (y * image.width + right) * 4;
        pixels.extend_from_slice(&image.pixels[start as usize..end as usize]);
    }
    Some(RgbaImage {
        width: right - left,
        height: bottom - top,
        pixels,
    })
}

pub(crate) fn write_screenshot(
    image: &RgbaImage,
    path: &Path,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let file = File::create(path)?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), image.width, image.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&image.pixels)?;
    Ok(())
}

/// Stacks `images` vertically (separated by a gap, on the theme background) and
/// writes the result as `secondary-{light|dark}.png` under `dir`. Failures are
/// reported on stderr and never abort the demo.
pub(crate) fn write_composite(dir: &Path, theme: Theme, images: &[RgbaImage]) {
    let width = images.iter().map(|image| image.width).max().unwrap_or(0);
    let height = images.iter().map(|image| image.height).sum::<u32>()
        + GAP.saturating_mul(images.len().saturating_sub(1) as u32);
    if width == 0 || height == 0 {
        eprintln!("demo: nothing to composite");
        return;
    }

    let background = theme.background;
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for (index, image) in images.iter().enumerate() {
        if index > 0 {
            for _ in 0..GAP {
                for _ in 0..width {
                    pixels.extend_from_slice(&[background.r, background.g, background.b, 0xFF]);
                }
            }
        }
        // Centre each window horizontally on the widest one.
        let x_offset = (width - image.width) / 2;
        for row in 0..image.height {
            let start = (row * image.width * 4) as usize;
            let end = start + (image.width * 4) as usize;
            for _ in 0..x_offset {
                pixels.extend_from_slice(&[background.r, background.g, background.b, 0xFF]);
            }
            pixels.extend_from_slice(&image.pixels[start..end]);
            for _ in x_offset + image.width..width {
                pixels.extend_from_slice(&[background.r, background.g, background.b, 0xFF]);
            }
        }
    }

    let composite = RgbaImage {
        width,
        height,
        pixels,
    };
    let suffix = if theme.is_dark { "dark" } else { "light" };
    let path = dir.join(format!("secondary-{suffix}.png"));
    match write_screenshot(&composite, &path) {
        Ok(()) => eprintln!("demo: wrote secondary screenshot to {}", path.display()),
        Err(error) => eprintln!("demo: secondary screenshot failed: {error}"),
    }
}
