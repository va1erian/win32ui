//! The acrylic title strip's self-drawn menu bar, checked on a real screen
//! capture: with `Backdrop::Acrylic`, `TitleBar::Extended` and
//! `WindowSpec::menu_in_strip(true)`, the menu items must be painted with
//! alpha-correct Direct2D (so they are opaque, not dropped), the strip must
//! show the material rather than the opaque theme background, the caption
//! buttons must still be drawn, and the content area below must stay opaque.
//!
//! Opt-in, like the extended-frame checks: it takes the foreground, parks the
//! pointer and captures the screen. Run with:
//! `cargo test --test title_menu_strip -- --ignored --nocapture`.

#![cfg(windows)]

mod common;

#[path = "extended_frame/screen.rs"]
mod screen;

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::time::Duration;

use screen::{PointerGuard, Shot, capture, material_visible};
use win32ui::prelude::*;

fn px(shot: &Shot, x: i32, y: i32) -> [u8; 3] {
    let [r, g, b, _] = shot.image.pixel(x as u32, y as u32).unwrap_or([0; 4]);
    [r, g, b]
}

fn distance(a: [u8; 3], b: [u8; 3]) -> i32 {
    a.iter()
        .zip(b)
        .map(|(a, b)| (*a as i32 - b as i32).abs())
        .sum()
}

fn spec(name: &str, theme: Theme) -> WindowSpec {
    WindowSpec::new(name)
        .theme(theme)
        .backdrop(Backdrop::Acrylic)
        .title_bar(TitleBar::Extended)
        .menu_in_strip(true)
}

fn countdown(seconds: u64) {
    for left in (1..=seconds).rev() {
        eprintln!("do not touch the mouse or keyboard, capturing in {left} s");
        std::thread::sleep(Duration::from_secs(1));
    }
}

/// Counts the strip pixels close to the theme text and pure black, and reports
/// the strip's mean colour.
fn strip_summary(shot: &Shot, theme: Theme) -> (usize, usize, [f64; 3]) {
    let text = [theme.text.r, theme.text.g, theme.text.b];
    let (mut text_pixels, mut black_pixels) = (0, 0);
    let mut mean = [0.0f64; 3];
    let mut count = 0.0f64;
    for y in 0..shot.strip_height {
        for x in 0..shot.image.width as i32 {
            let pixel = px(shot, x, y);
            if distance(pixel, text) < 90 {
                text_pixels += 1;
            }
            if pixel == [0, 0, 0] {
                black_pixels += 1;
            }
            for channel in 0..3 {
                mean[channel] += pixel[channel] as f64;
            }
            count += 1.0;
        }
    }
    (text_pixels, black_pixels, mean.map(|total| total / count))
}

/// Writes `image` (a strip crop, nearest-neighbour zoomed by `factor`) as a
/// PNG, for a human or vision agent to inspect.
fn write_png(image: &RgbaImage, path: &Path, factor: u32) {
    let (mut width, mut height, mut pixels) = (image.width, image.height, image.pixels.clone());
    if factor > 1 {
        width *= factor;
        height *= factor;
        pixels = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                pixels.extend_from_slice(&image.pixel(x / factor, y / factor).unwrap_or_default());
            }
        }
    }
    let zipped = RgbaImage {
        width,
        height,
        pixels,
    };
    let Ok(file) = File::create(path) else {
        return;
    };
    let mut encoder = png::Encoder::new(BufWriter::new(file), zipped.width, zipped.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    if let Ok(mut writer) = encoder.write_header() {
        let _ = writer.write_image_data(&zipped.pixels);
    }
}

fn save_screenshots(shot: &Shot, theme: Theme) {
    let suffix = if theme.is_dark { "dark" } else { "light" };
    let dir = Path::new("docs/screenshots");
    write_png(
        &shot.image,
        &dir.join(format!("title-menu-{suffix}.png")),
        1,
    );
    let crop = |x: i32| {
        let rect = Rect::new(x, 0, x + 360, 100);
        let left = rect.left.clamp(0, shot.image.width as i32) as u32;
        let top = rect.top.clamp(0, shot.image.height as i32) as u32;
        let right = rect.right.clamp(0, shot.image.width as i32) as u32;
        let bottom = rect.bottom.clamp(0, shot.image.height as i32) as u32;
        let mut pixels = Vec::with_capacity(((right - left) * (bottom - top) * 4) as usize);
        for y in top..bottom {
            let start = (y * shot.image.width + left) * 4;
            let end = (y * shot.image.width + right) * 4;
            pixels.extend_from_slice(&shot.image.pixels[start as usize..end as usize]);
        }
        RgbaImage {
            width: right - left,
            height: bottom - top,
            pixels,
        }
    };
    write_png(
        &crop(0),
        &dir.join(format!("title-menu-{suffix}-topleft.png")),
        3,
    );
    write_png(
        &crop(shot.image.width as i32 - 360),
        &dir.join(format!("title-menu-{suffix}-topright.png")),
        3,
    );
}

fn check(name: &str, theme: Theme, save: bool) {
    let Some(shot) = capture(spec(name, theme)) else {
        return;
    };
    eprintln!(
        "{name}: backdrop_active={} foreground={} strip={} buttons={:?}",
        shot.backdrop_active, shot.foreground, shot.strip_height, shot.buttons
    );
    if save {
        save_screenshots(&shot, theme);
    }
    if !material_visible(&shot, name) {
        return;
    }
    assert!(
        shot.menu_bar.is_empty(),
        "the native menu bar must be gone in strip mode, found {:?}",
        shot.menu_bar
    );
    assert!(
        !shot.buttons.is_empty() && shot.buttons.bottom <= shot.strip_height,
        "DWM's caption buttons must still be drawn inside the strip"
    );

    let (text_pixels, black_pixels, mean) = strip_summary(&shot, theme);
    eprintln!("{name}: strip mean {mean:?}, text {text_pixels}, black {black_pixels}");
    assert!(
        text_pixels >= 20,
        "expected opaque DirectWrite menu text in the strip, found {text_pixels}"
    );
    assert!(
        black_pixels < (shot.image.width as usize * shot.strip_height.max(1) as usize) / 4,
        "the strip must not be mostly black (material missing): {black_pixels} black pixels"
    );
    let background = [theme.background.r, theme.background.g, theme.background.b];
    assert!(
        distance(mean.map(|c| c.round() as u8), background) > 4,
        "the strip {mean:?} looks like the opaque theme background {background:?}"
    );
    let content = px(
        &shot,
        shot.image.width as i32 / 2,
        shot.image.height as i32 - 40,
    );
    assert_eq!(
        content, background,
        "the content below the strip must stay opaque theme background"
    );
}

#[test]
#[ignore = "takes the foreground, moves the real pointer and captures the screen"]
fn strip_menu_on_the_real_screen() {
    countdown(5);
    let _pointer = PointerGuard::park();
    check("strip.light", Theme::light(), true);
    check("strip.dark", Theme::dark(), true);
}
