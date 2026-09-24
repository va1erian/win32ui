//! Occlusion-proof window capture through Windows.Graphics.Capture.
//!
//! This is the `wgc` backend behind [`crate::sys::capture`]: it grabs the
//! DWM-composited surface of an `HWND` (caption buttons, frame, Mica/acrylic
//! backdrop and all) even when the window is occluded, without raising it,
//! moving the pointer or taking focus.

use core::time::Duration;
use std::thread::sleep;
use std::time::Instant;

use windows::Graphics::Capture::{
    Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_11_0,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
    D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Resource, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::{
    DXGI_ERROR_DEVICE_REMOVED, DXGI_ERROR_DEVICE_RESET, IDXGIDevice,
};
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::UI::WindowsAndMessaging::IsIconic;
use windows::core::{Interface, factory};

use crate::error::{CaptureError, Error, Result, Win32Error};
use crate::hwnd::Hwnd;

use super::capture::Captured;
use super::{raw_hwnd, win32_error};

/// How long to wait for the first frame before giving up. The first frame of a
/// freshly created capture session usually arrives in tens of milliseconds.
const FIRST_FRAME_TIMEOUT: Duration = Duration::from_millis(1000);
/// How often the free-threaded frame pool is polled while waiting.
const FRAME_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Captures the composited surface of `hwnd` into a top-down RGBA buffer.
///
/// Works on any top-level window, including one owned by another process, and
/// never touches focus or the pointer. See [`crate::sys::capture`] for the
/// `PrintWindow` and screen-`BitBlt` alternatives.
pub(crate) fn capture(hwnd: Hwnd) -> Result<Captured> {
    if hwnd.is_null() {
        return Err(Error::WindowDestroyed);
    }
    // A minimised window has no composited surface; DWM does not draw it at
    // all. Do not restore it on the caller's behalf.
    // SAFETY: `hwnd` is a caller-provided handle; `IsIconic` only reads it.
    if unsafe { IsIconic(raw_hwnd(hwnd)) }.as_bool() {
        return Err(Error::Capture(CaptureError::Minimized));
    }

    let item = capture_item(hwnd)?;
    let size = item.Size().map_err(win32_error)?;
    if size.Width <= 0 || size.Height <= 0 {
        return Err(Error::Capture(CaptureError::Unavailable));
    }

    let device = create_capture_device()?;
    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
        &device.device,
        DirectXPixelFormat::B8G8R8A8UIntNormalized,
        1,
        size,
    )
    .map_err(win32_error)?;
    let session = pool.CreateCaptureSession(&item).map_err(win32_error)?;
    // Both are Windows 11 properties; on older builds the casts fail and the
    // defaults (cursor off for a window capture, border on) stand. Best effort.
    if let Ok(()) = session.SetIsBorderRequired(false) {}
    if let Ok(()) = session.SetIsCursorCaptureEnabled(false) {}
    session.StartCapture().map_err(win32_error)?;

    let frame = wait_for_frame(&pool)?;
    let captured = copy_frame(&device, &frame);
    // Releases the WinRT resources as soon as the pixels are copied; COM
    // references would also be released on drop, but `Close` is deterministic.
    let _ = frame.Close();
    let _ = pool.Close();
    let _ = session.Close();
    captured
}

/// A fresh `GraphicsCaptureItem` for `hwnd`, via the documented interop
/// factory. Works across processes.
fn capture_item(hwnd: Hwnd) -> Result<GraphicsCaptureItem> {
    let interop: IGraphicsCaptureItemInterop =
        factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().map_err(win32_error)?;
    // SAFETY: `hwnd` is a live window handle supplied by the caller; the
    // interop call takes it by value and does not retain it.
    unsafe { interop.CreateForWindow::<GraphicsCaptureItem>(raw_hwnd(hwnd)) }.map_err(win32_error)
}

/// The D3D11 device and its WinRT projection, created once per capture.
struct CaptureDevice {
    device: IDirect3DDevice,
    d3d: ID3D11Device,
    context: ID3D11DeviceContext,
}

/// Creates a hardware D3D11 device and wraps it for Windows.Graphics.Capture.
fn create_capture_device() -> Result<CaptureDevice> {
    let mut d3d: Option<ID3D11Device> = None;
    let mut context: Option<ID3D11DeviceContext> = None;
    let mut level = D3D_FEATURE_LEVEL(0);
    // SAFETY: all out-pointers are valid locals; a null adapter asks for the
    // default hardware adapter; no borrowed data outlives the call.
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut d3d),
            Some(&mut level),
            Some(&mut context),
        )
    }
    .map_err(win32_error)?;
    let missing = || Error::Capture(CaptureError::Unavailable);
    let (d3d, context) = (d3d.ok_or_else(missing)?, context.ok_or_else(missing)?);

    let dxgi: IDXGIDevice = d3d.cast().map_err(win32_error)?;
    // SAFETY: `dxgi` is a live DXGI device; the call returns a new WinRT
    // device that owns its own reference.
    let inspectable =
        unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }.map_err(device_error)?;
    let device: IDirect3DDevice = inspectable.cast().map_err(device_error)?;

    Ok(CaptureDevice {
        device,
        d3d,
        context,
    })
}

