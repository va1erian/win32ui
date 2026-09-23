#![forbid(unsafe_code)]

//! A safe, `Copy` handle to a window. Stored as a `usize` so the public API
//! never names the `windows` crate's pointer newtype.

/// A window handle (`HWND`), or the null handle.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Hwnd(usize);

impl Hwnd {
    /// The null window handle.
    pub const NULL: Hwnd = Hwnd(0);

    /// Wraps a raw handle value. Only meaningful for values that came from
    /// Win32; provided for interop with code that already has an `HWND`.
    pub const fn from_raw(value: usize) -> Hwnd {
        Hwnd(value)
    }

    /// The raw handle value.
    pub const fn raw(self) -> usize {
        self.0
    }

    /// Whether this is the null handle.
    pub const fn is_null(self) -> bool {
        self.0 == 0
    }

    /// Whether the handle names a live window.
    pub fn is_alive(self) -> bool {
        !self.is_null() && crate::sys::window::is_window(self)
    }
}

impl std::fmt::Debug for Hwnd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Hwnd(0x{:x})", self.0)
    }
}

/// The handle type a frontend usually needs.
pub mod prelude {
    pub use super::Hwnd;
}
