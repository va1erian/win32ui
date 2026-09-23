//! Writes `FlowText`'s documentation screenshots: the now-playing line wide
//! (no wrap) and narrow (wrapped, with CJK, an emoji and RTL), in the light and
//! dark themes, each with a link hovered so the accent colour and the underline
//! show. Ignored by default; it needs a desktop session and a target directory:
//!
//! ```text
//! WIN32UI_FLOW_SHOTS=docs/screenshots cargo test --test flow_text_shots -- --ignored
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
use win32ui::column;
use win32ui::d2d::{FontSpec, RectF, Span, TextSystem};
use win32ui::prelude::*;
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN, SetCursorPos,
};

/// The font `FlowText` uses by default.
const FAMILY: &str = "Segoe UI, Arial, sans-serif";
const SIZE: f32 = 14.0;
/// The widths the line is captured at, in design units.
const WIDE_WIDTH: f32 = 620.0;
const NARROW_WIDTH: f32 = 240.0;
/// The margin around the line.
const MARGIN: f32 = 16.0;
/// How long the pointer rests on the link before the frame is captured.
const SETTLE_MS: u64 = 500;

#[derive(Clone, Debug, PartialEq)]
enum Msg {
    /// Park the real pointer on the link.
    Hover,
    /// Capture the window and write the image.
    Capture,
}

/// The line's spans, in the exact order `build_line` builds its runs.
fn spans(wide: bool) -> Vec<Span> {
    let mut spans = vec![
        Span::new("The Midnight Set"),
        Span::new(" · "),
        Span::new("Signal 1"),
        Span::new(" · "),
        Span::new("(2004)").size(12.0),
        Span::new(" · "),
        Span::new("Track 3, Disc 1"),
        Span::new(" · "),
        Span::new("Electronic"),
    ];
    if !wide {
        spans.extend([
            Span::new(" · "),
            Span::new("日本語のアルバム"),
            Span::new(" · "),
            Span::new("👋"),
            Span::new(" · "),
            Span::new("مرحبا"),
        ]);
    }
    spans
}

fn system() -> TextSystem {
    TextSystem::new().expect("DirectWrite is available")
}

fn build_line(ui: &mut Ui<Msg>, wide: bool) -> FlowText<Msg> {
    let flow = FlowText::new(ui)
        .expect("flow text")
        .run(Run::link("The Midnight Set").on_click(|| Some(Msg::Hover)))
        .separator(" · ")
        .run(Run::link("Signal 1").on_click(|| Some(Msg::Hover)))
        .separator(" · ")
        .run(Run::weak("(2004)").size(12.0))
        .separator(" · ")
        .run(Run::normal("Track 3, Disc 1"))
        .separator(" · ")
        .run(Run::normal("Electronic"));
    if wide {
        flow
    } else {
        flow.separator(" · ")
            .run(Run::weak("日本語のアルバム"))
            .separator(" · ")
            .run(Run::normal("👋"))
            .separator(" · ")
            .run(Run::weak("مرحبا"))
    }
}

/// The link to hover: the artist wide, the album narrow.
fn hover_rect(width: f32, wide: bool) -> RectF {
    let font = system().font(&FontSpec::new(FAMILY, SIZE)).expect("font");
    let layout = font.rich_layout(&spans(wide), width).expect("layout");
    layout.rects_of_span(if wide { 0 } else { 2 })[0]
}

struct Shot {
    flow: FlowText<Msg>,
    dpi: u32,
    wide: bool,
    out: PathBuf,
    done: Rc<RefCell<bool>>,
}

impl Shot {
    fn park_on_link(&self) {
        let scale = self.dpi as f32 / 96.0;
        let rect = screen_rect(self.flow.hwnd()).expect("flow rect");
        // Use the width the widget actually got, so the hover point matches its
        // own wrap even though the window frame eats a few pixels.
        let width = win32ui::d2d::pixels_to_dips(self.flow.bounds().width(), self.dpi);
        let hover = hover_rect(width, self.wide);
        let x = rect.left + ((hover.left + hover.right) / 2.0 * scale) as i32;
        let y = rect.top + ((hover.top + hover.bottom) / 2.0 * scale) as i32;
        // SAFETY: moves the cursor; no pointers involved.
        unsafe { SetCursorPos(x, y) }.expect("move the cursor");
    }
}

impl App for Shot {
    type Msg = Msg;

    fn update(&mut self, msg: Msg, ui: &mut Ui<Msg>) {
        match msg {
            Msg::Hover => self.park_on_link(),
            Msg::Capture => {
                let image = ui.capture().expect("capture");
                save(&image, &self.out);
                // SAFETY: moves the cursor off the window; no pointers involved.
                unsafe {
                    SetCursorPos(
                        GetSystemMetrics(SM_CXSCREEN) - 1,
                        GetSystemMetrics(SM_CYSCREEN) - 1,
                    )
                }
                .ok();
                *self.done.borrow_mut() = true;
                ui.quit();
            }
        }
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

/// Captures one (width, theme) combination and writes it under `out`.
fn shoot(out: &Path, theme: Theme, wide: bool) -> bool {
    let name = if theme.is_dark { "dark" } else { "light" };
    let width = if wide { WIDE_WIDTH } else { NARROW_WIDTH };
    let done = Rc::new(RefCell::new(false));
    let done_for_make = Rc::clone(&done);
    let out = out.to_path_buf();
    let file = out.join(format!(
        "flow-text-{}-{}.png",
        if wide { "wide" } else { "narrow" },
        name
    ));
    let (window_width, window_height) = if wide {
        (WIDE_WIDTH + 2.0 * MARGIN, 90.0)
    } else {
        (NARROW_WIDTH + 2.0 * MARGIN, 200.0)
    };
    let spec = WindowSpec::new("win32ui flow text")
        .size(dip(window_width), dip(window_height))
        .theme(theme);
    let ran = run_app_spec_with_watchdog_ms(spec, 20_000, move |ui| {
        let flow = build_line(ui, wide);
        let height = flow.preferred_height(dip(width));
        ui.set_layout(
            column![column![flow.height(height)].width(dip(width))]
                .margins(Insets::all(dip(MARGIN))),
        );
        let proxy = ui.proxy();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            let _ = proxy.send(Msg::Hover);
            std::thread::sleep(Duration::from_millis(SETTLE_MS));
            let _ = proxy.send(Msg::Capture);
        });
        Shot {
            dpi: ui.dpi(),
            wide,
            flow,
            out: file,
            done: done_for_make,
        }
    });
    ran.is_some() && *done.borrow()
}

#[test]
#[ignore = "writes screenshots; needs a desktop session and WIN32UI_FLOW_SHOTS=<dir>"]
fn write_flow_text_screenshots() {
    let Ok(dir) = std::env::var("WIN32UI_FLOW_SHOTS") else {
        return;
    };
    let dir = Path::new(&dir);
    for theme in [Theme::light(), Theme::dark()] {
        for wide in [true, false] {
            assert!(shoot(dir, theme, wide), "the shot was not captured");
        }
    }
}
