#![forbid(unsafe_code)]

//! OLE drag and drop: start a drag from any window, and receive drags on a
//! [`Custom`](crate::Custom) widget or a [`ListView`](crate::ListView).
//!
//! # Source
//!
//! [`begin_drag`] runs the modal `DoDragDrop` loop with an in-process payload:
//! an opaque, app-defined byte blob the receiving side reads back with
//! [`DragData::payload`]. A [`ListView`](crate::ListView) reports the gesture
//! as [`ListViewEvent::BeginDrag`](crate::ListViewEvent::BeginDrag) (map it with
//! [`on_begin_drag`](crate::ListView::on_begin_drag)); the app answers from
//! `update` with [`ListView::begin_drag`](crate::ListView::begin_drag).
//!
//! # Target
//!
//! A drop target is opt-in per widget
//! ([`Custom::accept_drops`](crate::Custom::accept_drops),
//! [`ListView::on_drop`](crate::ListView::on_drop)). It receives
//! [`DragEvent`]s with client coordinates, the [`DragData`], and answers each
//! with the [`DropEffect`] it would apply. Explorer file drops (`CF_HDROP`)
//! arrive through the same path, read with [`DragData::files`].
//!
//! Callbacks run inside the drag's modal loop. Messages they emit reach
//! [`App::update`](crate::App::update) after the drag ends.
//!
//! # OLE ownership
//!
//! win32ui initialises OLE (`OleInitialize`) lazily on the calling thread the
//! first time a drag source or drop target is created, and uninitialises it
//! when that thread exits. The count is shared with any initialisation the
//! application does itself.
//!
//! # Feedback
//!
//! The drag image and the effect cursor come from the shell's drag helpers.
//! [`begin_drag`] shows the source window's own image (a list view paints its
//! dragged rows, in the current theme and DPI) unless the app supplies a
//! [`DragImage`]. While the pointer is over a widget with a vertical scroll
//! host ([`Custom::with_vscroll`](crate::Custom::with_vscroll)) or a
//! [`ListView`](crate::ListView) near its top or bottom edge, the widget
//! scrolls by itself; OLE repeats [`DragEvent::Over`] about every 50 ms even
//! when the pointer rests, which a widget that scrolls its own way can use as
//! its timer.

mod effects;
mod hdrop;

pub use effects::{DropEffect, DropEffects};

use std::path::PathBuf;

use windows::Win32::System::SystemServices::{MK_CONTROL, MK_SHIFT};

use crate::capture::RgbaImage;
use crate::error::Result;
use crate::geometry::Point;
use crate::hwnd::Hwnd;
use crate::message::Modifiers;
use crate::sys;
use crate::sys::dnd::{DataObj, DragImageBits, DragPoint, Outcome};

/// `MK_ALT` from `oleidl.h`: Alt was held during the drag. The `windows` crate
/// does not declare it.
const MK_ALT: u32 = 0x0020;

/// The data being dragged: an app payload, and/or files dragged from Explorer.
///
/// Only valid inside the callback it was passed to.
pub struct DragData<'a> {
    object: &'a DataObj,
}

impl<'a> DragData<'a> {
    pub(crate) fn new(object: &'a DataObj) -> DragData<'a> {
        DragData { object }
    }

    /// Whether the drag carries an app payload started by [`begin_drag`].
    pub fn has_payload(&self) -> bool {
        self.object.has_payload()
    }

    /// The app payload, or `None` when the drag came from elsewhere.
    /// Allocates; call it on drop rather than on every [`DragEvent::Over`].
    pub fn payload(&self) -> Option<Vec<u8>> {
        self.object.payload()
    }

    /// Whether the drag carries a file list (`CF_HDROP`), as an Explorer drag
    /// does.
    pub fn has_files(&self) -> bool {
        self.object.has_hdrop()
    }

    /// The dragged file paths; empty when the drag carries no files.
    pub fn files(&self) -> Vec<PathBuf> {
        self.object
            .hdrop()
            .map(|block| hdrop::parse(&block))
            .unwrap_or_default()
    }
}

/// Where a drag is and what it carries, in one [`DragEvent`].
pub struct DragInfo<'a> {
    /// Pointer x in the target's client coordinates (device pixels).
    pub x: i32,
    /// Pointer y in the target's client coordinates (device pixels).
    pub y: i32,
    /// The modifier keys held.
    pub modifiers: Modifiers,
    /// The effects the source allows.
    pub allowed: DropEffects,
    /// What is being dragged.
    pub data: DragData<'a>,
}

