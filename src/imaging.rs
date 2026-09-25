#![forbid(unsafe_code)]

//! Image decoding, scaling and JPEG encoding through the Windows Imaging
//! Component (WIC).
//!
//! This is the system-codec alternative to bundling an image decoder: JPEG,
//! PNG and the other WIC container formats are handled by Windows, and the
//! resampler is WIC's. Images are exchanged as [`RgbaImage`], the same
//! straight-alpha buffer type [`capture`](crate::capture) returns.
//!
//! The entry points are usable from any thread (each call brackets itself with
//! a COM apartment), so an image cache can decode on its worker pool without
//! the UI thread's COM setup.
//!
//! ```
//! # let jpeg_bytes: &[u8] = &[];
//! if let Ok(image) = win32ui::imaging::decode(jpeg_bytes) {
//!     let thumb = win32ui::imaging::resize(&image, 64, 64).unwrap();
//!     let _encoded = win32ui::imaging::encode_jpeg(&thumb).unwrap();
//! }
//! ```

use std::path::Path;

use crate::capture::RgbaImage;
use crate::error::Result;
use crate::sys;

/// Decodes `bytes` (JPEG, PNG, BMP, GIF, TIFF, ...) into an RGBA image.
///
/// Returns [`Error::Imaging`](crate::Error::Imaging) with
/// [`ImagingError::UnsupportedFormat`](crate::ImagingError::UnsupportedFormat)
/// when the bytes are not an image WIC can decode.
pub fn decode(bytes: &[u8]) -> Result<RgbaImage> {
    sys::imaging::decode(bytes)
}

/// Reads `path` and decodes it into an RGBA image.
///
/// A missing or unreadable file is reported as
/// [`ImagingError::UnsupportedFormat`](crate::ImagingError::UnsupportedFormat),
/// the same as undecodable bytes, so callers can treat any failure as "no
/// image".
pub fn decode_file(path: &Path) -> Result<RgbaImage> {
    match std::fs::read(path) {
        Ok(bytes) => sys::imaging::decode(&bytes),
        Err(_) => Err(crate::Error::Imaging(
            crate::ImagingError::UnsupportedFormat,
        )),
    }
}

/// Stretches `image` to exactly `width`-by-`height` pixels.
///
/// The aspect ratio is not preserved; callers that want it compute the target
/// size themselves. Returns the input unchanged when the size already matches.
pub fn resize(image: &RgbaImage, width: u32, height: u32) -> Result<RgbaImage> {
    sys::imaging::resize(image, width, height)
}

/// Encodes `image` as JPEG bytes for on-disk caching. The alpha channel is
/// dropped; the quality is WIC's default.
pub fn encode_jpeg(image: &RgbaImage) -> Result<Vec<u8>> {
    sys::imaging::encode_jpeg(image)
}

/// The imaging entry points a frontend usually needs.
pub mod prelude {
    pub use super::{decode, decode_file, encode_jpeg, resize};
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: u32, height: u32, rgba: [u8; 4]) -> RgbaImage {
        RgbaImage {
            width,
            height,
            pixels: rgba
                .iter()
                .copied()
                .cycle()
                .take((width * height * 4) as usize)
                .collect(),
        }
    }

    #[test]
    fn jpeg_round_trip_keeps_dimensions() {
        let image = solid(16, 8, [200, 40, 10, 255]);
        let encoded = encode_jpeg(&image).expect("encode");
        let decoded = decode(&encoded).expect("decode");
        assert_eq!((decoded.width, decoded.height), (16, 8));
    }

    #[test]
    fn resize_fills_the_requested_size() {
        let image = solid(10, 10, [10, 20, 30, 255]);
        let scaled = resize(&image, 4, 6).expect("resize");
        assert_eq!((scaled.width, scaled.height), (4, 6));
        assert_eq!(scaled.pixels.len(), 4 * 6 * 4);
    }

    #[test]
    fn decode_file_reads_from_disk() {
        let image = solid(5, 5, [1, 2, 3, 255]);
        let encoded = encode_jpeg(&image).expect("encode");
        let path = std::env::temp_dir().join(format!("win32ui-imaging-{}.jpg", std::process::id()));
        std::fs::write(&path, &encoded).expect("write");
        let decoded = decode_file(&path).expect("decode_file");
        let _ = std::fs::remove_file(&path);
        assert_eq!((decoded.width, decoded.height), (5, 5));
    }

    #[test]
    fn unsupported_bytes_are_rejected() {
        assert!(decode(b"not an image at all").is_err());
    }
}
