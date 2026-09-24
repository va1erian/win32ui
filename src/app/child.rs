#![forbid(unsafe_code)]

//! Secondary windows: non-modal [`open_window`](Ui::open_window) and modal
//! [`open_modal`](Ui::open_modal), plus the [`WindowHandle`] the opener keeps.
//!
//! Every window opened this way runs its own `App` with its own `Msg` type and
//! its own message queue, so its `update` is never re-entered (the same
//! invariant as the main window — see #33), while all windows share the one
//! process message loop. A non-modal window is owned by the opener (Windows
//! destroys it when the opener closes) but the opener keeps receiving input. A
//! modal window disables the opener, pumps messages until it closes, then
//! re-enables and reactivates the opener — in that order, so a disabled window
//! is not asked to come to the foreground.

use std::cell::RefCell;
use std::rc::Rc;

use crate::capture::RgbaImage;
use crate::error::Result;
use crate::geometry::Rect;
use crate::hwnd::Hwnd;
use crate::sys;
use crate::theme::Theme;
use crate::window::{TitleBar, Window, WindowClass, WindowExStyle, WindowStyle};

use super::core::Core;
use super::proxy::Proxy;
use super::run::AppHandler;
use super::spec::{App, WindowSpec};
use super::ui::Ui;

/// A handle to a secondary window opened with [`Ui::open_window`], held by the
/// opener. It sends messages to the child's queue (thread-safe, like
/// [`Proxy`](super::Proxy)), closes the child, and can capture it.
pub struct WindowHandle<M> {
    proxy: Proxy<M>,
    hwnd: Hwnd,
}

impl<M> Clone for WindowHandle<M> {
    fn clone(&self) -> WindowHandle<M> {
        WindowHandle {
            proxy: self.proxy.clone(),
            hwnd: self.hwnd,
        }
    }
}

impl<M: 'static> WindowHandle<M> {
    /// Sends `msg` to the child app's [`update`](App::update). Returns
    /// `Err(msg)` if the child has already closed.
    pub fn send(&self, msg: M) -> std::result::Result<(), M> {
        self.proxy.send(msg)
    }

    /// Closes the child window. Safe to call more than once.
    pub fn close(&self) {
        sys::window::destroy(self.hwnd);
    }

    /// Whether the child window is still open.
    pub fn is_alive(&self) -> bool {
        self.hwnd.is_alive()
    }

    /// The child window's handle.
    pub fn hwnd(&self) -> Hwnd {
        self.hwnd
    }

    /// Renders the child window into an image, for screenshots.
    pub fn capture(&self) -> Result<RgbaImage> {
        let size = sys::window::window_rect(self.hwnd).size();
        let captured = sys::capture::capture(self.hwnd, size.width, size.height)?;
        Ok(RgbaImage {
            width: captured.width as u32,
            height: captured.height as u32,
            pixels: captured.pixels,
        })
    }
}

/// Everything needed to run a widget-layer window: the [`Core`] and the
/// [`Window`] itself.
pub(crate) struct Built<A: App> {
    pub core: Rc<Core<A::Msg>>,
    pub window: Window,
}

