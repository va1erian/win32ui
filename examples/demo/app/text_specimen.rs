//! A DirectWrite text specimen in its own window: scripts, weights, mixed
//! sizes, fallback fonts, emoji, and a selection drawn from the layout.
//!
//! Opened when `WIN32UI_DEMO_SPECIMEN` is set. With `WIN32UI_DEMO_SCREENSHOT`
//! also set, the specimen writes `<name>-specimen.png` next to that path.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;

use win32ui::d2d::{D2dSurface, Font, FontSpec, Layout, PointF, RectF, TextSystem};
use win32ui::prelude::*;

use super::screenshot::write_screenshot;

const MARGIN: f32 = 24.0;
const WIDTH_DIP: f32 = 700.0;
const HEIGHT_DIP: f32 = 750.0;
const CAPTURE_MS: u32 = 900;

/// Everything to draw, built once: DirectWrite layouts are in device-independent
/// pixels, so they stay valid at any DPI.
struct Page {
    fills: Vec<(RectF, Color)>,
    texts: Vec<(PointF, Layout, Color)>,
}

/// Builds a [`Page`] top to bottom.
struct Builder<'a> {
    text: &'a TextSystem,
    theme: Theme,
    caption: Font,
    body: Font,
    page: Page,
    y: f32,
}

impl Builder<'_> {
    fn spec(&self, family: &str, size: f32) -> FontSpec {
        FontSpec::new(family, size)
    }

    fn font(&self, spec: &FontSpec) -> Font {
        self.text.font(spec).expect("font")
    }

    fn put(&mut self, x: f32, y: f32, layout: Layout, color: Color) {
        self.page.texts.push((PointF::new(x, y), layout, color));
    }

    /// A small heading over the next specimen row.
    fn caption(&mut self, label: &str) {
        self.y += 10.0;
        let layout = self.caption.layout(label, f32::INFINITY).expect("layout");
        let height = layout.height();
        self.put(MARGIN, self.y, layout, self.theme.text_secondary);
        self.y += height + 1.0;
    }

    /// One unwrapped line of `text` in `font`.
    fn line(&mut self, font: &Font, text: &str) {
        let layout = font.layout(text, f32::INFINITY).expect("layout");
        let height = layout.height();
        self.put(MARGIN, self.y, layout, self.theme.text);
        self.y += height;
    }

    fn body_line(&mut self, text: &str) {
        let font = self.body.clone();
        self.line(&font, text);
    }

    /// Several fonts side by side on one shared baseline.
    fn baseline_row(&mut self, sizes: &[f32]) {
        let fonts: Vec<Font> = sizes
            .iter()
            .map(|&size| self.font(&self.spec("Segoe UI", size)))
            .collect();
        let layouts: Vec<Layout> = fonts
            .iter()
            .map(|font| font.layout("Ag", f32::INFINITY).expect("layout"))
            .collect();
        let baseline = fonts
            .iter()
            .map(|font| font.metrics().ascent)
            .fold(0.0, f32::max);
        let mut x = MARGIN;
        for (font, layout) in fonts.iter().zip(layouts) {
            let metrics = font.metrics();
            let width = layout.width();
            self.put(
                x,
                self.y + baseline - metrics.ascent,
                layout,
                self.theme.text,
            );
            x += width + 14.0;
        }
        let descent = fonts
            .iter()
            .map(|font| font.metrics().descent)
            .fold(0.0, f32::max);
        self.y += baseline + descent;
    }

    /// A wrapped paragraph with a selection and caret drawn from its layout.
    fn selection_paragraph(&mut self) {
        let text = "Carets, selections and hit testing come from the same layout that draws the \
                    text, so they line up exactly, even across wrapped lines and mixed scripts.";
        let layout = self
            .body
            .layout(text, WIDTH_DIP - 2.0 * MARGIN)
            .expect("layout");
        let start = text.find("selections").expect("word");
        let end = text.find("mixed").expect("word");
        for rect in layout.selection_rects(start, end) {
            self.page
                .fills
                .push((offset(rect, MARGIN, self.y), self.theme.selection));
        }
        let caret = layout.caret_rect(end);
        self.page
            .fills
            .push((offset(caret, MARGIN, self.y), self.theme.accent));
        let height = layout.height();
        self.put(MARGIN, self.y, layout, self.theme.text);
        self.y += height;
    }

    /// A line with a rule under it exactly `Font::width` long: what layout
    /// measures is what gets drawn, fallback glyphs included.
    fn measured_line(&mut self, text: &str) {
        let font = self.body.clone();
        let width = font.width(text);
        self.line(&font, text);
        let y = self.y + 1.0;
        self.page.fills.push((
            RectF::new(MARGIN, y, MARGIN + width, y + 2.0),
            self.theme.accent,
        ));
        self.y += 3.0;
    }
}

fn offset(rect: RectF, dx: f32, dy: f32) -> RectF {
    RectF::new(
        rect.left + dx,
        rect.top + dy,
        rect.right + dx,
        rect.bottom + dy,
    )
}

