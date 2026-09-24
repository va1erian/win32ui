#![forbid(unsafe_code)]

//! The [`Ui`] handle: the widget layer's view of the top-level window.

use std::rc::Rc;

use crate::accel::Shortcut;
use crate::capture::RgbaImage;
use crate::controls::menu::Menu;
use crate::error::Result;
use crate::geometry::{Point, Rect};
use crate::hwnd::Hwnd;
use crate::layout::Insets;
use crate::message::TimerId;
use crate::sys;
use crate::theme::Theme;
use crate::units::{Dip, Px, dip};

use super::core::Core;
use super::layout::Layout;
use super::proxy::Proxy;

/// The widget-layer handle to the top-level window.
///
/// It creates widgets (their constructors take it), sets the title, closes the
/// window, quits the loop, and is where later issues hang theme, layout and
/// menus. It is cheap to clone (an `Rc`), so a widget can keep one to enqueue
/// messages from its own event handlers.
pub struct Ui<M> {
    core: Rc<Core<M>>,
    /// A container this handle was scoped to, so widgets it creates parent to
    /// the container instead of the top-level window. `None` for the window
    /// itself.
    parent: Option<Hwnd>,
}

impl<M: 'static> Ui<M> {
    pub(crate) fn new(core: Rc<Core<M>>) -> Ui<M> {
        Ui { core, parent: None }
    }

    /// A handle scoped to the container `parent`: widgets created through it
    /// become children of `parent`, while messages, theme and layout still
    /// belong to the top-level window.
    pub(crate) fn with_parent(&self, parent: Hwnd) -> Ui<M> {
        Ui {
            core: Rc::clone(&self.core),
            parent: Some(parent),
        }
    }

    /// A weak handle to the shared per-window core, for widgets that must not
    /// keep the window alive (e.g. a split divider whose window the core owns).
    pub(crate) fn core_weak(&self) -> std::rc::Weak<Core<M>> {
        Rc::downgrade(&self.core)
    }

    /// The handle widgets created through this `Ui` parent to: the top-level
    /// window, or the container this handle was scoped to with
    /// [`Ui::with_parent`].
    ///
    /// A scoped handle is for building a container's children only. Widgets
    /// that must own the *top-level* window — [`MaterialTopBar`],
    /// [`MaterialStatusBar`] and [`TaskDialog`] — have to be created from the
    /// window's own `Ui`, not a container-scoped one.
    ///
    /// [`MaterialTopBar`]: crate::MaterialTopBar
    /// [`MaterialStatusBar`]: crate::MaterialStatusBar
    /// [`TaskDialog`]: crate::TaskDialog
    pub fn hwnd(&self) -> Hwnd {
        self.parent.unwrap_or_else(|| self.core.hwnd())
    }

    /// The window's dots-per-inch.
    pub fn dpi(&self) -> u32 {
        sys::dpi::window_dpi(self.core.hwnd())
    }

    /// Whether the window accepts input. A modal window's owner reports `false`
    /// while the modal dialog is open.
    pub fn is_enabled(&self) -> bool {
        sys::window_input::is_enabled(self.core.hwnd())
    }

    /// The client area, in device pixels.
    pub fn client_rect(&self) -> Rect {
        sys::window::client_rect(self.core.hwnd())
    }

    /// The outer rectangle, in screen coordinates.
    pub fn window_rect(&self) -> Rect {
        sys::window::window_rect(self.core.hwnd())
    }

    /// Sets the window title.
    pub fn set_title(&self, title: &str) {
        let _ = sys::window::set_title(self.core.hwnd(), title);
    }

    /// The window's current theme.
    pub fn theme(&self) -> Theme {
        self.core.theme()
    }

    /// Whether DWM is drawing a backdrop material behind this window's client
    /// area. See [`WindowSpec::backdrop`](super::WindowSpec::backdrop).
    pub fn backdrop_active(&self) -> bool {
        crate::theme::backdrop_active(self.core.hwnd())
    }

    /// How much room the caption buttons need on the right of an extended title
    /// bar, in design units, so a `title_bar` layout item can leave it free.
    /// Empty on a standard title bar. Re-queried when the window's DPI changes.
    pub fn caption_inset(&self) -> Insets {
        let hwnd = self.core.hwnd();
        let buttons = crate::window::nc::caption_inset(hwnd);
        if buttons.is_empty() {
            return Insets::all(dip(0.0));
        }
        let client = sys::window::client_rect(hwnd);
        let right = (client.right - buttons.left).max(0);
        let dpi = sys::dpi::window_dpi(hwnd);
        Insets::new(dip(0.0), dip(0.0), Px(right).to_dip(dpi), dip(0.0))
    }

    /// The height of the top strip an extended title bar reserves for its
    /// caption buttons and menu bar, in design units. Content laid out by the
    /// app must start below it (the strip itself is left empty unless a widget
    /// there paints with the Direct2D path). Zero on a standard title bar.
    pub fn title_bar_height(&self) -> Dip {
        let hwnd = self.core.hwnd();
        if !crate::window::nc::is_extended(hwnd) {
            return dip(0.0);
        }
        let dpi = sys::dpi::window_dpi(hwnd);
        // The material top bar band sits directly below the strip, so any
        // layout that reserves the strip reserves the bar too.
        let px = sys::nc::title_bar_height(hwnd) + crate::window::nc::top_bar(hwnd);
        Px(px).to_dip(dpi)
    }

    /// The height of the bottom material status bar band, in design units, so
    /// the app leaves room for it with a bottom layout margin. Zero when no
    /// [`MaterialStatusBar`](crate::MaterialStatusBar) is installed.
    pub fn material_status_bar_height(&self) -> Dip {
        let px = self.core.material_status_bar_height_px();
        Px(px).to_dip(self.dpi())
    }

    /// The height of the top material top bar band, in design units, so the app
    /// leaves room for it with a top layout margin. Zero when no
    /// [`MaterialTopBar`](crate::MaterialTopBar) is installed.
    pub fn material_top_bar_height(&self) -> Dip {
        Px(self.core.material_top_bar_height_px()).to_dip(self.dpi())
    }

    /// The client rectangle a [`TopBarItem::native`](crate::TopBarItem::native)
    /// slot reserves, so the app can position its own child control there. The
    /// rect is in client coordinates (device pixels). `None` when no item with
    /// `id` is laid out.
    pub fn material_top_bar_slot(&self, id: impl Into<crate::TopBarId>) -> Option<Rect> {
        self.core.top_bar_rect(id.into())
    }

    /// Installs a material status bar's shared state and repaints.
    pub(crate) fn install_material_status_bar(
        &self,
        state: Rc<super::core::MaterialStatusBarState>,
    ) {
        self.core.set_material_status_bar(state);
        sys::window::invalidate(self.core.hwnd());
    }

    /// Installs a material top bar's shared state and reserves its band.
    pub(crate) fn install_material_top_bar(&self, state: Rc<super::top_bar::TopBarState>) {
        self.core.set_material_top_bar(state);
    }

    /// Installs the app's top bar event mapping.
    pub(crate) fn set_material_top_bar_events(
        &self,
        f: impl Fn(super::top_bar::TopBarEvent) -> Option<M> + 'static,
    ) {
        self.core.set_material_top_bar_events(f);
    }

    /// Recomputes the top bar's band height and item layout, then repaints.
    pub(crate) fn refresh_material_top_bar(&self) {
        self.core.refresh_material_top_bar();
    }

    /// The caption buttons' bounds, relative to the window's top-left corner (as
    /// DWM reports them), or an empty rectangle
    /// when DWM reports none (a standard title bar, or a platform without the
    /// attribute). DWM draws the buttons here, over the extended strip.
    pub fn caption_buttons(&self) -> Rect {
        sys::nc::caption_buttons_in_window(self.core.hwnd()).unwrap_or_default()
    }

    /// The menu bar's bounds (screen coordinates), or an empty rectangle when
    /// the window has no `HMENU` bar.
    pub fn menu_bar_rect(&self) -> Rect {
        sys::nc::menu_bar_rect(self.core.hwnd()).unwrap_or_default()
    }

    /// The extended frame strip's height (the caption incl. its top frame), in
    /// device pixels. This is the `cyTopHeight` passed to
    /// `DwmExtendFrameIntoClientArea`. Zero on a standard title bar.
    pub fn strip_height(&self) -> Px {
        Px(crate::window::nc::strip_height(self.core.hwnd()))
    }

    /// Switches the window and every widget created through it to `theme`,
    /// live. Widgets re-derive their colours, update their native parts and
    /// repaint; nothing is recreated.
    pub fn set_theme(&self, theme: Theme) {
        self.core.set_theme_value(theme);
        crate::theme::set_window_theme(self.core.hwnd(), theme);
        sys::set_titlebar_dark(self.core.hwnd(), theme.is_dark);
        sys::set_class_background(
            self.core.hwnd(),
            crate::theme::window_background(self.core.hwnd(), theme),
        );
        if self.core.title_bar() == crate::window::TitleBar::Colored {
            sys::apply_caption_colors(self.core.hwnd(), &theme);
        }
        if self.core.title_bar() == crate::window::TitleBar::Extended {
            sys::apply_extended_colors(self.core.hwnd(), &theme, self.backdrop_active());
        }
        crate::theme::retheme_children(self.core.hwnd(), &theme);
        // Owner-drawn menus must switch between native and themed items live.
        // The strip menu has no native bar to rebuild; it repaints instead.
        if !self.core.has_title_menu()
            && let Some(menu) = self.core.menu_bar()
            && menu.is_owner_drawn() != theme.is_dark
        {
            let handle = menu.build(true, theme.is_dark, theme.raised);
            sys::menu::set_bar(self.core.hwnd(), handle);
        }
        if !self.core.has_title_menu() && self.core.menu_bar().is_some() {
            sys::menu_seam::set(self.core.hwnd(), theme.is_dark.then_some(theme.raised));
        }
        sys::window::invalidate(self.core.hwnd());
    }

    /// Opts into (or out of) following the OS theme: while `true`, the window
    /// calls [`Ui::set_theme`] with a freshly read [`Theme::system`] whenever
    /// [`is_theme_change`](crate::is_theme_change) reports that the system
    /// theme changed. Off by default.
    pub fn follow_system_theme(&self, follow: bool) {
        let hwnd = self.core.hwnd();
        let apply: Option<crate::theme::ApplyTheme> = follow.then(|| {
            let ui = self.clone();
            Rc::new(move |theme: &Theme| ui.set_theme(*theme)) as _
        });
        crate::theme::set_window_follow_system(hwnd, apply);
    }

    /// Enqueues `msg` for delivery to [`App::update`](super::App::update). This
    /// is how custom widgets hand events back to the application.
    pub fn emit(&self, msg: M) {
        self.core.enqueue(msg);
    }

    /// Returns a thread-safe handle for sending messages from worker threads.
    /// `Proxy` is `Clone`, and `Send + Sync` when the message type is; its
    /// sends join the same queue [`emit`](Ui::emit) feeds, so
    /// [`App::update`](super::App::update) is still never re-entered.
    pub fn proxy(&self) -> Proxy<M> {
        Proxy::new(&self.core)
    }

    /// Installs the window's layout tree and lays it out immediately. The tree
    /// is laid out again automatically whenever the window is resized or its
    /// DPI changes; the application never sees `WM_SIZE`.
    pub fn set_layout(&self, layout: Layout) {
        self.core.set_layout(layout, self.clone());
    }

    /// Lays the installed tree out again. Call this after changing something
    /// the layout depends on, such as a widget's visibility.
    pub fn relayout(&self) {
        self.core.relayout();
    }

    /// Intercepts the window close request. Returning `Some(msg)` enqueues it
    /// and lets the app decide; returning `None` (the default) closes the
    /// window and quits.
    pub fn on_close(&self, f: impl Fn() -> Option<M> + 'static) {
        self.core.set_on_close(f);
    }

    /// Maps a `WM_TIMER` tick to a message. Only one mapping can be installed.
    pub fn on_timer(&self, f: impl Fn(TimerId) -> Option<M> + 'static) {
        self.core.set_on_timer(f);
    }

    /// Registers a keyboard shortcut. The closure maps an activation to a
    /// message; returning `None` ignores it. The shortcut fires whichever
    /// widget has focus. Many shortcuts can be registered; `Display` on the
    /// [`Shortcut`] renders the same text menus and tooltips show.
    pub fn accelerator(&self, shortcut: Shortcut, f: impl Fn() -> Option<M> + 'static) {
        self.core.add_accelerator(shortcut, f);
    }

    /// Installs `menu` as the window's menu bar. Every enabled item that has a
    /// [`Shortcut`] is also registered as an accelerator, so menus and
    /// shortcuts always agree. The window keeps a clone of the menu alive; the
    /// caller may drop its own handle.
    pub fn set_menu_bar(&self, menu: Menu<M>) {
        let theme = self.core.theme();
        let hwnd = self.core.hwnd();
        // On an extended title bar with an active material, draw the menu in
        // the strip instead of attaching a native bar, so the items sit on the
        // acrylic (GDI text over glass would vanish). When the material cannot
        // be shown (unsupported Windows, transparency off, high contrast, or
        // DirectWrite unavailable) the native bar is kept, unchanged.
        let strip = self.core.menu_in_strip()
            && self.core.title_bar() == crate::window::TitleBar::Extended
            && crate::theme::backdrop_active(hwnd)
            && !sys::dwm::high_contrast();
        if strip
            && let Some(title_menu) =
                super::title_menu::TitleBarMenu::new(menu.clone(), self.core.menu_strip_placement())
        {
            self.core.install_menu_bar(menu.clone());
            self.core.install_title_menu(title_menu);
            self.core.set_menu_accelerators(&menu);
            sys::window::invalidate(hwnd);
            self.core.relayout();
            return;
        }

        let handle = menu.build(true, theme.is_dark, theme.raised);
        // Install the menu before `SetMenu`, so the owner-draw measure/draw
        // messages raised while the bar is first laid out can find it.
        self.core.install_menu_bar(menu.clone());
        sys::menu::set_bar(hwnd, handle);
        sys::menu_seam::set(hwnd, theme.is_dark.then_some(theme.raised));
        for (shortcut, action) in menu.shortcuts() {
            self.core.add_accelerator(shortcut, move || Some(action()));
        }
        self.core.relayout();
    }

    /// Sets the tick of the installed menu-bar item named with
    /// [`Menu::keyed`], in both the native bar and the strip menu, without
    /// rebuilding the menu. Returns whether such an item exists.
    pub fn set_menu_checked(&self, key: &str, checked: bool) -> bool {
        self.core.set_menu_checked(key, checked)
    }

    /// Shows `menu` as a context popup at the screen position `at`, then
    /// delivers the chosen item's message to [`App::update`](super::App::update).
    /// Get `at` from [`Ui::cursor_position`] or a widget event.
    pub fn popup(&self, menu: &Menu<M>, at: Point) {
        let theme = self.core.theme();
        let handle = menu.build(false, theme.is_dark, theme.raised);
        let previous = self.core.set_popup(Some(menu.clone()));
        let command = sys::menu::track_popup(handle, self.core.hwnd(), at);
        self.core.set_popup(previous);
        menu.destroy_handle();
        if let Some(action) = command.and_then(|id| menu.find_action(id)) {
            self.emit(action());
        }
    }

    /// The cursor position, in screen coordinates. Useful as the point for
    /// [`Ui::popup`].
    pub fn cursor_position(&self) -> Point {
        sys::menu::cursor_position()
    }

    /// Starts a repeating timer and returns its id.
    pub fn set_timer(&self, millis: u32) -> Result<TimerId> {
        sys::window::set_timer(self.core.hwnd(), millis).map(TimerId)
    }

    /// Stops a timer started by [`Ui::set_timer`].
    pub fn kill_timer(&self, id: TimerId) {
        sys::window::kill_timer(self.core.hwnd(), id.0);
    }

    /// Closes the window. For the top-level window this also ends the message
    /// loop; a secondary window closes without disturbing it.
    pub fn close(&self) {
        sys::window::destroy(self.core.hwnd());
        if self.core.quits_loop() {
            crate::looper::quit(0);
        }
    }

    /// Closes the window, recording `result` for the opener of a modal window
    /// ([`Ui::open_modal`]) to receive. On a non-modal window the result is
    /// simply dropped when the window goes away.
    pub fn close_with_result<R: 'static>(&self, result: R) {
        self.core.set_result(Box::new(result));
        self.close();
    }

    /// Ends the message loop.
    pub fn quit(&self) {
        crate::looper::quit(0);
    }

    /// Ends the message loop with a specific exit code.
    pub fn quit_with(&self, code: i32) {
        crate::looper::quit(code);
    }

    /// Brings the window to the foreground. See [`Window::set_foreground`](crate::Window::set_foreground).
    pub fn set_foreground(&self) {
        sys::window_input::set_foreground(self.core.hwnd());
    }

    /// Whether the window is the foreground (active) window. DWM draws the
    /// backdrop material only for an active window.
    pub fn is_foreground(&self) -> bool {
        sys::window_input::is_foreground(self.core.hwnd())
    }

    /// Renders the window into an image, for screenshots.
    pub fn capture(&self) -> Result<RgbaImage> {
        let size = self.window_rect().size();
        let captured = sys::capture::capture(self.core.hwnd(), size.width, size.height)?;
        Ok(RgbaImage {
            width: captured.width as u32,
            height: captured.height as u32,
            pixels: captured.pixels,
        })
    }

    /// Renders the window's screen rectangle into an image, including the
    /// DWM-drawn caption buttons, frame and backdrop material. The window must
    /// be on screen and unobscured. See [`Window::capture_screen`](crate::Window::capture_screen).
    pub fn capture_screen(&self) -> Result<RgbaImage> {
        let rect = self.window_rect();
        let captured = sys::capture::capture_screen(rect)?;
        Ok(RgbaImage {
            width: captured.width as u32,
            height: captured.height as u32,
            pixels: captured.pixels,
        })
    }

    /// Renders the window's DWM-composited surface into an image, including
    /// the caption buttons, frame, rounded corners and backdrop material,
    /// without raising the window or moving the pointer. Requires the `wgc`
    /// feature. See [`Window::capture_composited`](crate::Window::capture_composited).
    #[cfg(feature = "wgc")]
    pub fn capture_composited(&self) -> Result<RgbaImage> {
        crate::capture::capture_hwnd(self.core.hwnd())
    }
}

impl<M> Clone for Ui<M> {
    fn clone(&self) -> Ui<M> {
        Ui {
            core: Rc::clone(&self.core),
            parent: self.parent,
        }
    }
}
