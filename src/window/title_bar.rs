#![forbid(unsafe_code)]

//! The window's title-bar style ([`TitleBar`]).

/// How the window's title bar is drawn.
///
/// Set with [`WindowSpec::title_bar`](crate::WindowSpec::title_bar). Unlike
/// [`Backdrop`](crate::Backdrop), which changes the client area, this changes
/// only the system-drawn caption: the buttons, their hit-testing and the resize
/// borders stay native.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TitleBar {
    /// The standard system title bar (the default).
    #[default]
    Standard,
    /// The standard title bar painted from theme tokens (`DWMWA_CAPTION_COLOR`
    /// and friends; Windows 11 only). Falls back to the system colours when
    /// unsupported or in high-contrast mode.
    Colored,
    /// The caption is removed and the strip becomes part of the client area, so
    /// the app's widgets can sit in it. The system still draws the min/max/close
    /// buttons — and so keeps snap layouts — and the resize borders stay native.
    /// The caption buttons' inset is [`Ui::caption_inset`](crate::Ui::caption_inset).
    Extended,
}
