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
}

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;
