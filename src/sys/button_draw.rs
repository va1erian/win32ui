//! Owner-drawn radio and group box painting (`BS_OWNERDRAW` + `WM_DRAWITEM`).
//!
//! `DarkMode_Explorer` darkens push buttons and check boxes, but two native
//! parts ignore it: radio button text stays `COLOR_BTNTEXT` (black) and the
//! group box frame paints classic-light. Both are created with `BS_OWNERDRAW`
//! and painted here from the window's theme tokens instead. The ring, dot,
//! focus ring and frame are anti-aliased through Direct2D, falling back to the
//! old GDI shapes when it is unavailable (documented APIs only: `DrawTextW`,
//! `Ellipse`, `FrameRect`, `DrawFocusRect`).

use windows::Win32::Foundation::{COLORREF, HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    DRAW_TEXT_FORMAT, DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_TOP, DT_VCENTER,
    DrawFocusRect, DrawTextW, Ellipse, FillRect, FrameRect, GetStockObject, HDC, HFONT, NULL_PEN,
    SetBkMode, SetTextColor, TRANSPARENT,
};
use windows::Win32::UI::Controls::{DRAWITEMSTRUCT, ODS_DISABLED, ODS_FOCUS};
use windows::Win32::UI::WindowsAndMessaging::BS_OWNERDRAW;

use core::ffi::c_void;

use crate::color::Color;
use crate::d2d::{DcCanvas, PointF, RectF, Stroke};
use crate::geometry::Rect;

/// Adds `BS_OWNERDRAW` (`Winuser.h`) to a button style.
pub(crate) fn owner_drawn(style: u32) -> u32 {
    style | BS_OWNERDRAW as u32
}

/// Whether a `WM_DRAWITEM` state (`ODS_*` from `Winuser.h`) is disabled.
pub(crate) fn draw_disabled(state: u32) -> bool {
    state & ODS_DISABLED.0 != 0
}

/// Whether a `WM_DRAWITEM` state carries keyboard focus.
pub(crate) fn draw_focused(state: u32) -> bool {
    state & ODS_FOCUS.0 != 0
}

/// Reads a `WM_DRAWITEM` (`DRAWITEMSTRUCT`) payload: the control, the
/// action/state and the DC/rect, which are only valid while handling it.
pub(crate) struct DrawRequest {
    /// The owner-drawn control.
    pub hwnd: HWND,
    /// `CtlType` (`ODT_*` from `Winuser.h`): menu, button, combo box, …
    pub control_type: u32,
    /// `itemID`: the menu command id, or the control's item index.
    pub item_id: usize,
    /// `itemData`: the application-defined value set when the item was added.
    pub item_data: usize,
    /// `itemState` (`ODS_*`).
    pub state: u32,
    /// A raw `HDC` value, valid while handling the message.
    pub hdc: isize,
    /// The rectangle to paint.
    pub rect: Rect,
}

/// Copies the `DRAWITEMSTRUCT` `lparam` points at. Returns `None` for a null
/// pointer.
pub(crate) fn draw_request(lparam: isize) -> Option<DrawRequest> {
    if lparam == 0 {
        return None;
    }
    // SAFETY: for `WM_DRAWITEM`, `lparam` points at a `DRAWITEMSTRUCT` owned
    // by the system for the duration of the message.
    let info = unsafe { &*(lparam as *const DRAWITEMSTRUCT) };
    Some(DrawRequest {
        hwnd: info.hwndItem,
        control_type: info.CtlType.0,
        item_id: info.itemID as usize,
        item_data: info.itemData,
        state: info.itemState.0,
        hdc: info.hDC.0 as isize,
        rect: Rect::new(
            info.rcItem.left,
            info.rcItem.top,
            info.rcItem.right,
            info.rcItem.bottom,
        ),
    })
}

/// Combines `DrawTextW` flags (`Winuser.h`: `DT_*`). The `windows` format
/// newtype does not implement `BitOr`, so the bits are folded manually.
fn text_format(flags: &[DRAW_TEXT_FORMAT]) -> DRAW_TEXT_FORMAT {
    DRAW_TEXT_FORMAT(flags.iter().fold(0, |bits, flag| bits | flag.0))
}

/// `rect` as a raw `RECT`.
fn rect_of(rect: Rect) -> RECT {
    RECT {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
    }
}

