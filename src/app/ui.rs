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
}

impl<M: 'static> Ui<M> {
    pub(crate) fn new(core: Rc<Core<M>>) -> Ui<M> {
        Ui { core }
    }

    /// A weak handle to the shared per-window core, for widgets that must not
    /// keep the window alive (e.g. a split divider whose window the core owns).
    pub(crate) fn core_weak(&self) -> std::rc::Weak<Core<M>> {
        Rc::downgrade(&self.core)
    }

    /// The top-level window's handle.
    pub fn hwnd(&self) -> Hwnd {
        self.core.hwnd()
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
        Px(sys::nc::title_bar_height(hwnd)).to_dip(dpi)
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
        if let Some(menu) = self.core.menu_bar()
            && menu.is_owner_drawn() != theme.is_dark
        {
            let handle = menu.build(true, theme.is_dark, theme.raised);
            sys::menu::set_bar(self.core.hwnd(), handle);
        }
        sys::window::invalidate(self.core.hwnd());
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
        let handle = menu.build(true, theme.is_dark, theme.raised);
        // Install the menu before `SetMenu`, so the owner-draw measure/draw
        // messages raised while the bar is first laid out can find it.
        self.core.install_menu_bar(menu.clone());
        sys::menu::set_bar(self.core.hwnd(), handle);
        for (shortcut, action) in menu.shortcuts() {
            self.core.add_accelerator(shortcut, move || Some(action()));
        }
        self.core.relayout();
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
}

impl<M> Clone for Ui<M> {
    fn clone(&self) -> Ui<M> {
        Ui {
            core: Rc::clone(&self.core),
        }
    }
}
