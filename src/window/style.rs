#![forbid(unsafe_code)]

//! [`WindowStyle`] and [`WindowExStyle`]: builders for a window's `dwStyle`
//! and `dwExStyle` bits.

use windows::Win32::UI::WindowsAndMessaging as wam;

/// A builder for a window's `dwStyle` bits.
#[derive(Clone, Copy, Debug, Default)]
pub struct WindowStyle(u32);

impl WindowStyle {
    /// No styles.
    pub const fn new() -> WindowStyle {
        WindowStyle(0)
    }

    /// `WS_OVERLAPPEDWINDOW`: a resizable top-level window.
    pub const fn overlapped() -> WindowStyle {
        WindowStyle(wam::WS_OVERLAPPEDWINDOW.0)
    }

    /// A child window (`WS_CHILD`).
    pub const fn child(self) -> WindowStyle {
        WindowStyle(self.0 | wam::WS_CHILD.0)
    }

    /// A popup window (`WS_POPUP`).
    pub const fn popup(self) -> WindowStyle {
        WindowStyle(self.0 | wam::WS_POPUP.0)
    }

    /// Initially visible (`WS_VISIBLE`).
    pub const fn visible(self) -> WindowStyle {
        WindowStyle(self.0 | wam::WS_VISIBLE.0)
    }

    /// A thin border (`WS_BORDER`).
    pub const fn border(self) -> WindowStyle {
        WindowStyle(self.0 | wam::WS_BORDER.0)
    }

    /// A caption/title bar (`WS_CAPTION`).
    pub const fn caption(self) -> WindowStyle {
        WindowStyle(self.0 | wam::WS_CAPTION.0)
    }

    /// A resizable frame (`WS_THICKFRAME`).
    pub const fn resizable(self) -> WindowStyle {
        WindowStyle(self.0 | wam::WS_THICKFRAME.0)
    }

    /// Minimize/maximize boxes (`WS_MINIMIZEBOX | WS_MAXIMIZEBOX`).
    pub const fn min_max(self) -> WindowStyle {
        WindowStyle(self.0 | wam::WS_MINIMIZEBOX.0 | wam::WS_MAXIMIZEBOX.0)
    }

    /// A minimize box (`WS_MINIMIZEBOX`).
    pub const fn minimize_box(self) -> WindowStyle {
        WindowStyle(self.0 | wam::WS_MINIMIZEBOX.0)
    }

    /// A maximize box (`WS_MAXIMIZEBOX`).
    pub const fn maximize_box(self) -> WindowStyle {
        WindowStyle(self.0 | wam::WS_MAXIMIZEBOX.0)
    }

    /// A system menu (`WS_SYSMENU`).
    pub const fn sys_menu(self) -> WindowStyle {
        WindowStyle(self.0 | wam::WS_SYSMENU.0)
    }

    /// Clip children (`WS_CLIPCHILDREN`), avoiding flicker on resize.
    pub const fn clip_children(self) -> WindowStyle {
        WindowStyle(self.0 | wam::WS_CLIPCHILDREN.0)
    }

    /// Clip siblings (`WS_CLIPSIBLINGS`), so this child does not paint over its
    /// siblings.
    pub const fn clip_siblings(self) -> WindowStyle {
        WindowStyle(self.0 | wam::WS_CLIPSIBLINGS.0)
    }

    /// Include in the tab order (`WS_TABSTOP`).
    pub const fn tab_stop(self) -> WindowStyle {
        WindowStyle(self.0 | wam::WS_TABSTOP.0)
    }

    /// Adds raw style bits, for control-specific styles (e.g. `LVS_REPORT`).
    pub const fn with(self, bits: u32) -> WindowStyle {
        WindowStyle(self.0 | bits)
    }

    /// The accumulated style bits.
    pub const fn bits(self) -> u32 {
        self.0
    }
}

/// A builder for a window's `dwExStyle` bits.
#[derive(Clone, Copy, Debug, Default)]
pub struct WindowExStyle(u32);

impl WindowExStyle {
    /// No extended styles.
    pub const fn new() -> WindowExStyle {
        WindowExStyle(0)
    }

    /// A sunken client edge (`WS_EX_CLIENTEDGE`).
    pub const fn client_edge(self) -> WindowExStyle {
        WindowExStyle(self.0 | wam::WS_EX_CLIENTEDGE.0)
    }

    /// A tool window (`WS_EX_TOOLWINDOW`).
    pub const fn tool_window(self) -> WindowExStyle {
        WindowExStyle(self.0 | wam::WS_EX_TOOLWINDOW.0)
    }

    /// A container for dialog navigation (`WS_EX_CONTROLPARENT`), so
    /// `IsDialogMessageW` moves the focus among its children with Tab.
    pub const fn control_parent(self) -> WindowExStyle {
        WindowExStyle(self.0 | wam::WS_EX_CONTROLPARENT.0)
    }

    /// Add raw ex-style bits.
    pub const fn with(self, bits: u32) -> WindowExStyle {
        WindowExStyle(self.0 | bits)
    }

    /// The accumulated style bits.
    pub const fn bits(self) -> u32 {
        self.0
    }
}
