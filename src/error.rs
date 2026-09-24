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

    /// A feature was used on a window that does not support it.
    #[error("unsupported window configuration: {0}")]
    WindowConfig(&'static str),
}

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// The error types a frontend usually needs.
pub mod prelude {
    pub use super::{Error, Result, Win32Error};
}
