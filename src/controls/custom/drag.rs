#![forbid(unsafe_code)]

//! [`Custom`] as a drag source and drop target.

use std::rc::Rc;

use super::{Custom, CustomWidget};
use crate::dnd::{DragImage, DropEffect, DropEffects};
use crate::error::Result;
use crate::sys;

impl<W: CustomWidget, M: 'static> Custom<W, M> {
    /// Makes the widget a drop target: drags over it are delivered to
    /// [`CustomWidget::drag`], including Explorer file drops. Opt-in, so a
    /// widget that never calls this is invisible to drag and drop. Fails when
    /// OLE cannot be initialised on this thread (see [`crate::dnd`]).
    pub fn accept_drops(self) -> Result<Custom<W, M>> {
        let sink: Rc<dyn sys::dnd::TargetSink> = self.drop_sink.clone();
        let registration = sys::dnd::register_target(self.control.hwnd(), sink)?;
        self.drop_target.replace(Some(registration));
        Ok(self)
    }

    /// Starts a drag from this widget carrying `payload`; see
    /// [`begin_drag`](crate::dnd::begin_drag). Call it while the mouse button
    /// is down, for instance from the `Msg` an [`Input::MouseMove`](crate::Input::MouseMove) raised.
    pub fn begin_drag(
        &self,
        payload: &[u8],
        allowed: DropEffects,
        image: Option<&DragImage>,
    ) -> Result<DropEffect> {
        crate::dnd::begin_drag(self.control.hwnd(), payload, allowed, image)
    }
}
