//! The extended title bar's frame, checked on real screen captures:
//! `TitleBar::Extended` + a backdrop must draw the three caption buttons fully
//! inside the top strip, show the material there (never opaque theme colour,
//! never black), keep the menu bar visible below the strip and leave the client
//! on the solid theme background. `PrintWindow` does not reproduce DWM's frame,
//! so the tests capture the screen.
//!
//! The material only renders on Windows 11 build 22621+, for the foreground
//! window, with transparency effects on and no high contrast. A test that
//! cannot get that on the machine prints why and skips instead of passing on
//! slivers. The tests park the real pointer away from the window and serialize,
//! because they capture the screen rectangle the window covers.

#![cfg(windows)]

mod common;

#[path = "extended_frame/screen.rs"]
mod screen;

use std::time::Duration;

use screen::{PointerGuard, Shot, capture, material_visible};
use win32ui::prelude::*;

fn px(shot: &Shot, x: i32, y: i32) -> [u8; 3] {
    let [r, g, b, _] = shot.image.pixel(x as u32, y as u32).unwrap_or([0; 4]);
    [r, g, b]
}

fn rgb(color: Color) -> [u8; 3] {
    [color.r, color.g, color.b]
}

fn distance(a: [u8; 3], b: [u8; 3]) -> i32 {
    a.iter()
        .zip(b)
        .map(|(a, b)| (*a as i32 - b as i32).abs())
        .sum()
}

/// The strip's mean colour over a band left of the caption buttons, where
/// nothing but the material is drawn.
fn strip_mean(shot: &Shot) -> [f64; 3] {
    let (mut sum, mut count) = ([0.0; 3], 0.0);
    for y in 4..(shot.strip_height - 4) {
        for x in 20..(shot.buttons.left - 20).max(21) {
            let pixel = px(shot, x, y);
            for channel in 0..3 {
                sum[channel] += pixel[channel] as f64;
            }
            count += 1.0;
        }
    }
    sum.map(|total| total / count)
}

/// How many separate runs of pixels differ from `background` along row `y`
/// (runs closer than three pixels merge).
fn clusters_on_row(shot: &Shot, y: i32, x_range: (i32, i32), background: [u8; 3]) -> usize {
    let mut clusters = 0;
    let mut gap = 3;
    for x in x_range.0..x_range.1 {
        if distance(px(shot, x, y), background) > 40 {
            if gap >= 3 {
                clusters += 1;
            }
            gap = 0;
        } else {
            gap += 1;
        }
    }
    clusters
}

fn assert_buttons_fully_drawn(shot: &Shot) {
    let buttons = shot.buttons;
    assert!(!buttons.is_empty(), "DWM reported no caption buttons");
    assert!(
        buttons.top >= 0 && buttons.bottom <= shot.strip_height,
        "the caption buttons {buttons:?} must lie inside the strip (height {})",
        shot.strip_height
    );
    let centre = (buttons.top + buttons.bottom) / 2;
    // The pixel just left of the buttons is the strip material they sit on.
    let strip = px(shot, buttons.left - 6, centre);
    let glyphs = clusters_on_row(shot, centre, (buttons.left, buttons.right), strip);
    assert!(
        glyphs >= 3,
        "expected the minimize, maximize and close glyphs on the centre row, found {glyphs}"
    );
}

fn extended(name: &str, theme: Theme, backdrop: Backdrop) -> WindowSpec {
    WindowSpec::new(name)
        .theme(theme)
        .backdrop(backdrop)
        .title_bar(TitleBar::Extended)
}

fn check_buttons(name: &str, theme: Theme) {
    let Some(shot) = capture(extended(name, theme, Backdrop::Mica)) else {
        return;
    };
    if !material_visible(&shot, name) {
        return;
    }
    assert_buttons_fully_drawn(&shot);
    let mean = strip_mean(&shot);
    assert!(
        mean.iter().any(|channel| *channel > 4.0),
        "the strip is black, not the material: mean {mean:?}"
    );
}