/// Polls the free-threaded pool until a frame arrives or the timeout elapses.
///
/// A free-threaded pool delivers frames on its own thread, so this never needs
/// to pump messages and cannot re-enter the caller's message loop.
fn wait_for_frame(pool: &Direct3D11CaptureFramePool) -> Result<Direct3D11CaptureFrame> {
    let deadline = Instant::now() + FIRST_FRAME_TIMEOUT;
    loop {
        match pool.TryGetNextFrame() {
            Ok(frame) => return Ok(frame),
            Err(_) if Instant::now() < deadline => sleep(FRAME_POLL_INTERVAL),
            Err(_) => return Err(Error::Capture(CaptureError::Timeout)),
        }
    }
}

/// Copies `frame`'s texture into a CPU-readable staging texture and converts
/// its BGRA8 pixels to tightly packed RGBA.
fn copy_frame(device: &CaptureDevice, frame: &Direct3D11CaptureFrame) -> Result<Captured> {
    let surface = frame.Surface().map_err(device_error)?;
    let access: IDirect3DDxgiInterfaceAccess = surface.cast().map_err(device_error)?;
    // SAFETY: the frame's surface backs an `ID3D11Texture2D`; the interop call
    // returns a new reference to it.
    let texture: ID3D11Texture2D = unsafe { access.GetInterface() }.map_err(device_error)?;

    let mut desc = D3D11_TEXTURE2D_DESC::default();
    // SAFETY: `desc` is a valid out-pointer for a live texture.
    unsafe { texture.GetDesc(&mut desc) };
    let (width, height) = (desc.Width as usize, desc.Height as usize);
    if width == 0 || height == 0 {
        return Err(Error::Capture(CaptureError::Unavailable));
    }

    desc.Usage = D3D11_USAGE_STAGING;
    desc.BindFlags = 0;
    desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
    desc.MiscFlags = 0;

    let mut staging: Option<ID3D11Texture2D> = None;
    // SAFETY: `desc` is initialised, no initial data is passed, and `staging`
    // is a valid out-pointer.
    unsafe { device.d3d.CreateTexture2D(&desc, None, Some(&mut staging)) }.map_err(device_error)?;
    let staging = staging.ok_or(Error::Capture(CaptureError::Unavailable))?;

    let source: ID3D11Resource = texture.cast().map_err(device_error)?;
    let target: ID3D11Resource = staging.cast().map_err(device_error)?;
    // SAFETY: both resources are live, same size and format, and the
    // immediate context is the device that created them.
    unsafe { device.context.CopyResource(&target, &source) };

    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    // SAFETY: `target` is a live staging texture with CPU read access and
    // subresource 0 exists.
    unsafe {
        device
            .context
            .Map(&target, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
    }
    .map_err(device_error)?;
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

/// Reads a mapped BGRA8 surface into tightly packed RGBA, row by row,
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
            // Staging BGRA8 -> straight RGBA; the alpha the compositor produced
            // is kept so rounded corners and the backdrop stay translucent.
            pixels.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
    }
    pixels
}

/// Maps a `windows` error to a device-loss capture error when the HRESULT says
/// the D3D11 device was removed or reset, and to [`Error::Win32`] otherwise.
fn device_error(error: windows::core::Error) -> Error {
    let code = error.code();
    if code == DXGI_ERROR_DEVICE_REMOVED || code == DXGI_ERROR_DEVICE_RESET {
        Error::Capture(CaptureError::DeviceLost)
    } else {
        Error::from(Win32Error::new(code.0, error.message()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_mapped_converts_bgra_to_rgba_and_skips_padding() {
        // Two rows, pitch 12 (8 used + 4 padding). Pixels are B,G,R,A.
        let pitch = 12usize;
        let mut bytes = vec![0u8; pitch * 2];
        // Row 0: pixel (0,0) = B=1,G=2,R=3,A=4; pixel (1,0) = B=5,G=6,R=7,A=8.
        bytes[0..8].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        // Row 1: pixel (0,1) = B=9,G=10,R=11,A=12; pixel (1,1) = B=13..A=16.
        bytes[pitch..pitch + 8].copy_from_slice(&[9, 10, 11, 12, 13, 14, 15, 16]);

        let mapped = D3D11_MAPPED_SUBRESOURCE {
            pData: bytes.as_mut_ptr() as *mut _,
            RowPitch: pitch as u32,
            DepthPitch: (pitch * 2) as u32,
        };
        // SAFETY: `bytes` holds `pitch * 2` readable bytes.
        let rgba = unsafe { read_mapped(&mapped, 2, 2) };
        assert_eq!(
            rgba,
            vec![3, 2, 1, 4, 7, 6, 5, 8, 11, 10, 9, 12, 15, 14, 13, 16]
        );
    }

    #[test]
    fn read_mapped_keeps_alpha() {
        let pitch = 4usize;
        let mut bytes = vec![10u8, 20, 30, 0];
        let mapped = D3D11_MAPPED_SUBRESOURCE {
            pData: bytes.as_mut_ptr() as *mut _,
            RowPitch: pitch as u32,
            DepthPitch: pitch as u32,
        };
        // SAFETY: `bytes` holds `pitch` readable bytes.
        let rgba = unsafe { read_mapped(&mapped, 1, 1) };
        assert_eq!(rgba, vec![30, 20, 10, 0], "alpha must survive conversion");
    }
}
