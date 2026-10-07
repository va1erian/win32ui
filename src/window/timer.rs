#![forbid(unsafe_code)]

//! `WM_TIMER` timers on a [`Window`], delivered as
//! [`Message::Timer`](crate::Message::Timer).

use crate::error::Result;
use crate::message::TimerId;
use crate::sys;
use crate::window::Window;

impl Window {
    /// Starts a repeating timer.
    pub fn set_timer(&self, millis: u32) -> Result<TimerId> {
        sys::timer::set_timer(self.hwnd, millis).map(TimerId)
    }

    /// Starts a repeating timer that Windows may fire up to `tolerance_ms`
    /// late so it can batch the wake-up with other timers
    /// (`SetCoalescableTimer`), saving power for periodic work that does not
    /// need exact timing, such as refreshing a dashboard. It arrives as the
    /// same [`Message::Timer`](crate::Message::Timer) as
    /// [`Window::set_timer`] and is stopped with [`Window::kill_timer`].
    ///
    /// `tolerance_ms` of `0` uses the system's default tolerance and
    /// `u32::MAX` disables coalescing; any other value above `0x7FFFFFF5`
    /// (`TIMERV_COALESCING_MAX`) is rejected with an error.
    pub fn set_coalescable_timer(&self, millis: u32, tolerance_ms: u32) -> Result<TimerId> {
        sys::timer::set_coalescable_timer(self.hwnd, millis, tolerance_ms).map(TimerId)
    }

    /// Stops a timer started by [`Window::set_timer`] or
    /// [`Window::set_coalescable_timer`].
    pub fn kill_timer(&self, id: TimerId) {
        sys::timer::kill_timer(self.hwnd, id.0);
    }
}
