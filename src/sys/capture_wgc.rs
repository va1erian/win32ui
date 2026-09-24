//! Occlusion-proof window capture through Windows.Graphics.Capture.
//!
//! This is the `wgc` backend behind [`crate::sys::capture`]: it grabs the
//! DWM-composited surface of an `HWND` (caption buttons, frame, Mica/acrylic
//! backdrop and all) even when the window is occluded, without raising it,
//! moving the pointer or taking focus.
//!
//! The captured region is the window's *visible* frame (the DWM extended frame
//! bounds), which excludes the invisible resize border and the drop shadow that
//! `GetWindowRect` includes; the two rectangles differ by a few pixels on a
//! standard window.

use core::time::Duration;
use std::thread::sleep;
use std::time::Instant;

use windows::Graphics::Capture::{
    Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Foundation::{
    E_NOINTERFACE, REGDB_E_CLASSNOTREG, RO_E_METADATA_NAME_NOT_FOUND,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_STAGING, ID3D11Resource, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::{DXGI_ERROR_DEVICE_REMOVED, DXGI_ERROR_DEVICE_RESET};
use windows::Win32::System::WinRT::Direct3D11::IDirect3DDxgiInterfaceAccess;
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::UI::WindowsAndMessaging::IsIconic;
use windows::core::{Interface, factory};

use crate::error::{CaptureError, Error, Result, Win32Error};
use crate::hwnd::Hwnd;

use super::capture::Captured;
use super::capture_wgc_device::{self, CaptureDevice};
use super::raw_hwnd;

/// How long to wait for the first frame before giving up. The first frame of a
/// freshly created capture session usually arrives in tens of milliseconds.
const FIRST_FRAME_TIMEOUT: Duration = Duration::from_millis(1000);
/// How often the free-threaded frame pool is polled while waiting.
const FRAME_POLL_INTERVAL: Duration = Duration::from_millis(10);
/// The pixel format the capture pool is created with: 8 bits per channel,
/// BGRA order, premultiplied alpha.
const FORMAT: DirectXPixelFormat = DirectXPixelFormat::B8G8R8A8UIntNormalized;

/// Captures the composited surface of `hwnd` into a top-down RGBA buffer.
///
/// Works on any top-level window, including one owned by another process, and
/// never touches focus or the pointer. See [`crate::sys::capture`] for the
/// `PrintWindow` and screen-`BitBlt` alternatives.
pub(crate) fn capture(hwnd: Hwnd) -> Result<Captured> {
    // A handle to a window that has since been destroyed is not a generic Win32
    // failure; the caller can retry once it has a live window.
    if !hwnd.is_alive() {
        return Err(Error::WindowDestroyed);
    }
    // A minimised window has no composited surface; DWM does not draw it at
    // all. Do not restore it on the caller's behalf.
    // SAFETY: `hwnd` is live; `IsIconic` only reads it.
    if unsafe { IsIconic(raw_hwnd(hwnd)) }.as_bool() {
        return Err(Error::Capture(CaptureError::Minimized));
    }
    // The capture stack is absent on some systems (and some service sessions);
    // report that as `Unavailable` so the caller can fall back to `PrintWindow`.
    if !GraphicsCaptureSession::IsSupported()
        .map_err(|_| Error::Capture(CaptureError::Unavailable))?
    {
        return Err(Error::Capture(CaptureError::Unavailable));
    }

    let item = capture_item(hwnd)?;
    let size = item.Size().map_err(capture_error)?;
    if size.Width <= 0 || size.Height <= 0 {
        return Err(Error::Capture(CaptureError::EmptyWindow));
    }

    let device = capture_wgc_device::create()?;
    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(&device.device, FORMAT, 1, size)
        .map_err(capture_error)?;
    let session = Session::start(pool, &item)?;
    grab(&device, &session)
}

/// Owns the capture session's COM objects and closes them deterministically,
/// including when an early error return would otherwise skip the cleanup.
struct Session {
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
}

impl Session {
    /// Creates, configures and starts a capture session over `item`.
    fn start(pool: Direct3D11CaptureFramePool, item: &GraphicsCaptureItem) -> Result<Session> {
        let session = pool.CreateCaptureSession(item).map_err(capture_error)?;
        // Both are Windows 11 properties; on older builds the casts fail and the
        // defaults (cursor off for a window capture, border on) stand. Best
        // effort.
        let _ = session.SetIsBorderRequired(false);
        let _ = session.SetIsCursorCaptureEnabled(false);
        session.StartCapture().map_err(capture_error)?;
        Ok(Session { pool, session })
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}

/// A fresh `GraphicsCaptureItem` for `hwnd`, via the documented interop
/// factory. Works across processes.
fn capture_item(hwnd: Hwnd) -> Result<GraphicsCaptureItem> {
    let interop: IGraphicsCaptureItemInterop =
        factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().map_err(capture_error)?;
    // SAFETY: `hwnd` is a live window handle supplied by the caller; the
    // interop call takes it by value and does not retain it.
    unsafe { interop.CreateForWindow::<GraphicsCaptureItem>(raw_hwnd(hwnd)) }.map_err(capture_error)
}

/// Waits for one frame and returns it cropped to the window's current content
/// size.
///
/// A free-threaded pool delivers frames on its own thread, so this never needs
/// to pump messages and cannot re-enter the caller's message loop.
fn grab(device: &CaptureDevice, session: &Session) -> Result<Captured> {
    let frame = wait_for_frame(&session.pool)?;
    let content = frame_content_size(&frame)?;
    let texture = frame_texture(&frame)?;
    let (texture_w, texture_h) = texture_size(&texture);

    // The window may have been resized after the pool was created. If it grew,
    // the pool's surface is too small for the new content: resize the pool and
    // take one more frame. If it shrank, cropping to `content` is enough.
    if content.Width > texture_w || content.Height > texture_h {
        let _ = frame.Close();
        session
            .pool
            .Recreate(&device.device, FORMAT, 1, content)
            .map_err(capture_error)?;
        let frame = wait_for_frame(&session.pool)?;
        let content = frame_content_size(&frame)?;
        let texture = frame_texture(&frame)?;
        let captured = copy_texture(device, &texture, content);
        let _ = frame.Close();
        return captured;
    }

    let captured = copy_texture(device, &texture, content);
    let _ = frame.Close();
    captured
}

/// Polls the free-threaded pool until a frame arrives or the timeout elapses.
fn wait_for_frame(pool: &Direct3D11CaptureFramePool) -> Result<Direct3D11CaptureFrame> {
    let deadline = Instant::now() + FIRST_FRAME_TIMEOUT;
    loop {
        let error = match pool.TryGetNextFrame() {
            Ok(frame) => return Ok(frame),
            Err(error) => error,
        };
        if Instant::now() >= deadline {
            // An empty (null) frame is reported as `S_OK` and just means no
            // frame has arrived yet; any other code is a real failure.
            return Err(if error.code().is_ok() {
                Error::Capture(CaptureError::Timeout)
            } else {
                capture_error(error)
            });
        }
        sleep(FRAME_POLL_INTERVAL);
    }
}

/// The window's current content size, which can differ from the frame pool's.
fn frame_content_size(frame: &Direct3D11CaptureFrame) -> Result<SizeInt32> {
    let size = frame.ContentSize().map_err(capture_error)?;
    if size.Width <= 0 || size.Height <= 0 {
        return Err(Error::Capture(CaptureError::EmptyWindow));
    }
    Ok(size)
}

/// The `ID3D11Texture2D` backing a frame's surface.
fn frame_texture(frame: &Direct3D11CaptureFrame) -> Result<ID3D11Texture2D> {
    let surface = frame.Surface().map_err(capture_error)?;
    let access: IDirect3DDxgiInterfaceAccess = surface.cast().map_err(capture_error)?;
    // SAFETY: the frame's surface backs an `ID3D11Texture2D`; the interop call
    // returns a new reference to it.
    unsafe { access.GetInterface() }.map_err(capture_error)
}

/// The pixel size of a texture.
fn texture_size(texture: &ID3D11Texture2D) -> (i32, i32) {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    // SAFETY: `desc` is a valid out-pointer for a live texture.
    unsafe { texture.GetDesc(&mut desc) };
    (desc.Width as i32, desc.Height as i32)
}

/// Copies `content` (the visible region) of `texture` into a CPU-readable
/// staging texture and converts its premultiplied BGRA8 pixels to straight RGBA.
fn copy_texture(
    device: &CaptureDevice,
    texture: &ID3D11Texture2D,
    content: SizeInt32,
) -> Result<Captured> {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    // SAFETY: `desc` is a valid out-pointer for a live texture.
    unsafe { texture.GetDesc(&mut desc) };
    let width = content.Width.min(desc.Width as i32).max(0) as usize;
    let height = content.Height.min(desc.Height as i32).max(0) as usize;
    if width == 0 || height == 0 {
        return Err(Error::Capture(CaptureError::EmptyWindow));
    }

    let mut staging_desc = desc;
    staging_desc.Usage = D3D11_USAGE_STAGING;
    staging_desc.BindFlags = 0;
    staging_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
    staging_desc.MiscFlags = 0;

    let mut staging: Option<ID3D11Texture2D> = None;
    // SAFETY: `staging_desc` is initialised, no initial data is passed, and
    // `staging` is a valid out-pointer.
    unsafe {
        device
            .d3d
            .CreateTexture2D(&staging_desc, None, Some(&mut staging))
    }
    .map_err(capture_error)?;
    let staging = staging.ok_or(Error::Capture(CaptureError::Unavailable))?;

    let source: ID3D11Resource = texture.cast().map_err(capture_error)?;
    let target: ID3D11Resource = staging.cast().map_err(capture_error)?;
    // SAFETY: both resources are live, same size and format, and the immediate
    // context is the device that created them.
    unsafe { device.context.CopyResource(&target, &source) };

    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    // SAFETY: `target` is a live staging texture with CPU read access and
    // subresource 0 exists.
    unsafe {
        device
            .context
            .Map(&target, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
    }
    .map_err(capture_error)?;
    // SAFETY: `mapped.pData` points at `RowPitch * height` readable bytes of
    // the mapped staging texture until `Unmap` below.
    let pixels = unsafe { read_mapped(&mapped, width, height) };
    // SAFETY: `target` is still mapped from the call above.
    unsafe { device.context.Unmap(&target, 0) };

    Ok(Captured {
        width: width as i32,
        height: height as i32,
        pixels,
    })
}

/// Reads a mapped premultiplied BGRA8 surface into straight RGBA, row by row,
/// honouring the row pitch.
///
/// # Safety
///
/// `mapped.pData` must point at `mapped.RowPitch * height` readable bytes.
unsafe fn read_mapped(mapped: &D3D11_MAPPED_SUBRESOURCE, width: usize, height: usize) -> Vec<u8> {
    let pitch = mapped.RowPitch as usize;
    let row_len = width * 4;
    // SAFETY: the caller guarantees enough readable bytes for a full last row
    // (the pitch is at least the row length for a texture).
    let source = unsafe { core::slice::from_raw_parts(mapped.pData as *const u8, pitch * height) };
    let mut pixels = Vec::with_capacity(row_len * height);
    for y in 0..height {
        let row = &source[y * pitch..y * pitch + row_len];
        for pixel in row.as_chunks::<4>().0 {
            // Staging BGRA8 is premultiplied; expose straight RGBA so the image
            // composites correctly on any background.
            let alpha = pixel[3];
            pixels.extend_from_slice(&[
                un_premultiply(pixel[2], alpha),
                un_premultiply(pixel[1], alpha),
                un_premultiply(pixel[0], alpha),
                alpha,
            ]);
        }
    }
    pixels
}

/// Divides a premultiplied channel by its alpha, rounding, so the result is
/// straight colour. A fully transparent pixel is black.
fn un_premultiply(channel: u8, alpha: u8) -> u8 {
    match alpha {
        0 => 0,
        0xFF => channel,
        _ => ((channel as u32 * 255 + alpha as u32 / 2) / alpha as u32).min(255) as u8,
    }
}

/// Maps a `windows` error from the capture path to the capture error it
/// describes: device loss, an absent capture stack, or a generic `Win32`
/// failure.
fn capture_error(error: windows::core::Error) -> Error {
    let code = error.code();
    if code == DXGI_ERROR_DEVICE_REMOVED || code == DXGI_ERROR_DEVICE_RESET {
        Error::Capture(CaptureError::DeviceLost)
    } else if code == E_NOINTERFACE
        || code == REGDB_E_CLASSNOTREG
        || code == RO_E_METADATA_NAME_NOT_FOUND
    {
        Error::Capture(CaptureError::Unavailable)
    } else {
        Error::from(Win32Error::new(code.0, error.message()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_mapped_converts_bgra_to_rgba_and_skips_padding() {
        // Two rows, pitch 12 (8 used + 4 padding). Pixels are B,G,R,A, opaque.
        let pitch = 12usize;
        let mut bytes = vec![0u8; pitch * 2];
        // Row 0: pixel (0,0) = B=1,G=2,R=3,A=255; pixel (1,0) = B=5,G=6,R=7.
        bytes[0..8].copy_from_slice(&[1, 2, 3, 255, 5, 6, 7, 255]);
        // Row 1: pixel (0,1) = B=9,G=10,R=11; pixel (1,1) = B=13,G=14,R=15.
        bytes[pitch..pitch + 8].copy_from_slice(&[9, 10, 11, 255, 13, 14, 15, 255]);

        let mapped = D3D11_MAPPED_SUBRESOURCE {
            pData: bytes.as_mut_ptr() as *mut _,
            RowPitch: pitch as u32,
            DepthPitch: (pitch * 2) as u32,
        };
        // SAFETY: `bytes` holds `pitch * 2` readable bytes.
        let rgba = unsafe { read_mapped(&mapped, 2, 2) };
        assert_eq!(
            rgba,
            vec![3, 2, 1, 255, 7, 6, 5, 255, 11, 10, 9, 255, 15, 14, 13, 255]
        );
    }

    #[test]
    fn read_mapped_un_premultiplies_translucent_pixels() {
        // A premultiplied pixel: B=16, G=32, R=64, A=128 -> straight RGBA.
        let pitch = 8usize;
        let mut bytes = vec![16u8, 32, 64, 128, 0, 0, 0, 0];
        let mapped = D3D11_MAPPED_SUBRESOURCE {
            pData: bytes.as_mut_ptr() as *mut _,
            RowPitch: pitch as u32,
            DepthPitch: pitch as u32,
        };
        // SAFETY: `bytes` holds `pitch` readable bytes for the one row.
        let rgba = unsafe { read_mapped(&mapped, 2, 1) };
        assert_eq!(
            rgba,
            vec![128, 64, 32, 128, 0, 0, 0, 0],
            "premultiplied colour must be divided by its alpha; a fully \
             transparent pixel is black"
        );
    }

    #[test]
    fn un_premultiply_is_identity_at_the_extremes() {
        assert_eq!(un_premultiply(200, 0), 0);
        assert_eq!(un_premultiply(200, 255), 200);
        assert_eq!(un_premultiply(128, 128), 255);
    }
}
