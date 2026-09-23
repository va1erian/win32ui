//! Owner-drawn radio and group box painting (`BS_OWNERDRAW` + `WM_DRAWITEM`).
//!
//! `DarkMode_Explorer` darkens push buttons and check boxes, but two native
//! parts ignore it: radio button text stays `COLOR_BTNTEXT` (black) and the
//! group box frame paints classic-light. Both are created with `BS_OWNERDRAW`
//! and painted here from the window's theme tokens instead (documented APIs
//! only: `DrawTextW`, `Ellipse`, `FrameRect`, `DrawFocusRect`).

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
    /// Control background.
    pub background: Color,
}

/// Paints an owner-drawn radio button: background, ring glyph with dot, label
/// and an optional focus rectangle. Brushes come from the bounded GDI cache;
/// the ring is drawn with the stock null pen, so no GDI object is created.
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
    let focused = draw_focused(state);
    let dc = HDC(hdc as *mut c_void);
    let Some(background) = crate::gdi::cache_brush(paint.background) else {
        return;
    };
    let Some(edge) = crate::gdi::cache_brush(if disabled {
        paint.text_disabled
    } else {
        paint.edge
    }) else {
        return;
    };
    let Some(dot) = crate::gdi::cache_brush(if disabled {
        paint.text_disabled
    } else {
        paint.dot
    }) else {
        return;
    };
    // SAFETY: `dc` is the `WM_DRAWITEM` device context, valid for the call;
    // stock objects need no cleanup and cached brushes stay alive in the
    // bounded per-thread GDI cache. Every selected object is restored.
    unsafe {
        let null_pen = GetStockObject(NULL_PEN);
        let mut area = RECT {
            left: bounds.left,
            top: bounds.top,
            right: bounds.right,
            bottom: bounds.bottom,
        };
        FillRect(dc, &area, background);

        let diameter = (bounds.height() - 6).clamp(12, 20);
        let top = bounds.top + (bounds.height() - diameter) / 2;
        let left = bounds.left + 2;
        let old_pen = windows::Win32::Graphics::Gdi::SelectObject(dc, null_pen);
        let old_brush = windows::Win32::Graphics::Gdi::SelectObject(dc, edge.into());
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
            windows::Win32::Graphics::Gdi::SelectObject(dc, dot.into());
            let dot_inset = diameter / 2 - diameter * 22 / 100;
            let _ = Ellipse(
                dc,
                left + dot_inset,
                top + dot_inset,
                left + diameter - dot_inset,
                top + diameter - dot_inset,
            );
        }
        windows::Win32::Graphics::Gdi::SelectObject(dc, old_brush);
        windows::Win32::Graphics::Gdi::SelectObject(dc, old_pen);

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

        if focused && !disabled {
            area.left += 1;
            area.top += 1;
            area.right -= 1;
            area.bottom -= 1;
            let _ = DrawFocusRect(dc, &area);
        }
    }
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

/// Paints an owner-drawn group box: background, a single-pixel frame and the
/// title straddling the frame's top edge. See [`draw_radio`] for the object
/// lifetime notes.
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
        let area = RECT {
            left: bounds.left,
            top: bounds.top,
            right: bounds.right,
            bottom: bounds.bottom,
        };
        FillRect(dc, &area, background);

        let title_height = text_size.height.max(1);
        let frame = RECT {
            left: bounds.left,
            top: bounds.top + title_height / 2,
            right: bounds.right,
            bottom: bounds.bottom,
        };
        FrameRect(dc, &frame, border);

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