/// The control's client rectangle for binding a DC render target, whose origin
/// is the control's top-left. `bounds.right`/`bottom` are the client extent.
fn full_rect(bounds: Rect) -> Rect {
    Rect::new(0, 0, bounds.right.max(1), bounds.bottom.max(1))
}

/// Theme colours for painting one radio button.
pub(crate) struct RadioPaint {
    /// Label text.
    pub text: Color,
    /// Label text when disabled.
    pub text_disabled: Color,
    /// Glyph ring.
    pub edge: Color,
    /// Glyph dot when checked.
    pub dot: Color,
    /// Focus ring around the whole control.
    pub focus: Color,
    /// Control background.
    pub background: Color,
}

/// Paints an owner-drawn radio button: background, ring glyph with dot, label
/// and an optional focus ring. The ring, dot and focus ring are anti-aliased
/// through Direct2D; when it is unavailable the same shapes fall back to GDI.
/// `state` is the `WM_DRAWITEM` `itemState` (`ODS_*`); only the focus and
/// disabled flags are read.
pub(crate) fn draw_radio(
    hdc: isize,
    bounds: Rect,
    label: &str,
    font: HFONT,
    checked: bool,
    state: u32,
    paint: &RadioPaint,
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

    let diameter = (bounds.height() - 6).clamp(12, 20);
    let top = bounds.top + (bounds.height() - diameter) / 2;
    let left = bounds.left + 2;
    let edge = if disabled {
        paint.text_disabled
    } else {
        paint.edge
    };
    let dot = if disabled {
        paint.text_disabled
    } else {
        paint.dot
    };
    let center = PointF::new((left + diameter / 2) as f32, (top + diameter / 2) as f32);

    if !draw_radio_d2d(
        hdc, bounds, center, diameter, checked, focused, edge, dot, paint,
    ) && let (Some(edge_brush), Some(dot_brush)) =
        (crate::gdi::cache_brush(edge), crate::gdi::cache_brush(dot))
    {
        // SAFETY: as above; the stock null pen and cached brushes are live and
        // every selected object is restored.
        unsafe {
            let old_pen = windows::Win32::Graphics::Gdi::SelectObject(dc, GetStockObject(NULL_PEN));
            let old_brush = windows::Win32::Graphics::Gdi::SelectObject(dc, edge_brush.into());
            let _ = Ellipse(dc, left, top, left + diameter, top + diameter);
            windows::Win32::Graphics::Gdi::SelectObject(dc, background.into());
            let _ = Ellipse(
                dc,
                left + 2,
                top + 2,
                left + diameter - 2,
                top + diameter - 2,
            );
            if checked {
                windows::Win32::Graphics::Gdi::SelectObject(dc, dot_brush.into());
                let inset = diameter / 2 - diameter * 22 / 100;
                let _ = Ellipse(
                    dc,
                    left + inset,
                    top + inset,
                    left + diameter - inset,
                    top + diameter - inset,
                );
            }
            windows::Win32::Graphics::Gdi::SelectObject(dc, old_brush);
            windows::Win32::Graphics::Gdi::SelectObject(dc, old_pen);
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

    // SAFETY: `dc` is live for the call; the font is restored afterwards.
    unsafe {
        let text_color = if disabled {
            paint.text_disabled
        } else {
            paint.text
        };
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, COLORREF(text_color.to_colorref()));
        let old_font = windows::Win32::Graphics::Gdi::SelectObject(dc, font.into());
        let mut wide: Vec<u16> = label.encode_utf16().collect();
        let mut cell = RECT {
            left: left + diameter + 6,
            top: bounds.top,
            right: bounds.right,
            bottom: bounds.bottom,
        };
        DrawTextW(
            dc,
            &mut wide,
            &mut cell,
            text_format(&[
                DT_LEFT,
                DT_VCENTER,
                DT_SINGLELINE,
                DT_NOPREFIX,
                DT_END_ELLIPSIS,
            ]),
        );
        windows::Win32::Graphics::Gdi::SelectObject(dc, old_font);
    }
}

/// Draws the radio glyph and focus ring with Direct2D. Returns `false` when
/// Direct2D is unavailable, so the caller can paint them with GDI.
#[allow(clippy::too_many_arguments)]
fn draw_radio_d2d(
    hdc: isize,
    bounds: Rect,
    center: PointF,
    diameter: i32,
    checked: bool,
    focused: bool,
    edge: Color,
    dot: Color,
    paint: &RadioPaint,
) -> bool {
    let Ok(mut canvas) = DcCanvas::new(hdc, full_rect(bounds)) else {
        return false;
    };
    // Half the stroke is outside the radius, so inset by half the 1.5px width.
    let radius = (diameter as f32 / 2.0 - 0.75).max(1.0);
    canvas.stroke_ellipse(center, radius, radius, edge, Stroke::solid(1.5));
    if checked {
        let dot_radius = diameter as f32 * 0.22;
        canvas.fill_ellipse(center, dot_radius, dot_radius, dot);
    }
    if focused {
        let ring = RectF::new(
            bounds.left as f32 + 1.0,
            bounds.top as f32 + 1.0,
            bounds.right as f32 - 1.0,
            bounds.bottom as f32 - 1.0,
        );
        canvas.stroke_rounded_rect(ring, 2.0, paint.focus, Stroke::solid(1.0));
    }
    let _ = canvas.end_draw();
    true
}

/// Theme colours for painting one group box.
pub(crate) struct GroupPaint {
    /// Title text.
    pub text: Color,
    /// Frame.
    pub border: Color,
    /// Control background.
    pub background: Color,
}

/// Paints an owner-drawn group box: background, a single-pixel rounded frame
/// and the title straddling the frame's top edge. The frame is anti-aliased
/// through Direct2D, falling back to GDI's `FrameRect`. See [`draw_radio`] for
/// the object lifetime notes.
pub(crate) fn draw_groupbox(
    hdc: isize,
    bounds: Rect,
    title: &str,
    font: HFONT,
    paint: &GroupPaint,
) {
    if hdc == 0 {
        return;
    }
    let dc = HDC(hdc as *mut c_void);
    let Some(background) = crate::gdi::cache_brush(paint.background) else {
        return;
    };
    let Some(border) = crate::gdi::cache_brush(paint.border) else {
        return;
    };
    let text_size = super::gdi::measure_text(font, title);
    // SAFETY: as in `draw_radio`.
    unsafe {
        FillRect(dc, &rect_of(bounds), background);
    }

    let title_height = text_size.height.max(1);
    let frame = Rect::new(
        bounds.left,
        bounds.top + title_height / 2,
        bounds.right,
        bounds.bottom,
    );
    if !draw_groupbox_d2d(hdc, bounds, frame, paint.border) {
        // SAFETY: as in `draw_radio`; the cached brush is live.
        unsafe {
            FrameRect(dc, &rect_of(frame), border);
        }
    }

    // SAFETY: as in `draw_radio`; the title background erases the frame under
    // the title and the font is restored.
    unsafe {
        let title_left = bounds.left + 8;
        let title_back = RECT {
            left: title_left,
            top: bounds.top,
            right: title_left + text_size.width + 6,
            bottom: bounds.top + title_height,
        };
        FillRect(dc, &title_back, background);

        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, COLORREF(paint.text.to_colorref()));
        let old_font = windows::Win32::Graphics::Gdi::SelectObject(dc, font.into());
        let mut wide: Vec<u16> = title.encode_utf16().collect();
        let mut cell = RECT {
            left: title_left + 3,
            top: bounds.top,
            right: title_back.right,
            bottom: bounds.top + title_height,
        };
        DrawTextW(
            dc,
            &mut wide,
            &mut cell,
            text_format(&[DT_LEFT, DT_TOP, DT_SINGLELINE, DT_NOPREFIX]),
        );
        windows::Win32::Graphics::Gdi::SelectObject(dc, old_font);
    }
}

/// Draws the group box frame with Direct2D. Returns `false` when Direct2D is
/// unavailable, so the caller can paint it with GDI.
fn draw_groupbox_d2d(hdc: isize, bounds: Rect, frame: Rect, border: Color) -> bool {
    let Ok(mut canvas) = DcCanvas::new(hdc, full_rect(bounds)) else {
        return false;
    };
    let shape = RectF::new(
        frame.left as f32 + 0.5,
        frame.top as f32 + 0.5,
        frame.right as f32 - 0.5,
        frame.bottom as f32 - 0.5,
    );
    canvas.stroke_rounded_rect(shape, 4.0, border, Stroke::solid(1.0));
    let _ = canvas.end_draw();
    true
}
