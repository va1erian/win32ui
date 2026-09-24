#![forbid(unsafe_code)]

//! Rendering a window's pixels into an RGBA buffer, for screenshots and visual
//! tests.
//!
//! Three backends, for different needs:
//!
//! * [`Window::capture`] (`PrintWindow`): cheap, works without DWM, but misses
//!   the caption buttons, the frame and the backdrop material, and in practice
//!   needs an active window for Direct2D child panes to be current.
//! * [`Window::capture_screen`] (screen `BitBlt`): includes the DWM output but
//!   requires the window to be on screen and unobscured.
//! * [`capture_hwnd`] (`Windows.Graphics.Capture`, the `wgc` feature): the
//!   exact composited surface — frame, caption buttons, backdrop — even when
//!   the window is occluded or owned by another process, without raising it or
//!   moving the pointer. Prefer it for screenshots and visual tests when the
//!   feature is enabled. Its region is the window's *visible* frame (the DWM
//!   extended frame bounds), which excludes the invisible resize border that
//!   `capture` includes, so the two images differ in size.

use crate::error::Result;
use crate::geometry::Size;
#[cfg(feature = "wgc")]
use crate::hwnd::Hwnd;
use crate::sys;
use crate::window::Window;

/// A tightly packed RGBA image, row-major and top-down: `pixels` holds
/// `width * height * 4` bytes (red, green, blue, alpha per pixel).
///
/// The channels are *straight* (not premultiplied) alpha, so the image can be
/// composited over any background: a translucent pixel's colour is its own, not
/// already scaled by its alpha. A fully transparent pixel is `[0, 0, 0, 0]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RgbaImage {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// The pixel data, four bytes per pixel.
    pub pixels: Vec<u8>,
}

impl RgbaImage {
    /// The image dimensions.
    pub fn size(&self) -> Size {
        Size::new(self.width as i32, self.height as i32)
    }

    /// The RGBA value at `(x, y)`, or `None` if the point is outside the image.
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let offset = (y as usize * self.width as usize + x as usize) * 4;
        let pixel = self.pixels.get(offset..offset + 4)?;
        Some([pixel[0], pixel[1], pixel[2], pixel[3]])
    }
}

impl Window {
    /// Renders this window into an [`RgbaImage`].
    ///
    /// Uses `PrintWindow` with `PW_RENDERFULLCONTENT`, so the result is correct
    /// even when the window is occluded or draws with DirectComposition. The
    /// captured region is the whole window (frame included) and the alpha
    /// channel is forced to 255.
    pub fn capture(&self) -> Result<RgbaImage> {
        let size = self.window_rect().size();
        let captured = sys::capture::capture(self.hwnd(), size.width, size.height)?;
        Ok(RgbaImage {
            width: captured.width as u32,
            height: captured.height as u32,
            pixels: captured.pixels,
        })
    }

    /// Renders this window's DWM-composited surface into an [`RgbaImage`].
    ///
    /// Uses `Windows.Graphics.Capture`, so the result includes everything DWM
    /// draws — the caption buttons, the frame, rounded corners and the
    /// backdrop material — and is correct even when the window is occluded or
    /// behind another window. Unlike [`capture_screen`](Window::capture_screen),
    /// the window never has to be raised, focused or unoccluded, and the
    /// pointer is never moved. Requires the `wgc` feature.
    ///
    /// The captured region is the window's *visible* frame — the DWM extended
    /// frame bounds — which excludes the invisible resize border and drop
    /// shadow that [`capture`](Window::capture) and
    /// [`window_rect`](Window::window_rect) include, so its size is slightly
    /// smaller. The pixels are straight RGBA (premultiplied alpha is undone).
    /// Returns
    /// [`CaptureError::Minimized`](crate::CaptureError::Minimized) for a
    /// minimised window (which it does not restore).
    #[cfg(feature = "wgc")]
    pub fn capture_composited(&self) -> Result<RgbaImage> {
        capture_hwnd(self.hwnd())
    }

    /// Renders the window's screen rectangle into an [`RgbaImage`], via a
    /// `BitBlt` from the screen DC.
    ///
    /// Unlike [`capture`](Window::capture), this includes everything DWM draws
    /// on screen: the caption buttons, the frame and the backdrop material —
    /// none of which `PrintWindow` reproduces. The window must be on screen and
    /// unobscured for the result to be meaningful. The captured region is the
    /// whole window (frame included) and the alpha channel is forced to 255.
    pub fn capture_screen(&self) -> Result<RgbaImage> {
        let rect = self.window_rect();
        let captured = sys::capture::capture_screen(rect)?;
        Ok(RgbaImage {
            width: captured.width as u32,
            height: captured.height as u32,
            pixels: captured.pixels,
        })
    }
}

/// Captures the DWM-composited surface of any top-level window handle.
///
/// Works on a window this process does not own (for example an app a test
/// harness or an agent launched), without raising it, focusing it or moving
/// the pointer, and even when it is occluded. The captured region is the
/// window's *visible* frame (the DWM extended frame bounds), which excludes the
/// invisible resize border and drop shadow that `Window::window_rect` and
/// `Window::capture` include; the pixels are straight RGBA. Requires the `wgc`
/// feature.
///
/// The crate does not initialise COM; `Windows.Graphics.Capture` expects an
/// already-initialised, usually single-threaded (`STA`) apartment on the
/// calling thread. A caller that has initialised COM with another threading
/// model may get `RPC_E_CHANGED_MODE`. The capture itself does not need a
/// message loop and does not touch focus or the pointer.
///
/// Returns [`CaptureError::Minimized`](crate::CaptureError::Minimized) for a
/// minimised window, [`CaptureError::EmptyWindow`](crate::CaptureError::EmptyWindow)
/// when the window has no content, [`CaptureError::Timeout`](crate::CaptureError::Timeout)
/// if no frame arrives within about a second, and
/// [`CaptureError::Unavailable`](crate::CaptureError::Unavailable) when
/// Windows.Graphics.Capture is not available on the system.
#[cfg(feature = "wgc")]
pub fn capture_hwnd(hwnd: Hwnd) -> Result<RgbaImage> {
    let captured = sys::capture_wgc::capture(hwnd)?;
    Ok(RgbaImage {
        width: captured.width as u32,
        height: captured.height as u32,
        pixels: captured.pixels,
    })
}

/// The captured-image type a frontend usually needs.
pub mod prelude {
    pub use super::RgbaImage;
}
