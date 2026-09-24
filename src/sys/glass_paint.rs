//! Buffered, opaque painting of a native control's client area and frame, for
//! [`glass_child`](super::glass_child).
//!
//! GDI leaves the alpha byte of every pixel it draws at zero. The control draws
//! into a buffered-paint DIB (`BeginBufferedPaint`) and the buffer's alpha is
//! set to 255 (`BufferedPaintSetAlpha`) before it is copied to the target: the
//! documented way to draw GDI content over DWM glass. Buffered paint reuses its
//! buffers, so a paint creates no GDI objects once warm.
//!
//! The client area is drawn with `WM_ERASEBKGND` and `WM_PAINT` carrying the
//! buffer's DC in `wParam`, which the common controls honour; `Edit` ignores
//! `WM_PRINTCLIENT`. The frame is drawn with `WM_PRINT`.
//!
//! All `unsafe` in this crate lives under `sys`; every block below carries a
//! `// SAFETY:` note.

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetWindowDC, HDC, MapWindowPoints, ReleaseDC};
use windows::Win32::UI::Controls::{
    BP_PAINTPARAMS, BPBF_TOPDOWNDIB, BPPF_NONCLIENT, BeginBufferedPaint, BufferedPaintSetAlpha,
    EndBufferedPaint,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClientRect, GetWindowRect, PRF_NONCLIENT, WM_ERASEBKGND, WM_PAINT, WM_PRINT,
};

/// How a message reaches the control's own painting: `DefSubclassProc` from
/// inside a subclass (so it does not re-enter it), `SendMessageW` otherwise.
pub(super) type Send = unsafe fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

/// Has `hwnd` erase and paint its client area into `dc` (zero alpha).
pub(super) fn print_client(hwnd: HWND, dc: HDC, send: Send) {
    let wparam = WPARAM(dc.0 as usize);
    // SAFETY: `hwnd` is live and draws into the live DC `dc` for the duration
    // of the calls; `send` delivers the messages synchronously.
    unsafe {
        send(hwnd, WM_ERASEBKGND, wparam, LPARAM(0));
        send(hwnd, WM_PAINT, wparam, LPARAM(0));
    }
}

/// Paints `hwnd`'s client area inside `rect` into `target` through an opaque
/// buffer. Returns whether the buffer could be created; buffered painting must
/// be initialised on this thread (`BufferedPaintInit`).
pub(super) fn paint_client(hwnd: HWND, target: HDC, rect: &RECT, send: Send) -> bool {
    let mut buffer_dc = HDC::default();
    // SAFETY: `target` is a live DC and `rect` a valid rectangle for the call;
    // `buffer_dc` receives the buffer's DC, valid until `EndBufferedPaint`.
    let buffer = unsafe { BeginBufferedPaint(target, rect, BPBF_TOPDOWNDIB, None, &mut buffer_dc) };
    if buffer == 0 {
        return false;
    }
    print_client(hwnd, buffer_dc, send);
    // SAFETY: `buffer` is the live paint buffer begun above; it is made opaque,
    // copied to `target` and ended exactly once.
    unsafe {
        let _ = BufferedPaintSetAlpha(buffer, None, 255);
        let _ = EndBufferedPaint(buffer, true);
    }
    true
}

/// Paints `hwnd`'s non-client frame (a border) through an opaque buffer.
/// Returns whether it did; `false` leaves the frame to the default painting.
pub(super) fn paint_frame(hwnd: HWND, send: Send) -> bool {
    let (mut window, mut client) = (RECT::default(), RECT::default());
    let mut corners = [POINT::default(); 2];
    // SAFETY: both rectangles and the corner array are valid out-pointers for a
    // live window; the client corners are mapped to screen coordinates.
    unsafe {
        if GetWindowRect(hwnd, &mut window).is_err() || GetClientRect(hwnd, &mut client).is_err() {
            return false;
        }
        corners[1] = POINT {
            x: client.right,
            y: client.bottom,
        };
        MapWindowPoints(Some(hwnd), None, &mut corners);
    }
    // The client rectangle relative to the window's top-left corner.
    let client = RECT {
        left: corners[0].x - window.left,
        top: corners[0].y - window.top,
        right: corners[1].x - window.left,
        bottom: corners[1].y - window.top,
    };
    let frame = RECT {
        left: 0,
        top: 0,
        right: window.right - window.left,
        bottom: window.bottom - window.top,
    };
    if client == frame {
        return false;
    }
    let params = BP_PAINTPARAMS {
        cbSize: core::mem::size_of::<BP_PAINTPARAMS>() as u32,
        dwFlags: BPPF_NONCLIENT,
        prcExclude: &client,
        pBlendFunction: core::ptr::null(),
    };
    // SAFETY: the window DC is released below; `params` and `client` outlive
    // the buffered paint, which ends before this block does.
    unsafe {
        let dc = GetWindowDC(Some(hwnd));
        if dc.is_invalid() {
            return false;
        }
        let mut buffer_dc = HDC::default();
        let buffer = BeginBufferedPaint(dc, &frame, BPBF_TOPDOWNDIB, Some(&params), &mut buffer_dc);
        let painted = buffer != 0;
        if painted {
            send(
                hwnd,
                WM_PRINT,
                WPARAM(buffer_dc.0 as usize),
                LPARAM(PRF_NONCLIENT as isize),
            );
            let _ = BufferedPaintSetAlpha(buffer, None, 255);
            let _ = EndBufferedPaint(buffer, true);
        }
        ReleaseDC(Some(hwnd), dc);
        painted
    }
}

