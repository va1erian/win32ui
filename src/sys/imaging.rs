//! Windows Imaging Component (WIC) calls behind [`crate::imaging`].
//!
//! Decoding uses the system codecs (JPEG, PNG, BMP, GIF, TIFF, ...) instead of
//! a bundled decoder, scaling uses WIC's resampler, and JPEG encoding backs the
//! thumbnail cache. Every COM pointer lives only for the duration of one call;
//! the caller gets plain RGBA bytes back.

use core::ffi::c_void;

use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatJpeg, GUID_WICPixelFormat24bppBGR,
    GUID_WICPixelFormat32bppRGBA, IWICBitmapFrameEncode, IWICBitmapSource, IWICFormatConverter,
    IWICImagingFactory, IWICPalette, WICBitmapDitherTypeNone, WICBitmapEncoderNoCache,
    WICBitmapInterpolationModeFant, WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnLoad,
};
use windows::Win32::System::Com::StructuredStorage::IPropertyBag2;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
    IStream, STATFLAG_NONAME, STATSTG, STREAM_SEEK_SET,
};
use windows::Win32::UI::Shell::SHCreateMemStream;
use windows::core::{GUID, Interface};

use crate::capture::RgbaImage;
use crate::error::{Error, ImagingError, Result};

/// Maps a failed `windows` call into the crate's imaging error.
fn imaging(error: windows::core::Error) -> Error {
    Error::Imaging(ImagingError::Win32(super::win32(error)))
}

/// Owns a COM apartment for the duration of one imaging call.
///
/// WIC's factory is free-threaded, but COM must still be initialised on the
/// calling thread. Worker threads that decode images have usually never
/// initialised COM, so each call enters (and, if it was the one to initialise,
/// leaves) a multithreaded apartment.
struct Apartment {
    own: bool,
}

impl Apartment {
    fn enter() -> Apartment {
        // SAFETY: `CoInitializeEx` takes an optional reserved pointer and only
        // affects the current thread.
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        // `RPC_E_CHANGED_MODE` means the thread is already in another apartment
        // model; WIC works in either, so the call can continue. Any other
        // failure is surfaced by the `CoCreateInstance` that follows.
        Apartment { own: hr.is_ok() }
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.own {
            // SAFETY: balances the successful `CoInitializeEx` in `enter`.
            unsafe { CoUninitialize() };
        }
    }
}

/// Creates the WIC imaging factory.
fn factory() -> Result<IWICImagingFactory> {
    // SAFETY: `CLSID_WICImagingFactory` names an in-process COM class; the
    // returned interface is released when dropped.
    unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }
        .map_err(imaging)
}

/// Decodes `bytes` (any WIC-supported container) into a straight-RGBA image.
pub(crate) fn decode(bytes: &[u8]) -> Result<RgbaImage> {
    if bytes.is_empty() {
        return Err(Error::Imaging(ImagingError::UnsupportedFormat));
    }
    let _apartment = Apartment::enter();
    let factory = factory()?;
    // SAFETY: the factory is live; `CreateStream` allocates an empty stream.
    let stream = unsafe { factory.CreateStream() }.map_err(imaging)?;
    // SAFETY: `bytes` outlives every use of the stream below.
    unsafe { stream.InitializeFromMemory(bytes) }.map_err(imaging)?;
    // A decoder failure almost always means unsupported/corrupt data, which
    // callers treat as "no image"; map it to `UnsupportedFormat`.
    let decoder = unsafe {
        factory.CreateDecoderFromStream(&stream, core::ptr::null(), WICDecodeMetadataCacheOnLoad)
    }
    .map_err(|_| Error::Imaging(ImagingError::UnsupportedFormat))?;
    // SAFETY: the decoder is live and has at least one frame to return.
    let frame = unsafe { decoder.GetFrame(0) }.map_err(imaging)?;
    let source: IWICBitmapSource = frame.cast().map_err(imaging)?;
    let converted = convert(&factory, &source, &GUID_WICPixelFormat32bppRGBA)?;
    copy_rgba(&converted)
}

