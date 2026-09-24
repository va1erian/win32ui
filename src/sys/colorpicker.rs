//! Raw plumbing for the colour picker: the `ChooseColor` common dialog and the
//! owner-draw painting of the swatch button.
//!
//! All `unsafe` in this crate lives under `sys`; every block below carries a
//! `// SAFETY:` note. Constants come from the `windows` crate (generated from
//! `Commdlg.h`/`CommCtrl.h`).

use core::ffi::c_void;

use windows::Win32::Foundation::{COLORREF, RECT};
use windows::Win32::Graphics::Gdi::{DrawFocusRect, HDC};
use windows::Win32::UI::Controls::Dialogs::{
    CC_ANYCOLOR, CC_FULLOPEN, CC_RGBINIT, CHOOSECOLORW, ChooseColorW,
};
use windows::Win32::UI::Controls::{ODS_DISABLED, ODS_FOCUS};

use crate::color::Color;
use crate::geometry::Rect;
use crate::hwnd::Hwnd;

use super::raw_hwnd;

/// Shows the common `ChooseColor` dialog owned by `owner`, seeded with
/// `initial`, and returns the chosen colour, or `None` when the user cancels.
pub(crate) fn choose_color(owner: Hwnd, initial: Color) -> Option<Color> {
    // The dialog edits this in place and remembers the last custom colours only
    // for the lifetime of the call; win32ui does not persist them.
    let mut custom = [COLORREF(0); 16];
    let mut request = CHOOSECOLORW {
        lStructSize: size_of::<CHOOSECOLORW>() as u32,
        hwndOwner: raw_hwnd(owner),
        rgbResult: COLORREF(initial.to_colorref()),
        lpCustColors: custom.as_mut_ptr(),
        Flags: CC_ANYCOLOR | CC_FULLOPEN | CC_RGBINIT,
        ..Default::default()
    };
    // SAFETY: `request` is fully initialised for the call and `lpCustColors`
    // points at `custom`, which outlives it; the call only reads/writes them.
    let chosen = unsafe { ChooseColorW(&mut request) };
    chosen
        .as_bool()
        .then(|| Color::from_colorref(request.rgbResult.0))
}

/// Paints one owner-drawn colour swatch: the fill, a one-pixel border and, when
/// focused, the standard focus rectangle. Brushes come from the bounded GDI
/// cache, so no object is created per paint.
pub(crate) fn draw_swatch(dc: isize, bounds: Rect, color: Color, border: Color, state: u32) {
    if dc == 0 || bounds.is_empty() {
        return;
    }
    let hdc = HDC(dc as *mut c_void);
    let disabled = state & ODS_DISABLED.0 != 0;
    let fill = if disabled {
        color.lerp(border, 0.5)
    } else {
        color
    };
    if let Some(brush) = crate::gdi::cache_brush(fill) {
        crate::sys::gdi::fill_rect(hdc, bounds.shrink(1), brush);
    }
    if let Some(brush) = crate::gdi::cache_brush(border) {
        let (l, t, r, b) = (bounds.left, bounds.top, bounds.right, bounds.bottom);
        crate::sys::gdi::fill_rect(hdc, Rect::new(l, t, r, t + 1), brush);
        crate::sys::gdi::fill_rect(hdc, Rect::new(l, b - 1, r, b), brush);
        crate::sys::gdi::fill_rect(hdc, Rect::new(l, t, l + 1, b), brush);
        crate::sys::gdi::fill_rect(hdc, Rect::new(r - 1, t, r, b), brush);
    }
    if state & ODS_FOCUS.0 != 0 {
        let focus = RECT {
            left: bounds.left + 2,
            top: bounds.top + 2,
            right: bounds.right - 2,
            bottom: bounds.bottom - 2,
        };
        // SAFETY: `hdc` is the `WM_DRAWITEM` device context, valid for the
        // call, and `focus` is a valid `RECT`.
        unsafe {
            let _ = DrawFocusRect(hdc, &focus);
        }
    }
}
