#![forbid(unsafe_code)]

//! Crate-wide error type. Library crates use `thiserror`, per AGENTS.md.

/// A failed Win32 call, kept as an owned `(HRESULT, message)` pair so the
/// public API never names the `windows` crate's error type.
#[derive(Clone, Debug, thiserror::Error)]
#[error("Win32 error 0x{code:08X}: {message}")]
pub struct Win32Error {
    code: i32,
    message: String,
}

impl Win32Error {
    /// Builds an error from a raw `HRESULT` code and the system message that
    /// describes it. `sys` performs the conversion from `windows::core::Error`.
    pub(crate) fn new(code: i32, message: impl Into<String>) -> Win32Error {
        Win32Error {
            code,
            message: message.into(),
        }
    }

    /// The raw `HRESULT` code.
    pub fn code(&self) -> i32 {
        self.code
    }

    /// The system message describing the code.
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Things that can go wrong while talking to Win32.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A Win32 call returned an error status.
    #[error("Win32 call failed: {0}")]
    Win32(#[from] Win32Error),

    /// `RegisterClassExW` failed; the window name is included for context.
    #[error("window class `{name}` could not be registered")]
    ClassRegistration { name: String },

    /// `CreateWindowExW` failed.
    #[error("could not create a window of class `{class}`")]
    CreateWindow {
        class: String,
        #[source]
        source: Win32Error,
    },

    /// A common control could not be created.
    #[error("could not create the {0} control")]
    CreateControl(&'static str),

    /// A GDI object could not be created.
    #[error("could not create GDI object: {0}")]
    Gdi(&'static str),

    /// A window icon could not be created.
    #[error("could not create window icon: {0}")]
    Icon(&'static str),

    /// An operation requires a window that has already been destroyed.
    #[error("the window has already been destroyed")]
    WindowDestroyed,

    /// A control was used before common controls were initialised.
    #[error("common controls were not initialised")]
    ControlsUnavailable,

    /// Task dialogs need Common Controls v6, which requires an application
    /// manifest requesting it; the loaded `comctl32.dll` does not export
    /// `TaskDialogIndirect`.
    #[error("task dialogs require Common Controls v6 (add the v6 application manifest)")]
    TaskDialogUnavailable,

    /// A task dialog was configured in a way that cannot be shown.
    #[error("invalid task dialog: {0}")]
    TaskDialog(String),

    /// A Direct2D surface was misused (for example, drawn to re-entrantly).
    #[error("invalid Direct2D use: {0}")]
    Direct2d(&'static str),

    /// An image could not be decoded, scaled or encoded ([`crate::imaging`]).
    #[error(transparent)]
    Imaging(#[from] ImagingError),

    /// An OpenGL (WGL) context could not be created on a window, so its widget
    /// falls back to GDI.
    #[error("could not create an OpenGL context: {0}")]
    Gl(&'static str),

    /// The occlusion-proof `Windows.Graphics.Capture` path failed for a
    /// reason callers may want to distinguish. Other Win32 failures keep
    /// their [`Win32`](Error::Win32) variant.
    #[error(transparent)]
    Capture(#[from] CaptureError),

    /// A feature was used on a window that does not support it.
    #[error("unsupported window configuration: {0}")]
    WindowConfig(&'static str),
}

/// Why an occlusion-proof (Windows.Graphics.Capture) capture failed.
///
/// A caller that only needs to know *whether* a capture worked can ignore
/// this; the cases are split out because they suggest different responses:
/// a minimised window must be restored by the app before capturing, a timeout
/// may be retried, and an unavailable backend means falling back to
/// [`Window::capture`](crate::Window::capture).
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CaptureError {
    /// The window is minimised, so it has no composited surface to capture.
    #[error("the window is minimised")]
    Minimized,
    /// The window had no content to capture (a zero-sized surface), for example
    /// one that has not been laid out yet.
    #[error("the window has no content to capture")]
    EmptyWindow,
    /// No frame arrived within the capture timeout.
    #[error("no capture frame arrived before the timeout")]
    Timeout,
    /// Windows.Graphics.Capture is not available on this system.
    #[error("Windows.Graphics.Capture is not available")]
    Unavailable,
    /// The D3D11 device was removed while the frame was copied back.
    #[error("the capture device was lost")]
    DeviceLost,
}

/// Why a Windows Imaging Component operation failed.
///
/// Most failures are a corrupt or unsupported image, which a caller usually
/// treats as "no image" and renders a placeholder for; the wrapped
/// [`Win32Error`] is kept for diagnosis.
#[derive(Clone, Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ImagingError {
    /// The bytes are not an image in a format the Windows Imaging Component
    /// understands.
    #[error("unsupported or corrupt image data")]
    UnsupportedFormat,
    /// A width or height of zero was requested, or an image exceeds the
    /// supported dimensions.
    #[error("invalid image dimensions")]
    InvalidSize,
    /// A call into the Windows Imaging Component failed.
    #[error("imaging call failed: {0}")]
    Win32(#[from] Win32Error),
}

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// The error types a frontend usually needs.
pub mod prelude {
    pub use super::{CaptureError, Error, ImagingError, Result, Win32Error};
}
