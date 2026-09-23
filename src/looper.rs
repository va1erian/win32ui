#![forbid(unsafe_code)]

//! The UI thread's message loop.

use crate::sys;

/// Runs the message loop until [`quit`] is called, returning its exit code.
///
/// This blocks the calling thread; it must be the thread that created the
/// windows being serviced.
pub fn run() -> i32 {
    loop {
        match sys::message::pump() {
            sys::message::Pumped::Message => {}
            sys::message::Pumped::Quit(code) => return code,
            sys::message::Pumped::Error => return -1,
        }
    }
}

/// Asks [`run`] to return with `code` on the calling thread.
pub fn quit(code: i32) {
    sys::message::post_quit(code);
}
