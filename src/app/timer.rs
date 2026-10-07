#![forbid(unsafe_code)]

//! `WM_TIMER` timers on the app window, mapped to messages by
//! [`Ui::on_timer`].

use crate::error::Result;
use crate::message::TimerId;
use crate::sys;

use super::ui::Ui;

impl<M: 'static> Ui<M> {
    /// Starts a repeating timer and returns its id.
    pub fn set_timer(&self, millis: u32) -> Result<TimerId> {
        sys::timer::set_timer(self.core.hwnd(), millis).map(TimerId)
    }

    /// Starts a repeating timer that Windows may fire up to `tolerance_ms`
    /// late so it can batch the wake-up with other timers
    /// (`SetCoalescableTimer`), saving power for periodic work that does not
    /// need exact timing, such as refreshing a dashboard. Its ticks reach
    /// [`Ui::on_timer`] like those of [`Ui::set_timer`], and it is stopped
    /// with [`Ui::kill_timer`].
    ///
    /// `tolerance_ms` of `0` uses the system's default tolerance and
    /// `u32::MAX` disables coalescing; any other value above `0x7FFFFFF5`
    /// (`TIMERV_COALESCING_MAX`) is rejected with an error.
    pub fn set_coalescable_timer(&self, millis: u32, tolerance_ms: u32) -> Result<TimerId> {
        sys::timer::set_coalescable_timer(self.core.hwnd(), millis, tolerance_ms).map(TimerId)
    }

    /// Stops a timer started by [`Ui::set_timer`] or
    /// [`Ui::set_coalescable_timer`].
    pub fn kill_timer(&self, id: TimerId) {
        sys::timer::kill_timer(self.core.hwnd(), id.0);
    }
}
