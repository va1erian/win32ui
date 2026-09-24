//! The D3D11 device behind the `Windows.Graphics.Capture` backend.
//!
//! Kept separate from [`super::capture_wgc`] so that the session, frame and
//! pixel-conversion code stays small. A hardware device is preferred; when no
//! usable adapter exists (a GPU-less VM, RDP, some CI runners) the WARP software
//! rasteriser is used instead, and only when that also fails does the capture
//! report [`CaptureError::Unavailable`].

use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL,
    D3D_FEATURE_LEVEL_11_0,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device,
    ID3D11DeviceContext, ID3D11Multithread,
};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::System::WinRT::Direct3D11::CreateDirect3D11DeviceFromDXGIDevice;
use windows::core::Interface;

use crate::error::{CaptureError, Error, Result};

use super::win32_error;

/// The D3D11 device and its WinRT projection, created once per capture.
pub(crate) struct CaptureDevice {
    /// The device as the capture frame pool sees it.
    pub(crate) device: IDirect3DDevice,
    /// The raw device, used to create the read-back staging texture.
    pub(crate) d3d: ID3D11Device,
    /// The immediate context that maps the staging texture.
    pub(crate) context: ID3D11DeviceContext,
}

/// Creates a hardware D3D11 device, or a WARP one when no hardware adapter is
/// available.
pub(crate) fn create() -> Result<CaptureDevice> {
    match create_with(D3D_DRIVER_TYPE_HARDWARE) {
        Ok(device) => Ok(device),
        // No usable hardware adapter: fall back to the software rasteriser
        // rather than failing the capture.
        Err(_) => {
            create_with(D3D_DRIVER_TYPE_WARP).map_err(|_| Error::Capture(CaptureError::Unavailable))
        }
    }
}

/// Creates and projects a D3D11 device for `driver`.
fn create_with(driver: D3D_DRIVER_TYPE) -> Result<CaptureDevice> {
    let mut d3d: Option<ID3D11Device> = None;
    let mut context: Option<ID3D11DeviceContext> = None;
    let mut level = D3D_FEATURE_LEVEL(0);
    // SAFETY: all out-pointers are valid locals; a null adapter asks for the
    // default adapter for `driver`; no borrowed data outlives the call.
    unsafe {
        D3D11CreateDevice(
            None,
            driver,
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

    // The free-threaded frame pool delivers frames on its own thread while the
    // caller maps the staging texture on this one, so the device must be
    // safe to use from both. The call is best effort: a device that does not
    // expose `ID3D11Multithread` is not shared across the two paths.
    if let Ok(multithread) = d3d.cast::<ID3D11Multithread>() {
        // SAFETY: `multithread` is a live interface on the device just created;
        // a boolean flag is its whole input.
        unsafe {
            let _ = multithread.SetMultithreadProtected(true);
        }
    }

    let dxgi: IDXGIDevice = d3d.cast().map_err(win32_error)?;
    // SAFETY: `dxgi` is a live DXGI device; the call returns a new WinRT device
    // that owns its own reference.
    let inspectable =
        unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }.map_err(win32_error)?;
    let device: IDirect3DDevice = inspectable.cast().map_err(win32_error)?;

    Ok(CaptureDevice {
        device,
        d3d,
        context,
    })
}
