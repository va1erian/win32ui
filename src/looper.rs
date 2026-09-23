#![forbid(unsafe_code)]

//! The UI thread's message loop.

use crate::hwnd::Hwnd;
use crate::sys;

/// Runs the message loop until [`quit`] is called, returning its exit code.
///
/// This blocks the calling thread; it must be the thread that created the
/// windows being serviced.
pub fn run() -> i32 {
    loop {
        match sys::looper::pump() {
            sys::looper::Pumped::Message => {}
            sys::looper::Pumped::Quit(code) => return code,
            sys::looper::Pumped::Error => return -1,
        }
    }
}

/// Runs a nested message loop until `window` is destroyed, returning its exit
/// code.
///
/// This is the loop behind [`Window::run_modal`](crate::Window::run_modal): it
/// keeps pumping messages (for every window on the thread) but stops as soon as
/// `window` disappears, leaving the outer [`run`] loop in place. A `WM_QUIT`
/// posted while it runs also ends it.
pub fn run_modal(window: Hwnd) -> i32 {
    loop {
        match sys::looper::pump() {
            sys::looper::Pumped::Message => {
                if !sys::window::is_window(window) {
                    return 0;
                }
            }
            sys::looper::Pumped::Quit(code) => return code,
            sys::looper::Pumped::Error => return -1,
        }
    }
}

/// Asks [`run`] to return with `code` on the calling thread.
pub fn quit(code: i32) {
    sys::looper::post_quit(code);
}

/// The message-loop entry points a frontend usually needs.
pub mod prelude {
    pub use super::{quit, run, run_modal};
}