/// Stretches `image` to `width`-by-`height` with WIC's resampler. Aspect ratio
/// is not preserved; callers that want it compute the target size themselves.
pub(crate) fn resize(image: &RgbaImage, width: u32, height: u32) -> Result<RgbaImage> {
    if width == 0 || height == 0 {
        return Err(Error::Imaging(ImagingError::InvalidSize));
    }
    if image.width == width && image.height == height {
        return Ok(image.clone());
    }
    let _apartment = Apartment::enter();
    let factory = factory()?;
    let source = bitmap_from_rgba(&factory, image)?;
    let source: IWICBitmapSource = source.cast().map_err(imaging)?;
    // SAFETY: the factory is live; `CreateBitmapScaler` allocates a scaler.
    let scaler = unsafe { factory.CreateBitmapScaler() }.map_err(imaging)?;
    // SAFETY: `source` is a live bitmap source.
    unsafe { scaler.Initialize(&source, width, height, WICBitmapInterpolationModeFant) }
        .map_err(imaging)?;
    let scaled: IWICBitmapSource = scaler.cast().map_err(imaging)?;
    let converted = convert(&factory, &scaled, &GUID_WICPixelFormat32bppRGBA)?;
    copy_rgba(&converted)
}

/// Encodes `image` as JPEG bytes (WIC's default quality) for the thumbnail
/// cache. The alpha channel is dropped.
pub(crate) fn encode_jpeg(image: &RgbaImage) -> Result<Vec<u8>> {
    if image.width == 0 || image.height == 0 {
        return Err(Error::Imaging(ImagingError::InvalidSize));
    }
    let _apartment = Apartment::enter();
    let factory = factory()?;
    // SAFETY: a null initial buffer creates an empty, writable memory stream.
    let stream = unsafe { SHCreateMemStream(None) }.ok_or_else(|| {
        Error::Imaging(ImagingError::Win32(crate::error::Win32Error::new(
            0x8007_000E_u32 as i32,
            "could not allocate the image stream",
        )))
    })?;
    let encoder = unsafe { factory.CreateEncoder(&GUID_ContainerFormatJpeg, core::ptr::null()) }
        .map_err(imaging)?;
    unsafe { encoder.Initialize(&stream, WICBitmapEncoderNoCache) }.map_err(imaging)?;

    let mut frame: Option<IWICBitmapFrameEncode> = None;
    let mut options: Option<IPropertyBag2> = None;
    // SAFETY: both out-parameters are valid locals.
    unsafe { encoder.CreateNewFrame(&mut frame, &mut options) }.map_err(imaging)?;
    let frame = frame.ok_or(Error::Imaging(ImagingError::InvalidSize))?;
    unsafe { frame.Initialize(None::<&IPropertyBag2>) }.map_err(imaging)?;
    unsafe { frame.SetSize(image.width, image.height) }.map_err(imaging)?;
    let mut format = GUID_WICPixelFormat24bppBGR;
    unsafe { frame.SetPixelFormat(&mut format) }.map_err(imaging)?;
    if format != GUID_WICPixelFormat24bppBGR {
        return Err(Error::Imaging(ImagingError::UnsupportedFormat));
    }
    // The JPEG encoder takes BGR without alpha; WIC's `WriteSource` would need
    // a format converter, so reorder here in plain Rust.
    let bgr = to_bgr(image);
    let stride = image
        .width
        .checked_mul(3)
        .ok_or(Error::Imaging(ImagingError::InvalidSize))?;
    unsafe { frame.WritePixels(image.height, stride, &bgr) }.map_err(imaging)?;
    unsafe { frame.Commit() }.map_err(imaging)?;
    unsafe { encoder.Commit() }.map_err(imaging)?;
    read_stream(&stream)
}