impl<'a> DragInfo<'a> {
    pub(crate) fn new(point: DragPoint, object: &'a DataObj) -> DragInfo<'a> {
        DragInfo {
            x: point.x,
            y: point.y,
            modifiers: Modifiers {
                ctrl: point.keys & MK_CONTROL.0 != 0,
                shift: point.keys & MK_SHIFT.0 != 0,
                alt: point.keys & MK_ALT != 0,
                win: false,
            },
            allowed: DropEffects::from_bits(point.allowed),
            data: DragData::new(object),
        }
    }

    /// The pointer position as a [`Point`].
    pub fn position(&self) -> Point {
        Point::new(self.x, self.y)
    }

    /// The effect the held modifier keys ask for among the allowed ones;
    /// see [`DropEffects::preferred`].
    pub fn preferred_effect(&self) -> DropEffect {
        self.allowed.preferred(self.modifiers)
    }
}

/// A drag over a drop target. The target answers each event with the
/// [`DropEffect`] it would apply ([`DropEffect::None`] rejects); for
/// [`DragEvent::Leave`] the answer is ignored.
pub enum DragEvent<'a> {
    /// A drag entered the widget.
    Enter(DragInfo<'a>),
    /// The drag moved over the widget, or a repeat tick while it rests.
    Over(DragInfo<'a>),
    /// The drag left the widget, or was cancelled.
    Leave,
    /// The data was dropped. The answer is the effect reported back to the
    /// source (a [`DropEffect::Move`] tells it to remove its copy).
    Drop(DragInfo<'a>),
}

/// A custom drag image: straight-alpha pixels and where the pointer sits in
/// them.
///
/// Render it at the source window's DPI and from its theme so it matches the
/// surroundings; without one, [`begin_drag`] uses the source window's own
/// image.
#[derive(Clone, Debug)]
pub struct DragImage {
    /// The image, straight (not premultiplied) alpha.
    pub image: RgbaImage,
    /// The pointer position inside the image, in image pixels.
    pub hotspot: Point,
}

/// Starts a drag from `source` carrying `payload` and runs it to the end.
///
/// This is the modal `DoDragDrop` loop: it returns when the button is released
/// or Escape is pressed, with the effect the target applied, or
/// [`DropEffect::None`] when the drag was cancelled or dropped nowhere.
/// Call it while the mouse button is still down, for instance from the
/// message a [`ListView`](crate::ListView) mapped from `BeginDrag`. `allowed`
/// is what the app is prepared to do; a target picks one.
///
/// `image` overrides the drag image; `None` asks `source` to paint its own.
pub fn begin_drag(
    source: Hwnd,
    payload: &[u8],
    allowed: DropEffects,
    image: Option<&DragImage>,
) -> Result<DropEffect> {
    let bits = image.and_then(|drag| {
        DragImageBits::from_straight_rgba(
            drag.image.width,
            drag.image.height,
            &drag.image.pixels,
            (drag.hotspot.x, drag.hotspot.y),
        )
    });
    let data = DataObj::with_payload(payload);
    Ok(
        match sys::dnd::do_drag_drop(&data, allowed.bits(), source, bits)? {
            Outcome::Dropped(effect) => DropEffect::from_bits(effect),
            Outcome::Cancelled => DropEffect::None,
        },
    )
}

/// The names for the drag-and-drop types.
pub mod prelude {
    pub use super::{
        DragData, DragEvent, DragImage, DragInfo, DropEffect, DropEffects, begin_drag,
    };
}
