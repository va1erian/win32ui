//! Direct2D: the process factory and the HWND render target.
//!
//! All COM `unsafe` for the safe [`crate::d2d`] module lives here. Direct2D
//! objects are apartment-bound, so the factory is cached per (UI) thread.

mod target;
pub(crate) mod text;

pub(crate) use target::Target;

use std::cell::OnceCell;
use std::mem::ManuallyDrop;

use windows::Win32::Foundation::D2DERR_RECREATE_TARGET;
use windows::Win32::Graphics::Direct2D::{
    D2D1_DASH_STYLE, D2D1_DASH_STYLE_DASH, D2D1_DASH_STYLE_DOT, D2D1_DASH_STYLE_SOLID,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1CreateFactory, ID2D1Factory,
};
use windows::Win32::Graphics::Gdi::ValidateRect;

use crate::d2d::DashStyle;
use crate::error::Result;
use crate::hwnd::Hwnd;

use super::{raw_hwnd, win32_error};

thread_local! {
    /// Never released: dropping it during thread-local teardown at process exit
    /// runs after Direct2D has started shutting down and corrupts the exit code.
    /// One factory per UI thread, for the life of the process.
    static FACTORY: OnceCell<ManuallyDrop<ID2D1Factory>> = const { OnceCell::new() };
}

/// The calling thread's Direct2D factory, created on first use.
fn factory() -> Result<ID2D1Factory> {
    FACTORY.with(|cell| {
        if let Some(factory) = cell.get() {
            return Ok(ID2D1Factory::clone(factory));
        }
        // SAFETY: a single-threaded factory with default options; the result
        // is used only from this thread (it lives in a `thread_local`).
        let factory: ID2D1Factory =
            unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None) }
                .map_err(win32_error)?;
        Ok(ID2D1Factory::clone(
            cell.get_or_init(|| ManuallyDrop::new(factory)),
        ))
    })
}

/// What `EndDraw` reported.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EndDraw {
    /// The frame was presented.
    Presented,
    /// The device was lost (`D2DERR_RECREATE_TARGET`): the target and every
    /// resource made from it must be recreated before the next frame.
    TargetLost,
}

/// Whether an `EndDraw` failure with this `HRESULT` means the render target
/// must be recreated, as opposed to a genuine error.
pub(crate) fn is_target_lost(hresult: i32) -> bool {
    hresult == D2DERR_RECREATE_TARGET.0
}

/// Maps a [`DashStyle`] to Direct2D's built-in dash style.
fn dash_style(dash: DashStyle) -> D2D1_DASH_STYLE {
    match dash {
        DashStyle::Solid => D2D1_DASH_STYLE_SOLID,
        DashStyle::Dashed => D2D1_DASH_STYLE_DASH,
        DashStyle::Dotted => D2D1_DASH_STYLE_DOT,
    }
}

/// Marks the window's whole client area as painted. `EndDraw` does not do this
/// (only `BeginPaint`/`EndPaint` does), and an invalid region left behind makes
/// Windows send `WM_PAINT` forever.
pub(crate) fn validate(hwnd: Hwnd) {
    // SAFETY: `hwnd` is a window handle and a null rectangle means the whole
    // client area; a stale handle makes the call fail harmlessly.
    unsafe {
        let _ = ValidateRect(Some(raw_hwnd(hwnd)), None);
    }
}

/// The `WM_ERASEBKGND` message id.
pub(crate) const WM_ERASEBKGND: u32 = windows::Win32::UI::WindowsAndMessaging::WM_ERASEBKGND;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_recreate_target_is_a_device_loss() {
        assert!(is_target_lost(D2DERR_RECREATE_TARGET.0));
        assert!(!is_target_lost(0));
        assert!(!is_target_lost(windows::Win32::Foundation::E_FAIL.0));
    }

    #[test]
    fn dash_styles_map_to_direct2d() {
        assert_eq!(dash_style(DashStyle::Solid), D2D1_DASH_STYLE_SOLID);
        assert_eq!(dash_style(DashStyle::Dashed), D2D1_DASH_STYLE_DASH);
        assert_eq!(dash_style(DashStyle::Dotted), D2D1_DASH_STYLE_DOT);
    }
}
