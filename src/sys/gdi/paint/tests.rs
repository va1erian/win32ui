//! Tests for the paint session's back-buffer cache and alpha-aware blit.

use windows::Win32::Graphics::Gdi::{GetDC, ReleaseDC};

use super::{acquire_back_buffer, release_back_buffer};
use crate::hwnd::Hwnd;

/// A screen DC is enough to create compatible buffers, so these tests do
/// not need a window or the message loop.
fn screen_dc() -> windows::Win32::Graphics::Gdi::HDC {
    // SAFETY: a null window asks for the screen DC.
    unsafe { GetDC(None) }
}

fn release_dc(dc: windows::Win32::Graphics::Gdi::HDC) {
    // SAFETY: `dc` came from the matching `screen_dc`.
    unsafe {
        let _ = ReleaseDC(None, dc);
    }
}

#[test]
fn back_buffer_is_reused_then_grown() {
    let dc = screen_dc();
    if dc.0.is_null() {
        return;
    }
    let hwnd = Hwnd::from_raw(0x1234);
    let first = acquire_back_buffer(hwnd, dc, 100, 100, 96).expect("buffer");
    let again = acquire_back_buffer(hwnd, dc, 100, 100, 96).expect("reused");
    assert_eq!(first.0, again.0, "the buffer was not reused");
    let grown = acquire_back_buffer(hwnd, dc, 200, 100, 96).expect("grown");
    assert_ne!(first.0, grown.0, "the buffer was not grown");
    release_back_buffer(hwnd);
    let fresh = acquire_back_buffer(hwnd, dc, 100, 100, 96).expect("fresh");
    assert_ne!(grown.0, fresh.0, "the released buffer was reused");
    release_back_buffer(hwnd);
    release_dc(dc);
}

#[test]
fn dpi_change_recreates() {
    let dc = screen_dc();
    if dc.0.is_null() {
        return;
    }
    let hwnd = Hwnd::from_raw(0x5678);
    let normal = acquire_back_buffer(hwnd, dc, 100, 100, 96).expect("buffer");
    let scaled = acquire_back_buffer(hwnd, dc, 100, 100, 144).expect("scaled");
    assert_ne!(normal.0, scaled.0, "the buffer survived a DPI change");
    release_back_buffer(hwnd);
    release_dc(dc);
}

#[test]
fn draw_bitmap_honours_per_pixel_alpha() {
    use windows::Win32::Graphics::Gdi::{GetPixel, HGDIOBJ};

    use crate::color::Color;
    use crate::geometry::{Rect, Size};
    use crate::sys::gdi::{create_dib, delete_object, solid_brush};

    let dc = screen_dc();
    if dc.0.is_null() {
        return;
    }
    let hwnd = Hwnd::from_raw(0x9ABC);
    let buffer = acquire_back_buffer(hwnd, dc, 8, 8, 96).expect("buffer");

    // Paint the buffer an opaque background colour.
    let background = Color::rgb(0x11, 0x22, 0x33);
    let brush = solid_brush(background).expect("brush");
    super::fill_rect(buffer, Rect::new(0, 0, 8, 8), brush);

    // A fully transparent red pixel. `AlphaBlend` must leave the background
    // untouched; the old `SRCCOPY` blit stamped red over it.
    let bitmap = create_dib(1, 1, &[0xFF, 0x00, 0x00, 0x00]).expect("dib");
    super::draw_bitmap(buffer, bitmap, Size::new(1, 1), Rect::new(0, 0, 1, 1));

    // SAFETY: `buffer` is live with its compatible bitmap selected.
    let pixel = unsafe { GetPixel(buffer, 0, 0) };
    assert_eq!(
        Color::from_colorref(pixel.0),
        background,
        "a transparent pixel was not left as the background"
    );

    delete_object(HGDIOBJ(bitmap.0));
    release_back_buffer(hwnd);
    release_dc(dc);
}
