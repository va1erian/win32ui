//! Owner-drawn check box painting (`BS_OWNERDRAW` + `WM_DRAWITEM`).
//!
//! The themed native check box always fills its checked glyph with the
//! system accent, so an app accent (`Theme::accent`) never reaches it. The
//! box is painted here instead: an accent-filled rounded square with a check
//! mark when checked, an outlined one when not. Direct2D anti-aliases the
//! glyph; when it is unavailable the same shapes fall back to GDI
//! (`FillRect`, `FrameRect`, `Polygon`, `DrawFocusRect`).

use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Gdi::{
    DrawFocusRect, FillRect, FrameRect, GetStockObject, HDC, HFONT, NULL_PEN, Polygon, SelectObject,
};

use core::ffi::c_void;

use crate::color::Color;
use crate::d2d::{Cap, DcCanvas, LineJoin, PointF, RectF, Stroke};
use crate::geometry::Rect;

use super::button_draw::{draw_disabled, draw_focused, draw_label, full_rect, rect_of};

/// Theme colours for painting one check box.
pub(crate) struct CheckPaint {
    /// Label text.
    pub text: Color,
    /// Label text, and the glyph, when disabled.
    pub text_disabled: Color,
    /// Unchecked box outline.
    pub edge: Color,
    /// Checked box fill.
    pub accent: Color,
    /// Check mark drawn on the accent fill.
    pub mark: Color,
    /// Focus ring around the whole control.
    pub focus: Color,
    /// Control background.
    pub background: Color,
}

/// The check mark's three points as fractions of the box side.
const MARK: [(f32, f32); 3] = [(0.25, 0.52), (0.43, 0.70), (0.76, 0.33)];

/// Paints an owner-drawn check box: background, box glyph, label and an
/// optional focus ring. `state` is the `WM_DRAWITEM` `itemState` (`ODS_*`);
/// only the focus and disabled flags are read.
pub(crate) fn draw_checkbox(
    hdc: isize,
    bounds: Rect,
    label: &str,
    font: HFONT,
    checked: bool,
    state: u32,
    paint: &CheckPaint,
) {
    if hdc == 0 {
        return;
    }
    let disabled = draw_disabled(state);
    let focused = draw_focused(state) && !disabled;
    let dc = HDC(hdc as *mut c_void);
    let Some(background) = crate::gdi::cache_brush(paint.background) else {
        return;
    };
    // SAFETY: `dc` is the `WM_DRAWITEM` device context, valid for the call;
    // the cached brush stays alive in the bounded per-thread GDI cache.
    unsafe {
        FillRect(dc, &rect_of(bounds), background);
    }

    let side = (bounds.height() - 6).clamp(12, 20);
    let top = bounds.top + (bounds.height() - side) / 2;
    let left = bounds.left + 2;
    let glyph = Rect::new(left, top, left + side, top + side);
    let (edge, fill) = if disabled {
        (paint.text_disabled, paint.text_disabled)
    } else {
        (paint.edge, paint.accent)
    };
    let colors = GlyphColors {
        edge,
        fill,
        mark: paint.mark,
        focus: paint.focus,
    };
    if !draw_glyph_d2d(hdc, bounds, glyph, checked, focused, &colors) {
        draw_glyph_gdi(dc, bounds, glyph, checked, focused, &colors);
    }

    let text_color = if disabled {
        paint.text_disabled
    } else {
        paint.text
    };
    draw_label(dc, font, label, left + side + 6, bounds, text_color);
}

/// The resolved (state-adjusted) glyph colours.
struct GlyphColors {
    edge: Color,
    fill: Color,
    mark: Color,
    focus: Color,
}

/// Draws the box, check mark and focus ring with Direct2D. Returns `false`
/// when Direct2D is unavailable, so the caller can paint them with GDI.
fn draw_glyph_d2d(
    hdc: isize,
    bounds: Rect,
    glyph: Rect,
    checked: bool,
    focused: bool,
    colors: &GlyphColors,
) -> bool {
    let Ok(mut canvas) = DcCanvas::new(hdc, full_rect(bounds)) else {
        return false;
    };
    let side = glyph.width() as f32;
    let radius = (side * 0.2).clamp(2.0, 4.0);
    if checked {
        canvas.fill_rounded_rect(RectF::from_rect(glyph), radius, colors.fill);
        let at = |(x, y): (f32, f32)| {
            PointF::new(glyph.left as f32 + side * x, glyph.top as f32 + side * y)
        };
        let pen = Stroke::solid((side / 9.0).max(1.5))
            .cap(Cap::Round)
            .join(LineJoin::Round);
        canvas.draw_line(at(MARK[0]), at(MARK[1]), colors.mark, pen);
        canvas.draw_line(at(MARK[1]), at(MARK[2]), colors.mark, pen);
    } else {
        // Half the 1px stroke lies outside the path, so inset by half a pixel.
        let outline = RectF::new(
            glyph.left as f32 + 0.5,
            glyph.top as f32 + 0.5,
            glyph.right as f32 - 0.5,
            glyph.bottom as f32 - 0.5,
        );
        canvas.stroke_rounded_rect(outline, radius, colors.edge, Stroke::solid(1.0));
    }
    if focused {
        let ring = RectF::new(
            bounds.left as f32 + 1.0,
            bounds.top as f32 + 1.0,
            bounds.right as f32 - 1.0,
            bounds.bottom as f32 - 1.0,
        );
        canvas.stroke_rounded_rect(ring, 2.0, colors.focus, Stroke::solid(1.0));
    }
    let _ = canvas.end_draw();
    true
}

/// The GDI fallback for [`draw_glyph_d2d`]: square corners, and the check
/// mark as a filled polygon so no pen has to be created per paint.
fn draw_glyph_gdi(
    dc: HDC,
    bounds: Rect,
    glyph: Rect,
    checked: bool,
    focused: bool,
    colors: &GlyphColors,
) {
    let (Some(edge), Some(fill), Some(mark)) = (
        crate::gdi::cache_brush(colors.edge),
        crate::gdi::cache_brush(colors.fill),
        crate::gdi::cache_brush(colors.mark),
    ) else {
        return;
    };
    let side = glyph.width() as f32;
    let thick = (side / 9.0).max(1.5);
    let at = |(x, y): (f32, f32), dy: f32| POINT {
        x: glyph.left + (side * x).round() as i32,
        y: glyph.top + (side * y + dy).round() as i32,
    };
    // The stroke of the mark, thickened downwards into a closed shape.
    let points = [
        at(MARK[0], 0.0),
        at(MARK[1], 0.0),
        at(MARK[2], 0.0),
        at(MARK[2], thick),
        at(MARK[1], thick),
        at(MARK[0], thick),
    ];
    // SAFETY: `dc` is live for the call; the stock null pen and the cached
    // brushes are live and every selected object is restored.
    unsafe {
        if checked {
            FillRect(dc, &rect_of(glyph), fill);
            let old_pen = SelectObject(dc, GetStockObject(NULL_PEN));
            let old_brush = SelectObject(dc, mark.into());
            let _ = Polygon(dc, &points);
            SelectObject(dc, old_brush);
            SelectObject(dc, old_pen);
        } else {
            FrameRect(dc, &rect_of(glyph), edge);
        }
        if focused {
            let mut focus = rect_of(bounds);
            focus.left += 1;
            focus.top += 1;
            focus.right -= 1;
            focus.bottom -= 1;
            let _ = DrawFocusRect(dc, &focus);
        }
    }
}
