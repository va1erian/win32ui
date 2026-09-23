//! DirectWrite: font resolution, text formats, measurement and text layouts.
//!
//! The DirectWrite factory is free-threaded and its objects are immutable
//! once built, so everything here may be used from any thread; only drawing
//! (in [`super::Target`]) is tied to the UI thread's render target.

mod alias;
mod factory;
mod layout;
mod rich;

pub(crate) use factory::{FontRequest, ResolvedFont, TextFactory};
pub(crate) use layout::TextLayout;
pub(crate) use rich::{RichStyle, RichTextLayout};