/// Wraps tightly packed RGBA bytes as a WIC bitmap source.
fn bitmap_from_rgba(
    factory: &IWICImagingFactory,
    image: &RgbaImage,
) -> Result<windows::Win32::Graphics::Imaging::IWICBitmap> {
    let stride = image
        .width
        .checked_mul(4)
        .ok_or(Error::Imaging(ImagingError::InvalidSize))?;
    // SAFETY: `image.pixels` holds at least `stride * height` bytes, as
    // guaranteed by `RgbaImage`'s construction.
    unsafe {
        factory.CreateBitmapFromMemory(
            image.width,
            image.height,
            &GUID_WICPixelFormat32bppRGBA,
            stride,
            &image.pixels,
        )
    }
    .map_err(imaging)
}

/// Converts `source` to `format` with WIC's format converter.
fn convert(
    factory: &IWICImagingFactory,
    source: &IWICBitmapSource,
    format: &GUID,
) -> Result<IWICFormatConverter> {
    // SAFETY: the factory is live; `CreateFormatConverter` allocates one.
    let converter = unsafe { factory.CreateFormatConverter() }.map_err(imaging)?;
    // SAFETY: `source` is live; a `None` palette is correct for non-indexed
    // target formats such as RGBA.
    unsafe {
        converter.Initialize(
            source,
            format,
            WICBitmapDitherTypeNone,
            None::<&IWICPalette>,
            0.0,
            WICBitmapPaletteTypeCustom,
        )
    }
    .map_err(imaging)?;
    Ok(converter)
}

/// Reads a converted source's pixels into a tightly packed RGBA image.
fn copy_rgba(source: &IWICBitmapSource) -> Result<RgbaImage> {
    let mut width = 0u32;
    let mut height = 0u32;
    // SAFETY: both out-parameters are valid locals.
    unsafe { source.GetSize(&mut width, &mut height) }.map_err(imaging)?;
    if width == 0 || height == 0 {
        return Err(Error::Imaging(ImagingError::InvalidSize));
    }
    let stride = width
        .checked_mul(4)
        .ok_or(Error::Imaging(ImagingError::InvalidSize))?;
    let len = (stride as usize)
        .checked_mul(height as usize)
        .ok_or(Error::Imaging(ImagingError::InvalidSize))?;
    let mut pixels = vec![0u8; len];
    // SAFETY: `pixels` is exactly `stride * height` bytes; a null rect copies
    // the whole image.
    unsafe { source.CopyPixels(core::ptr::null(), stride, &mut pixels) }.map_err(imaging)?;
    Ok(RgbaImage {
        width,
        height,
        pixels,
    })
}

/// Reorders RGBA pixels into the BGR triplets the JPEG encoder takes.
fn to_bgr(image: &RgbaImage) -> Vec<u8> {
    let mut bgr = Vec::with_capacity(image.pixels.len() / 4 * 3);
    for pixel in image.pixels.as_chunks::<4>().0 {
        bgr.push(pixel[2]);
        bgr.push(pixel[1]);
        bgr.push(pixel[0]);
    }
    bgr
}

/// Copies the bytes an encoder wrote back out of its memory stream.
fn read_stream(stream: &IStream) -> Result<Vec<u8>> {
    let mut stat = STATSTG::default();
    // SAFETY: `stat` is a valid local; `STATFLAG_NONAME` skips the name.
    unsafe { stream.Stat(&mut stat, STATFLAG_NONAME) }.map_err(imaging)?;
    let size = stat.cbSize as usize;
    if size == 0 {
        return Err(Error::Imaging(ImagingError::InvalidSize));
    }
    // SAFETY: seeking to the start of the stream before reading it back.
    unsafe { stream.Seek(0, STREAM_SEEK_SET, None) }.map_err(imaging)?;
    let mut bytes = vec![0u8; size];
    let mut read = 0u32;
    // SAFETY: `bytes` is `size` bytes; `Read` writes at most that many.
    let hr = unsafe {
        stream.Read(
            bytes.as_mut_ptr() as *mut c_void,
            size as u32,
            Some(&mut read),
        )
    };
    hr.ok().map_err(imaging)?;
    bytes.truncate(read as usize);
    Ok(bytes)
}
