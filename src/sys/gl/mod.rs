//! The WGL (OpenGL) boundary: a rendering context on a child window and the
//! function loader `glow` uses. All OpenGL `unsafe` lives here.

mod context;
mod loader;

pub(crate) use context::Context;
