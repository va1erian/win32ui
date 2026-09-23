//! The Direct2D primitives panel: alpha colours, linear and radial gradients,
//! RGBA bitmaps (drawn and tiled), per-corner rounded rectangles, a rounded
//! clip layer and a path. Drawn with `D2dCanvas` through the `Custom` widget's
//! `paint_d2d` hook.

use std::cell::Cell;
use std::rc::Rc;

use win32ui::Size;
use win32ui::d2d::{
    ArcSize, Cap, D2dCanvas, DashStyle, GradientStop, ImageId, Interpolation, LinearGradient,
    PathBuilder, PointF, RadialGradient, Radius, RectF, Rgba, RoundedRect, Stroke, Sweep,
};
use win32ui::gdi::Canvas;
use win32ui::prelude::*;

/// The panel's fixed height, in design units.
const PANEL_DIP: f32 = 170.0;

/// A 16×16 two-tone checkerboard used to show bitmap drawing and tiling.
fn checkerboard() -> RgbaImage {
    let size = 16;
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let on = (x / 4 + y / 4) % 2 == 0;
            let (r, g, b) = if on {
                (0x2E, 0x9B, 0xFF)
            } else {
                (0xFF, 0xB4, 0x54)
            };
            pixels.extend_from_slice(&[r, g, b, 0xFF]);
        }
    }
    RgbaImage {
        width: size,
        height: size,
        pixels,
    }
}

/// The primitives panel. The checkerboard is uploaded once and its `ImageId`
/// reused, so a repaint does not re-upload.
pub(super) struct PrimitivesPanel {
    checker: Rc<RgbaImage>,
    checker_id: Cell<Option<ImageId>>,
}

impl PrimitivesPanel {
    pub(super) fn panel<M: 'static>(ui: &mut Ui<M>) -> Custom<PrimitivesPanel, M> {
        Custom::new(
            ui,
            PrimitivesPanel {
                checker: Rc::new(checkerboard()),
                checker_id: Cell::new(None),
            },
        )
        .expect("primitives panel")
    }

    /// Writes a zoomed screenshot of the panel, cropped from the main window's
    /// capture and named `<screenshot>-zoom.png`, when `WIN32UI_DEMO_SCREENSHOT`
    /// is set.
    pub(super) fn capture_if_requested<M: 'static>(ui: &Ui<M>, panel: &Custom<PrimitivesPanel, M>) {
        let Ok(path) = std::env::var("WIN32UI_DEMO_SCREENSHOT") else {
            return;
        };
        let path = std::path::Path::new(&path);
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("screenshot");
        let zoom = path.with_file_name(format!("{stem}-zoom.png"));
        let result = ui
            .capture()
            .map_err(|error| error.to_string())
            .and_then(|full| {
                let window = ui.window_rect();
                let region = panel.window_rect();
                let pad = 12;
                let rect = Rect::new(
                    region.left - window.left - pad,
                    region.top - window.top - pad,
                    region.right - window.left + pad,
                    region.bottom - window.top + pad,
                );
                super::screenshot::crop(&full, rect)
                    .ok_or_else(|| "the panel is off-screen".to_string())
            })
            .and_then(|zoomed| {
                super::screenshot::write_screenshot(&zoomed, &zoom)
                    .map_err(|error| error.to_string())
            });
        match result {
            Ok(()) => eprintln!("demo: wrote primitives screenshot to {}", zoom.display()),
            Err(error) => eprintln!("demo: primitives screenshot failed: {error}"),
        }
    }

    fn checker(&self, canvas: &mut D2dCanvas) -> ImageId {
        match self.checker_id.get() {
            Some(id) => id,
            None => {
                let id = canvas.image(&self.checker);
                self.checker_id.set(Some(id));
                id
            }
        }
    }
}

impl CustomWidget for PrimitivesPanel {
    type Event = ();

    fn paint(&self, _canvas: &Canvas, _bounds: Rect, _theme: &Theme) {}

