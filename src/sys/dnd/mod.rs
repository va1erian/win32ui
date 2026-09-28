//! OLE drag and drop: the in-process data object, the drop source, the drop
//! target and the drag-image helpers.
//!
//! # Who owns OLE
//!
//! `IDropTarget` and `DoDragDrop` need the calling thread to be an OLE
//! apartment. [`ole::ensure`] runs `OleInitialize` lazily, on the UI thread,
//! the first time a drag source or drop target is created; the matching
//! `OleUninitialize` runs when that thread exits. `OleInitialize` is
//! reference-counted, so an application that initialises OLE itself is not
//! disturbed. A thread already in a multithreaded apartment cannot host drag
//! and drop, and the calls fail with the OS error.

mod data;
mod hglobal;
mod image;
mod ole;
mod source;
mod target;

pub(crate) use data::DataObj;
pub(crate) use image::DragImageBits;
pub(crate) use source::{Outcome, do_drag_drop};
pub(crate) use target::{DragPoint, Registration, TargetSink, register_target};