impl Page {
    fn build(text: &TextSystem, theme: Theme) -> Page {
        let font = |spec: FontSpec| text.font(&spec).expect("font");
        let mut b = Builder {
            text,
            theme,
            caption: font(FontSpec::new("Segoe UI", 11.5)),
            body: font(FontSpec::new("Segoe UI, Arial, sans-serif", 17.0)),
            page: Page {
                fills: Vec::new(),
                texts: Vec::new(),
            },
            y: 14.0,
        };
        b.caption("Latin, with kerning and ligatures");
        b.body_line("The quick brown fox jumps over the lazy dog. 0123456789 fi fl ffi");
        b.caption("Weights and styles");
        for (family, weight, italic) in [
            ("Segoe UI", 300, false),
            ("Segoe UI", 400, true),
            ("Segoe UI Semibold", 400, false),
            ("Segoe UI", 700, false),
            ("Segoe UI", 700, true),
            ("Arial Black", 400, false),
        ] {
            let spec = b.spec(family, 17.0).weight(weight).italic(italic);
            let label = format!("{family} {weight}{}", if italic { " italic" } else { "" });
            let styled = b.font(&spec);
            b.line(&styled, &label);
        }
        b.caption("Mixed sizes on one baseline, positioned from font metrics");
        b.baseline_row(&[11.0, 14.0, 18.0, 24.0, 32.0, 44.0]);
        b.caption("CJK (fallback fonts)");
        b.body_line("日本語のテキスト  ·  简体中文  ·  한국어 텍스트");
        b.caption("Arabic and Hebrew (right to left)");
        b.body_line("مرحبا بالعالم  ·  שלום עולם  ·  abc مرحبا 123");
        b.caption("Combining marks");
        b.body_line("Cafe\u{301} nai\u{308}ve  n\u{303}  a\u{30A}  Z\u{30C}\u{323}  ก็ ที่ นี้");
        b.caption("Emoji (colour glyphs)");
        b.body_line("Hello 👋  🎉 🚀 ❤️ 👨‍👩‍👧 🏳️‍🌈");
        b.caption("Measured width equals drawn width (rule = Font::width)");
        b.measured_line("Measure me 👋 世界 مرحبا fi");
        b.caption("Selection and caret from the layout");
        b.selection_paragraph();
        b.page
    }
}

struct SpecimenHandler {
    page: Page,
    theme: Theme,
    surface: RefCell<Option<D2dSurface>>,
    capture_timer: Cell<Option<TimerId>>,
    screenshot: Option<PathBuf>,
}

impl SpecimenHandler {
    fn paint(&self, window: &Window) {
        let mut surface = self.surface.borrow_mut();
        if surface.is_none() {
            *surface = D2dSurface::new(window.hwnd()).ok();
        }
        let Some(surface) = surface.as_ref() else {
            return;
        };
        let Ok(mut canvas) = surface.begin_draw() else {
            return;
        };
        canvas.clear(self.theme.background);
        for &(rect, color) in &self.page.fills {
            canvas.fill_rect(rect, color);
        }
        for (origin, layout, color) in &self.page.texts {
            canvas.draw_text(layout, *origin, *color);
        }
        if let Err(error) = canvas.end_draw() {
            eprintln!("demo: specimen paint failed: {error}");
        }
    }
}

impl WindowHandler for SpecimenHandler {
    fn message(&self, window: &Window, message: Message) -> Option<LResult> {
        if D2dSurface::is_erase_background(&message) {
            return Some(1);
        }
        match message {
            Message::Create if self.screenshot.is_some() => {
                self.capture_timer.set(window.set_timer(CAPTURE_MS).ok());
            }
            Message::Paint => {
                self.paint(window);
                return Some(0);
            }
            Message::Size { width, height } => {
                if let Some(surface) = self.surface.borrow().as_ref() {
                    surface.resize(width, height);
                }
            }
            Message::Timer { id } if Some(id) == self.capture_timer.get() => {
                window.kill_timer(id);
                if let (Some(path), Ok(image)) = (&self.screenshot, window.capture()) {
                    match write_screenshot(&image, path) {
                        Ok(()) => eprintln!("demo: wrote screenshot to {}", path.display()),
                        Err(error) => eprintln!("demo: specimen screenshot failed: {error}"),
                    }
                }
            }
            _ => {}
        }
        None
    }
}

/// `<path stem>-specimen.png`, next to the requested screenshot.
fn specimen_path(requested: &str) -> PathBuf {
    let path = PathBuf::from(requested);
    let stem = path
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    path.with_file_name(format!("{stem}-specimen.png"))
}

/// Opens the specimen window when `WIN32UI_DEMO_SPECIMEN` is set. The window
/// lives until it is closed or the demo exits.
pub(crate) fn open_if_requested(theme: Theme, dpi: u32) {
    if std::env::var_os("WIN32UI_DEMO_SPECIMEN").is_none() {
        return;
    }
    let text = match TextSystem::new() {
        Ok(text) => text,
        Err(error) => return eprintln!("demo: no DirectWrite: {error}"),
    };
    let handler = SpecimenHandler {
        page: Page::build(&text, theme),
        theme,
        surface: RefCell::new(None),
        capture_timer: Cell::new(None),
        screenshot: std::env::var("WIN32UI_DEMO_SCREENSHOT")
            .ok()
            .map(|path| specimen_path(&path)),
    };
    let created =
        WindowClass::register("win32ui.demo.specimen", theme.background).and_then(|class| {
            let width = dip(WIDTH_DIP).to_px(dpi).value();
            let height = dip(HEIGHT_DIP).to_px(dpi).value();
            Window::create(
                class,
                None,
                WindowStyle::overlapped(),
                WindowExStyle::new(),
                Rect::new(120, 60, 120 + width, 60 + height),
                "DirectWrite text specimen",
                handler,
            )
        });
    match created {
        Ok(window) => {
            window.set_theme(theme);
            window.show();
            // The window belongs to the demo's message loop for the rest of
            // the process; dropping the handle here would destroy it.
            std::mem::forget(window);
        }
        Err(error) => eprintln!("demo: specimen window failed: {error}"),
    }
}
