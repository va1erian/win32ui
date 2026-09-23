#![forbid(unsafe_code)]

//! The system backdrop material ([`Backdrop`]) drawn behind the client area.

/// The system backdrop material behind the window's client area.
///
/// Set with [`WindowSpec::backdrop`](crate::WindowSpec::backdrop). The material
/// is only shown when Windows supports it and the user has not turned
/// transparency effects (or high contrast) off; otherwise the window falls back
/// to the solid [`Theme::background`](crate::Theme::background). Ask
/// [`Ui::backdrop_active`](crate::Ui::backdrop_active) whether it is active.
///
/// Extending the material into the client area (so widgets sit on it) is the
/// extended-client-area work: GDI draws text with zero alpha over the glass and
/// DWM drops it, so content there has to be painted with Direct2D alpha first.
/// The extended title bar extends only the caption strip, and the window erases
/// that strip to black itself; [`Canvas::clear_to_backdrop`](crate::gdi::Canvas::clear_to_backdrop)
/// is the low-level seam for a widget that draws inside the strip.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Backdrop {
    /// No material: the window paints its opaque theme background (the default).
    #[default]
    None,
    /// Mica: the desktop-tinted main-window material (Explorer, Settings).
    Mica,
    /// Mica Alt: the stronger, more contrasty variant (File Explorer tabs).
    MicaAlt,
    /// Acrylic: the transient, blurred material (flyouts, menus).
    Acrylic,
}
