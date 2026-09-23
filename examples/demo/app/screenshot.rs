//! Demo hooks: when `WIN32UI_DEMO_SCREENSHOT` names a path, write a PNG of the
//! main window there just before the demo exits; `write_composite` stacks the
//! secondary windows into one image for the light/dark screenshots.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use win32ui::prelude::*;

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