/// Builds a widget-layer window (its class, `Window`, `Core`, handler and app)
/// without showing it or running a loop.
///
/// `owner` makes the new window owned by another (destroyed with it); `None`
/// makes it a standalone top-level window. `secondary` selects a `Core` that
/// does not quit the loop on close. The `theme` has already resolved any
/// inheritance from the opener.
pub(crate) fn build<A, F>(
    spec: WindowSpec,
    theme: Theme,
    owner: Option<Hwnd>,
    secondary: bool,
    make: F,
) -> Result<Built<A>>
where
    A: App + 'static,
    F: FnOnce(&mut Ui<A::Msg>) -> A,
{
    let (title, width, height, _) = spec.parts();
    let core = Rc::new(if secondary {
        Core::new_secondary(theme)
    } else {
        Core::new(theme)
    });
    let app: Rc<RefCell<Option<A>>> = Rc::new(RefCell::new(None));
    let handler = AppHandler::new(Rc::clone(&core), Rc::clone(&app));

    let dpi = sys::dpi::system_dpi();
    let class = WindowClass::register("win32ui.app", theme.background)?;
    let window = Window::create(
        class,
        owner,
        WindowStyle::overlapped().min_max().clip_children(),
        WindowExStyle::new().control_parent(),
        Rect::new(0, 0, width.to_px(dpi).value(), height.to_px(dpi).value()),
        title,
        handler,
    )?;
    core.set_hwnd(window.hwnd());
    window.set_theme(theme);
    // The material is applied after the theme, so its dark variant is already
    // set; a rejected call (unsupported Windows, high contrast, transparency
    // off) leaves the solid theme background.
    core.set_title_bar(spec.title_bar_kind());
    core.set_menu_in_strip(spec.menu_in_strip_kind());
    core.set_menu_strip_placement(spec.menu_strip_placement_kind());
    let mut backdrop_active =
        sys::apply_backdrop(window.hwnd(), spec.backdrop_kind(), theme.is_dark);
    if spec.title_bar_kind() == TitleBar::Colored {
        sys::apply_caption_colors(window.hwnd(), &theme);
    }
    if spec.title_bar_kind() == TitleBar::Extended {
        // The material only shows through an extended frame.
        backdrop_active &= sys::nc::enable_extended(window.hwnd());
        sys::apply_extended_colors(window.hwnd(), &theme, backdrop_active);
    }
    crate::theme::set_backdrop_active(window.hwnd(), backdrop_active);
    // Tab/Shift+Tab move between the window's focusable children, handled by
    // `IsDialogMessageW` in the pump.
    sys::looper::enable_dialog_nav(window.hwnd());

    // Construct the app once the window (and thus `Ui`) exists; messages raised
    // while `make` runs are queued and delivered once the shared loop resumes.
    let mut ui = Ui::new(Rc::clone(&core));
    let built = make(&mut ui);
    *app.borrow_mut() = Some(built);

    Ok(Built { core, window })
}

impl<M: 'static> Ui<M> {
    /// Opens a non-modal secondary window that runs its own [`App`], owned by
    /// this window (destroyed if this window closes) but not modal.
    ///
    /// The child inherits this window's theme unless `spec` set one explicitly.
    /// The returned [`WindowHandle`] sends messages to the child and closes it.
    pub fn open_window<B, F>(&self, spec: WindowSpec, make: F) -> Result<WindowHandle<B::Msg>>
    where
        B: App + 'static,
        F: FnOnce(&mut Ui<B::Msg>) -> B,
    {
        let owner = self.hwnd();
        let theme = spec.theme_or(self.theme());
        let built = build::<B, _>(spec, theme, Some(owner), true, make)?;
        let hwnd = built.window.hwnd();
        built.window.show_painted();
        // Keep the class registration alive for as long as the OS window lives.
        built.core.set_window(built.window);
        let proxy = Proxy::new(&built.core);
        Ok(WindowHandle { proxy, hwnd })
    }

    /// Opens a modal secondary window that runs its own [`App`], disabling this
    /// window and pumping messages until the child closes.
    ///
    /// The child's app closes with a value through [`Ui::close_with_result`];
    /// `None` is returned if it closed without one. The owner is re-enabled and
    /// reactivated in that order once the child is gone.
    pub fn open_modal<B, F, R>(&self, spec: WindowSpec, make: F) -> Option<R>
    where
        B: App + 'static,
        F: FnOnce(&mut Ui<B::Msg>) -> B,
        R: 'static,
    {
        let owner = self.hwnd();
        let theme = spec.theme_or(self.theme());
        let built = build::<B, _>(spec, theme, Some(owner), true, make).ok()?;
        let child = built.window.hwnd();
        built.core.set_window(built.window);

        // The order matters: disable the owner, pump until the child closes,
        // then re-enable before reactivating, so the owner is not asked to come
        // to the foreground while still disabled (which would silently fail and
        // leave it behind other applications).
        sys::window_input::set_enabled(owner, false);
        sys::window::show(child, sys::window::ShowKind::Normal);
        crate::looper::run_modal(child);
        sys::window_input::set_enabled(owner, true);
        sys::window_input::set_foreground(owner);

        built.core.take_result::<R>()
    }
}