    fn paint_d2d(&self, canvas: &mut D2dCanvas, bounds: RectF, theme: &Theme) {
        canvas.clear(theme.background);
        let left = bounds.left;
        let top = bounds.top;

        // A horizontal linear gradient band (clamped).
        let band = RectF::new(left + 12.0, top + 12.0, bounds.right - 12.0, top + 34.0);
        let linear = LinearGradient::new(
            PointF::new(band.left, 0.0),
            PointF::new(band.right, 0.0),
            vec![
                GradientStop::new(0.0, Rgba::rgb(0x2E, 0x9B, 0xFF)),
                GradientStop::new(0.5, Rgba::rgb(0x00, 0xC2, 0x8A)),
                GradientStop::new(1.0, Rgba::rgb(0xFF, 0xB4, 0x54)),
            ],
        );
        canvas.fill_rect_linear(band, &linear);

        // A radial gradient circle and a semi-transparent fill over it.
        let center = PointF::new(left + 52.0, top + 64.0);
        let radial = RadialGradient::new(
            center,
            26.0,
            26.0,
            vec![
                GradientStop::new(0.0, Rgba::WHITE),
                GradientStop::new(1.0, Rgba::rgb(0xE0, 0x30, 0x30)),
            ],
        );
        canvas.fill_ellipse_radial(center, 26.0, 26.0, &radial);
        canvas.fill_rect_rgba(
            RectF::new(left + 40.0, top + 52.0, left + 64.0, top + 76.0),
            Rgba::with_alpha(0x00, 0x00, 0x00, 0x60),
        );

        // Per-corner rounded rectangles: differing radii on each corner.
        let rounded = RoundedRect::new(
            RectF::new(left + 96.0, top + 40.0, left + 210.0, top + 92.0),
            [
                Radius::uniform(22.0),
                Radius::new(6.0, 18.0),
                Radius::uniform(2.0),
                Radius::new(18.0, 6.0),
            ],
        );
        canvas.fill_rounded(rounded, Rgba::rgb(0x9B, 0x59, 0xB6));
        canvas.stroke_rounded(
            rounded,
            Rgba::with_alpha(0x00, 0x00, 0x00, 0x80),
            Stroke::solid(2.0),
        );

        // A rounded clip layer: only the shape's interior is painted.
        let clip = RoundedRect::uniform(
            RectF::new(left + 230.0, top + 40.0, left + 320.0, top + 92.0),
            16.0,
        );
        if canvas.push_clip_rounded(clip).is_ok() {
            canvas.fill_rect(
                RectF::new(left + 224.0, top + 34.0, left + 326.0, top + 98.0),
                Color::rgb(0x00, 0xC2, 0x8A),
            );
            let _ = canvas.pop_clip();
        }

        // A dotted line with round caps.
        canvas.draw_line_rgba(
            PointF::new(left + 12.0, top + 106.0),
            PointF::new(bounds.right - 12.0, top + 106.0),
            Rgba::rgb(0x2E, 0x9B, 0xFF),
            Stroke::solid(2.0).dash(DashStyle::Dotted).cap(Cap::Round),
        );

        // A path: a filled/stroked teardrop (line + arc + quadratic).
        if let Ok(mut path) = PathBuilder::new() {
            path.move_to(PointF::new(left + 380.0, top + 128.0))
                .line_to(PointF::new(left + 360.0, top + 96.0))
                .arc_to(
                    PointF::new(left + 400.0, top + 96.0),
                    20.0,
                    20.0,
                    Sweep::Clockwise,
                    ArcSize::Large,
                )
                .quadratic_to(
                    PointF::new(left + 396.0, top + 118.0),
                    PointF::new(left + 380.0, top + 128.0),
                )
                .close();
            if let Ok(path) = path.build() {
                canvas.fill_path(&path, Rgba::rgb(0xFF, 0xB4, 0x54));
                canvas.stroke_path(&path, Rgba::rgb(0x00, 0x00, 0x00), Stroke::solid(1.5));
            }
        }

        // The bitmap, drawn scaled and tiled.
        let checker = self.checker(canvas);
        canvas.draw_image(
            checker,
            RectF::new(left + 440.0, top + 40.0, left + 476.0, top + 76.0),
            None,
            1.0,
            Interpolation::Nearest,
        );
        canvas.fill_image_tiled(
            checker,
            RectF::new(left + 440.0, top + 96.0, left + 520.0, top + 132.0),
            1.0,
            Interpolation::Nearest,
        );
    }

    fn preferred_size(&self, dpi: u32) -> Option<Size> {
        Some(Size::new(0, dip(PANEL_DIP).to_px(dpi).value()))
    }
}