/// Mica Alt is darker (dark) or greyer (light) than the theme background, so on
/// it the strip must differ from both the opaque background and black; plain
/// Mica can coincide with the background on a dark wallpaper.
fn check_material(name: &str, theme: Theme) {
    let Some(shot) = capture(extended(name, theme, Backdrop::MicaAlt)) else {
        return;
    };
    if !material_visible(&shot, name) {
        return;
    }
    let background = rgb(theme.background);
    let mean = strip_mean(&shot).map(|channel| channel.round() as u8);
    assert!(
        distance(mean, background) > 6,
        "the strip {mean:?} is the opaque theme background {background:?}, not the material"
    );
    assert!(
        distance(mean, [0, 0, 0]) > 20,
        "the strip is black: {mean:?}"
    );
    // An opaque fill would look identical whatever the material; DWM's materials differ.
    let Some(acrylic) = capture(extended(name, theme, Backdrop::Acrylic)) else {
        return;
    };
    if material_visible(&acrylic, name) {
        let other = strip_mean(&acrylic).map(|channel| channel.round() as u8);
        assert!(
            distance(mean, other) > 6,
            "Mica Alt {mean:?} and Acrylic {other:?} strips are alike: an opaque fill hides the material"
        );
    }
    let top = shot.buttons.top.max(0);
    for y in top..(top + 3) {
        for x in shot.buttons.left..shot.buttons.right {
            assert_ne!(
                px(&shot, x, y),
                background,
                "an opaque theme pixel hides the caption button rows at ({x}, {y})"
            );
        }
    }
}

fn check_menu_and_content(name: &str, theme: Theme) {
    let Some(shot) = capture(extended(name, theme, Backdrop::Mica)) else {
        return;
    };
    let menu = shot.menu_bar;
    assert!(!menu.is_empty(), "the window has no menu bar");
    assert!(
        (shot.strip_height - 2..=shot.strip_height + 8).contains(&menu.top),
        "the menu bar {menu:?} must sit directly below the strip ({})",
        shot.strip_height
    );
    let text = rgb(shot.theme.text);
    let mut text_pixels = 0;
    for y in menu.top..menu.bottom {
        for x in menu.left..(menu.left + 120) {
            if distance(px(&shot, x, y), text) < 100 {
                text_pixels += 1;
            }
        }
    }
    assert!(
        text_pixels >= 10,
        "the menu bar must show text-coloured pixels, found {text_pixels}"
    );
    let content = px(
        &shot,
        shot.image.width as i32 / 2,
        shot.image.height as i32 - 40,
    );
    assert_eq!(
        content,
        rgb(shot.theme.background),
        "the content area must stay on the theme background, never black"
    );
}

/// The default spec (no backdrop, standard title bar) is untouched: the content
/// area is still the theme background and no strip is reserved.
fn check_default_spec() {
    let Some(shot) = capture(WindowSpec::new("extended.default").theme(Theme::light())) else {
        return;
    };
    assert_eq!(
        px(
            &shot,
            shot.image.width as i32 / 2,
            shot.image.height as i32 / 2
        ),
        rgb(Theme::light().background),
        "the default spec must keep the theme background"
    );
    assert_eq!(shot.strip_height, 0, "a standard title bar has no strip");
}
/// Gives the user time to take their hands off the mouse and keyboard.
fn countdown(seconds: u64) {
    for left in (1..=seconds).rev() {
        eprintln!("do not touch the mouse or keyboard, capturing in {left} s");
        std::thread::sleep(Duration::from_secs(1));
    }
}

/// Opens real windows, takes the foreground, parks the pointer and captures the
/// screen, so it is opt-in: `cargo test --test extended_frame -- --ignored
/// --nocapture`. The default `cargo test` must never steal focus or move the
/// user's pointer.
#[test]
#[ignore = "takes the foreground, moves the real pointer and captures the screen"]
fn extended_frame_on_the_real_screen() {
    countdown(10);
    // One guard for the whole run: the pointer moves once and comes back once,
    // even when an assertion below fails.
    let _pointer = PointerGuard::park();
    for (name, theme) in [("light", Theme::light()), ("dark", Theme::dark())] {
        check_buttons(&format!("extended.buttons.{name}"), theme);
        check_material(&format!("extended.material.{name}"), theme);
        check_menu_and_content(&format!("extended.menu.{name}"), theme);
    }
    check_default_spec();
}
