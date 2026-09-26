#![forbid(unsafe_code)]

//! An RAII window icon built from RGBA pixels.

use windows::Win32::UI::WindowsAndMessaging::HICON;

use crate::capture::RgbaImage;
use crate::error::{Error, Result};
use crate::geometry::Size;
use crate::sys;
use crate::window::Window;

/// A window icon, created from RGBA pixels via `CreateIconIndirect` and
/// destroyed on drop.
///
/// Windows keeps a reference to the icon rather than copying it when it is set
/// with [`Window::set_icon`], so the `Icon` must outlive the window (store it
/// in a field, not a temporary).
pub struct Icon {
    handle: HICON,
    size: Size,
    /// The icon's pixels, straight alpha, top-down. Kept so the strip menu can
    /// draw the icon without re-reading it from the `HICON`.
    rgba: RgbaImage,
}

impl Icon {
    /// Creates an icon from tightly packed RGBA pixels (`width * height * 4`
    /// bytes, row-major, top-down). The alpha channel is honoured.
    pub fn from_rgba(width: i32, height: i32, rgba: &[u8]) -> Result<Icon> {
        if width <= 0 || height <= 0 {
            return Err(Error::Icon("size"));
        }
        let expected = width as usize * height as usize * 4;
        if rgba.len() < expected {
            return Err(Error::Icon("pixel buffer too small"));
        }
        Ok(Icon {
            handle: sys::window_icon::create_icon(width, height, rgba)?,
            size: Size::new(width, height),
            rgba: RgbaImage {
                width: width as u32,
                height: height as u32,
                pixels: rgba[..expected].to_vec(),
            },
        })
    }

    /// Loads the icon resource `id` from this program, at the system icon
    /// size. The returned `Icon` owns its own copy, so it is destroyed with it.
    /// A program's `build.rs`-embedded icon is usually resource id 1.
    pub fn from_resource(id: u16) -> Result<Icon> {
        let (handle, width, height) =
            sys::window_icon::load_icon(id).ok_or(Error::Icon("resource not found"))?;
        match sys::window_icon::icon_rgba(handle) {
            Ok(rgba) => Ok(Icon {
                handle,
                size: Size::new(width, height),
                rgba,
            }),
            Err(error) => {
                sys::window_icon::destroy_icon(handle);
                Err(error)
            }
        }
    }

    /// The icon's dimensions.
    pub fn size(&self) -> Size {
        self.size
    }

    /// The icon's pixels, straight alpha, top-down (`width * height * 4`
    /// bytes). Used to draw the icon in the title strip.
    pub fn rgba(&self) -> &RgbaImage {
        &self.rgba
    }

    pub(crate) fn raw(&self) -> HICON {
        self.handle
    }
}

impl Drop for Icon {
    fn drop(&mut self) {
        sys::window_icon::destroy_icon(self.handle);
    }
}

impl Window {
    /// Sets the window's large and small icons from `icon`.
    ///
    /// The same image backs both sizes; Windows scales it as needed. The icon
    /// must outlive the window (see [`Icon`]).
    pub fn set_icon(&self, icon: &Icon) {
        sys::window_icon::set_icon(self.hwnd(), icon.raw());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_icon_resource_is_an_error() {
        assert!(Icon::from_resource(u16::MAX).is_err());
    }

    #[test]
    fn from_rgba_keeps_the_pixels() {
        let pixels = [
            0xFF, 0x00, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0x80, 0x00, 0x00, 0xFF, 0x40, 0xFF, 0xFF,
            0x00, 0x00,
        ];
        let icon = Icon::from_rgba(2, 2, &pixels).expect("icon");
        assert_eq!(icon.rgba().width, 2);
        assert_eq!(icon.rgba().height, 2);
        assert_eq!(icon.rgba().pixels, pixels);
    }

    #[test]
    fn an_hicon_round_trips_back_to_rgba() {
        // The sys conversion is exercised by feeding it an icon built from known
        // pixels; opaque pixels must survive, and a transparent corner must stay
        // transparent.
        let mut pixels = vec![0u8; 4 * 4 * 4];
        for (index, pixel) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            if index == 0 {
                continue; // stays fully transparent
            }
            *pixel = [0x20, 0x80, 0xE0, 0xFF];
        }
        let icon = Icon::from_rgba(4, 4, &pixels).expect("icon");
        let back = sys::window_icon::icon_rgba(icon.raw()).expect("conversion");
        assert_eq!(back.width, 4);
        assert_eq!(back.height, 4);
        assert_eq!(back.pixels.len(), pixels.len());
        assert_eq!(&back.pixels[4..], &pixels[4..], "opaque pixels survive");
        assert_eq!(back.pixel(0, 0), Some([0, 0, 0, 0]), "transparent stays");
    }
}