#[cfg(test)]
mod tests {
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
        DeleteDC, DeleteObject, SelectObject,
    };
    use windows::Win32::UI::Controls::{BufferedPaintInit, BufferedPaintUnInit};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, SendMessageW, WINDOW_STYLE, WS_EX_NOACTIVATE,
        WS_EX_TOOLWINDOW, WS_POPUP, WS_VISIBLE,
    };
    use windows::core::w;

    use super::*;

    /// `SendMessageW` with the [`Send`] signature.
    unsafe fn send(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        // SAFETY: the caller upholds `SendMessageW`'s contract.
        unsafe { SendMessageW(hwnd, msg, Some(wparam), Some(lparam)) }
    }

    const WIDTH: i32 = 120;
    const HEIGHT: i32 = 24;

    /// Paints the edit's client area into a zeroed 32-bpp DIB and returns its
    /// pixels (BGRA).
    fn render(edit: HWND, opaque: bool) -> Vec<u32> {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: core::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: WIDTH,
                biHeight: -HEIGHT,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        // SAFETY: a memory DC and a DIB section owned by this function; the
        // bitmap's pixels are read while it is alive and both are freed below.
        unsafe {
            let dc = CreateCompatibleDC(None);
            let mut bits = core::ptr::null_mut();
            let bitmap =
                CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0).expect("dib");
            let old = SelectObject(dc, bitmap.into());
            let rect = RECT {
                left: 0,
                top: 0,
                right: WIDTH,
                bottom: HEIGHT,
            };
            if opaque {
                assert!(paint_client(edit, dc, &rect, send), "buffered paint failed");
            } else {
                print_client(edit, dc, send);
            }
            let pixels =
                core::slice::from_raw_parts(bits as *const u32, (WIDTH * HEIGHT) as usize).to_vec();
            SelectObject(dc, old);
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(dc);
            pixels
        }
    }

    /// Plain GDI painting leaves alpha at zero (what DWM drops over the
    /// material); the buffered path makes every pixel opaque and still draws
    /// the control's text.
    #[test]
    fn the_buffered_paint_is_opaque_and_keeps_the_text() {
        // An edit only paints while visible: this one is shown off-screen,
        // without activation or a taskbar button.
        // SAFETY: a parentless `EDIT` window, destroyed below; buffered
        // painting is initialised and uninitialised on this thread around it.
        let edit = unsafe {
            BufferedPaintInit().expect("buffered paint init");
            CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                w!("EDIT"),
                w!("Hello glass"),
                WINDOW_STYLE(WS_POPUP.0 | WS_VISIBLE.0),
                -32000,
                -32000,
                WIDTH,
                HEIGHT,
                None,
                None,
                None,
                None,
            )
        }
        .expect("edit");

        let plain = render(edit, false);
        assert!(
            plain.iter().all(|pixel| pixel >> 24 == 0),
            "GDI was expected to leave alpha at zero"
        );

        let opaque = render(edit, true);
        assert!(
            opaque.iter().all(|pixel| pixel >> 24 == 0xFF),
            "every pixel should be opaque"
        );
        let background = opaque[0] & 0x00FF_FFFF;
        assert!(
            opaque.iter().any(|pixel| pixel & 0x00FF_FFFF != background),
            "the edit's text should be drawn into the buffer"
        );

        // SAFETY: the edit was created above and is destroyed once; the uninit
        // balances the init above.
        unsafe {
            let _ = DestroyWindow(edit);
            let _ = BufferedPaintUnInit();
        }
    }
}
