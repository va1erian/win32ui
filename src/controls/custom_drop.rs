#![forbid(unsafe_code)]

//! The drop-target side of [`Custom`](super::custom::Custom): forwards OLE
//! drag callbacks to [`CustomWidget::drag`](super::custom::CustomWidget::drag)
//! and scrolls the widget's scroll host near its edges.

use std::cell::Cell;
use std::rc::{Rc, Weak};

use crate::controls::custom::{CustomWidget, WidgetCx};
use crate::controls::custom_inner::CustomShared;
use crate::dnd::{DragEvent, DragInfo};
use crate::geometry::Rect;
use crate::hwnd::Hwnd;
use crate::sys::dnd::{DataObj, DragPoint, TargetSink};
use crate::units::Dip;

/// How close to the top or bottom edge a drag starts auto-scrolling, in design
/// units.
const EDGE_ZONE_DIP: f32 = 24.0;
/// How far one [`DragEvent::Over`] tick scrolls, in design units. OLE ticks
/// about every 50 ms, so this is roughly 320 dip per second.
const SCROLL_STEP_DIP: f32 = 16.0;

/// The [`TargetSink`] of one custom widget. It holds the widget weakly, so the
/// registration OLE keeps alive never keeps the widget alive.
pub(crate) struct CustomDropSink<W: CustomWidget, M> {
    pub(crate) shared: Weak<CustomShared<W, M>>,
    pub(crate) bounds: Rc<Cell<Rect>>,
    pub(crate) emit: Rc<dyn Fn(W::Event)>,
    pub(crate) animate: Rc<Cell<bool>>,
    pub(crate) hwnd: Cell<Hwnd>,
}

impl<W: CustomWidget, M: 'static> CustomDropSink<W, M> {
    /// Hands `event` to the widget and returns the raw effect bits it chose.
    /// A widget that is gone, or borrowed by the app right now, rejects.
    fn deliver(&self, event: DragEvent<'_>) -> u32 {
        let Some(shared) = self.shared.upgrade() else {
            return 0;
        };
        let mut cx = WidgetCx::new(
            self.hwnd.get(),
            Rc::clone(&self.bounds),
            Rc::clone(&self.emit),
            shared.ui.dpi(),
            Rc::clone(&self.animate),
        );
        let Ok(widget) = shared.widget.try_borrow() else {
            return 0;
        };
        widget.drag(event, &mut cx).bits()
    }

    /// Scrolls the scroll host when the pointer is in an edge zone.
    fn auto_scroll(&self, shared: &CustomShared<W, M>, y: i32) {
        let scroll = shared.scroll.borrow();
        let Some(scroll) = scroll.as_ref() else {
            return;
        };
        let dpi = shared.ui.dpi();
        let zone = Dip::new(EDGE_ZONE_DIP).to_px(dpi).value();
        let step = Dip::new(SCROLL_STEP_DIP).to_px(dpi).value();
        let height = self.bounds.get().height();
        if y < zone {
            scroll.scroll_by_px(-step);
        } else if y > height - zone {
            scroll.scroll_by_px(step);
        }
    }
}

impl<W: CustomWidget, M: 'static> TargetSink for CustomDropSink<W, M> {
    fn enter(&self, point: DragPoint, data: &DataObj) -> u32 {
        self.deliver(DragEvent::Enter(DragInfo::new(point, data)))
    }

    fn over(&self, point: DragPoint, data: &DataObj) -> u32 {
        if let Some(shared) = self.shared.upgrade() {
            self.auto_scroll(&shared, point.y);
        }
        self.deliver(DragEvent::Over(DragInfo::new(point, data)))
    }

    fn leave(&self) {
        self.deliver(DragEvent::Leave);
    }

    fn dropped(&self, point: DragPoint, data: &DataObj) -> u32 {
        self.deliver(DragEvent::Drop(DragInfo::new(point, data)))
    }
}
