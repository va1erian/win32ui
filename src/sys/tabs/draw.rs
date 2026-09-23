//! Owner-drawn tab painting (`WM_DRAWITEM` and the strip chrome pass).

use core::ffi::c_void;

use windows::Win32::Graphics::Gdi::{
    DT_CENTER, DT_END_ELLIPSIS, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DrawFocusRect, HDC, HFONT,
};
use windows::Win32::UI::Controls::{ODS_DISABLED, ODS_FOCUS, ODS_SELECTED};

use crate::color::Color;
use crate::d2d::{DcCanvas, RectF};
use crate::geometry::Rect;

/// The states an owner-drawn tab can be painted in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TabVisual {
    /// The selected tab (`ODS_SELECTED`).
    pub selected: bool,
    /// The tab control has keyboard focus (`ODS_FOCUS`).
    pub focused: bool,
    /// The pointer is over this tab (tracked by [`TabHost`](super::TabHost)).
    pub hot: bool,
    /// The tab is disabled (`ODS_DISABLED`).
    pub disabled: bool,
}

/// Decodes the `WM_DRAWITEM` `itemState` for a tab.
pub(crate) fn decode_state(state: u32) -> TabVisual {
    TabVisual {
        selected: state & ODS_SELECTED.0 != 0,
        focused: state & ODS_FOCUS.0 != 0,
        hot: false,
        disabled: state & ODS_DISABLED.0 != 0,
    }
}

/// Semantic colours for painting one tab.
pub(crate) struct TabPaint {
    /// Strip background for unselected tabs.
    pub background: Color,
    /// Selected tab fill.
    pub raised: Color,
    /// Hover fill.
    pub hover: Color,
    /// Selected tab text.
    pub text: Color,
    /// Unselected tab text.
    pub text_secondary: Color,
    /// Disabled tab text.
    pub text_disabled: Color,
    /// Accent indicator under the selected tab.
    pub accent: Color,
}

/// The control's client rectangle for binding a DC render target, whose origin
/// is the control's top-left.
fn full_rect(bounds: Rect) -> Rect {
    Rect::new(0, 0, bounds.right.max(1), bounds.bottom.max(1))
}

/// Fills `bounds` with `color` in a raw device context (e.g. from
/// `WM_ERASEBKGND` or `WM_DRAWITEM`).
pub(crate) fn fill(hdc: isize, bounds: Rect, color: Color) {
    if hdc == 0 || bounds.is_empty() {
        return;
    }
    if let Some(brush) = crate::gdi::cache_brush(color) {
        crate::sys::gdi::fill_rect(HDC(hdc as *mut c_void), bounds, brush);
    }
}

/// Paints one owner-drawn tab: state fill, centred label, an accent indicator
/// under the selected tab and a focus rectangle. Brushes come from the bounded
/// GDI cache; the caller's font is restored before returning.
pub(crate) fn draw_tab(
    hdc: isize,
    bounds: Rect,
    text: &str,
    font: HFONT,
    visual: TabVisual,
    paint: &TabPaint,
) {
    if hdc == 0 || bounds.is_empty() {
        return;
    }
    let dc = HDC(hdc as *mut c_void);
    let fill_color = if visual.selected {
        paint.raised
    } else if visual.hot && !visual.disabled {
        paint.hover
    } else {
        paint.background
    };
    fill(hdc, bounds, fill_color);

    let text_color = if visual.disabled {
        paint.text_disabled
    } else if visual.selected || visual.hot {
        paint.text
    } else {
        paint.text_secondary
    };
    let padding = 10;
    let text_rect = Rect::new(
        bounds.left + padding,
        bounds.top,
        (bounds.right - padding).max(bounds.left + padding),
        bounds.bottom,
    );
    let format = DT_CENTER.0 | DT_VCENTER.0 | DT_SINGLELINE.0 | DT_NOPREFIX.0 | DT_END_ELLIPSIS.0;
    // SAFETY: `dc` is the `WM_DRAWITEM` device context, valid for the call;
    // `select_font`/`select_object` restore the previous font.
    let previous = crate::sys::gdi::select_font(dc, font);
    crate::sys::gdi::draw_text(dc, text_rect, text, text_color, format);
    crate::sys::gdi::select_object(dc, previous);

    if visual.selected {
        let underline = Rect::new(bounds.left, bounds.bottom - 2, bounds.right, bounds.bottom);
        // The accent bar's rounded ends are anti-aliased; GDI's square fill is
        // the fallback.
        let drawn = DcCanvas::new(hdc, full_rect(bounds))
            .ok()
            .map(|mut canvas| {
                canvas.fill_rounded_rect(RectF::from_rect(underline), 1.0, paint.accent);
                let _ = canvas.end_draw();
            })
            .is_some();
        if !drawn {
            fill(hdc, underline, paint.accent);
        }
    }
    if visual.focused && visual.selected {
        let focus = windows::Win32::Foundation::RECT {
            left: bounds.left + 2,
            top: bounds.top + 2,
            right: bounds.right - 2,
            bottom: bounds.bottom - 3,
        };
        // SAFETY: `dc` is live for the call and `focus` is a valid `RECT`.
        unsafe {
            let _ = DrawFocusRect(dc, &focus);
        }
    }
}
