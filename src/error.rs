#![forbid(unsafe_code)]

//! Crate-wide error type. Library crates use `thiserror`, per AGENTS.md.

/// Things that can go wrong while talking to Win32.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A Win32 call returned an error status.
    #[error("Win32 call failed: {0}")]
    Win32(#[from] windows::core::Error),

    /// `RegisterClassExW` failed; the window name is included for context.
    #[error("window class `{name}` could not be registered")]
    ClassRegistration { name: String },

    /// `CreateWindowExW` failed.
    #[error("could not create a window of class `{class}`")]
    CreateWindow {
        class: String,
        #[source]
        source: windows::core::Error,
    },

    /// A common control could not be created.
    #[error("could not create the {0} control")]
    CreateControl(&'static str),

    /// A GDI object could not be created.
    #[error("could not create GDI object: {0}")]
    Gdi(&'static str),

    /// An operation requires a window that has already been destroyed.
    #[error("the window has already been destroyed")]
    WindowDestroyed,

    /// A control was used before common controls were initialised.
    #[error("common controls were not initialised")]
    ControlsUnavailable,
}

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;
