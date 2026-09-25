#![forbid(unsafe_code)]

//! The per-window widget-layer core: the message queue, drain scheduling, and
//! the shared state both [`Ui`](super::Ui) and the window handler need.
//!
//! The app itself deliberately lives *outside* this type, in the handler's
//! `RefCell`: `Ui` must stay usable while [`App::update`](super::App::update)
//! runs, and `update` holds the app borrow, so putting the app here would make
//! the drain's `try_borrow_mut` deadlock against itself.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::ffi::c_void;
use std::rc::Rc;

use crate::accel::Shortcut;
use crate::app::layout::Layout;
use crate::app::spec::MenuStripPlacement;
use crate::app::title_menu::TitleBarMenu;
use crate::controls::menu::{Menu, MenuPaint, measure, paint_item};
use crate::d2d::D2dSurface;
use crate::hwnd::Hwnd;
use crate::message::{Message, TimerId};
use crate::sys;
use crate::theme::Theme;
use crate::window::{TitleBar, Window};

mod accel;
mod layout;
mod status_bar;
mod title_menu;
mod top_bar;

pub(crate) use status_bar::MaterialStatusBarState;
pub(crate) use title_menu::TitleMenuAccess;

/// Maps a close request to an optional app message.
type CloseMapper<M> = Box<dyn Fn() -> Option<M>>;
/// Maps a timer tick to an optional app message.
type TimerMapper<M> = Box<dyn Fn(TimerId) -> Option<M>>;
/// Maps an accelerator activation to an optional app message.
type AccelMapper<M> = Box<dyn Fn() -> Option<M>>;
/// Observes a raw window message, returning whether it claimed it.
type RawMessageHandler = Box<dyn Fn(*const c_void) -> bool>;

/// A registered shortcut and the message it raises. The registration order is
/// the command id assigned to the shortcut in the window's accelerator table.
struct Accelerator<M> {
    shortcut: Shortcut,
    mapper: AccelMapper<M>,
    /// Registered by the menu bar, so a reinstall replaces it.
    from_menu: bool,
}

/// The shared, interior-mutable state behind a widget-layer window.
pub(crate) struct Core<M> {
    hwnd: Cell<Hwnd>,
    queue: RefCell<VecDeque<M>>,
    drain: u32,
    on_close: RefCell<Option<CloseMapper<M>>>,
    on_timer: RefCell<Option<TimerMapper<M>>>,
    /// Maps a `WM_DISPLAYCHANGE` notification to an optional app message.
    on_display_change: RefCell<Option<CloseMapper<M>>>,
    /// Observes every raw message before it is decoded (see
    /// [`Ui::on_raw_message`](super::Ui::on_raw_message)).
    on_raw_message: RefCell<Option<RawMessageHandler>>,
    accelerators: RefCell<Vec<Accelerator<M>>>,
    theme: Cell<Theme>,
    title_bar: Cell<TitleBar>,
    layout: RefCell<Option<Layout>>,
    /// Split-divider child windows, kept alive for the layout's lifetime.
    dividers: RefCell<Vec<Window>>,
    result: RefCell<Option<Box<dyn Any>>>,
    quits_loop: bool,
    // Holds the `Window` (and so its class registration) alive for the window's
    // lifetime. Only secondary windows set this; the main window in `run_app`
    // keeps its `Window` local.
    _window: RefCell<Option<Window>>,
    menu_bar: RefCell<Option<Menu<M>>>,
    menu_popup: RefCell<Option<Menu<M>>>,
    /// Whether the spec asked for the menu to live on the acrylic strip.
    menu_in_strip: Cell<bool>,
    /// Where the strip menu is drawn.
    menu_strip_placement: Cell<MenuStripPlacement>,
    /// The strip menu, installed once the strip mode is active.
    title_menu: RefCell<Option<TitleBarMenu<M>>>,
    /// The top-level window's transparent Direct2D surface for the strip.
    strip_surface: RefCell<Option<D2dSurface>>,
    /// The bottom material status bar, if the app installed one.
    material_status_bar: RefCell<Option<Rc<status_bar::MaterialStatusBarState>>>,
    /// The top material top bar, if the app installed one.
    material_top_bar: RefCell<Option<Rc<crate::app::top_bar::TopBarState>>>,
    /// The app's mapping from top bar events to messages.
    top_bar_events: RefCell<Option<top_bar::TopBarMapper<M>>>,
    /// The window icon, kept alive for the window's lifetime (`WM_SETICON`
    /// stores the handle rather than copying it).
    icon: RefCell<Option<crate::window::Icon>>,
}

impl<M> Core<M> {
    /// A top-level window that quits the message loop when it closes.
    pub(crate) fn new(theme: Theme) -> Core<M> {
        Core::with_theme(theme, true)
    }

    /// A secondary window (owned by another): closing it leaves the shared loop
    /// running.
    pub(crate) fn new_secondary(theme: Theme) -> Core<M> {
        Core::with_theme(theme, false)
    }

    fn with_theme(theme: Theme, quits_loop: bool) -> Core<M> {
        Core {
            hwnd: Cell::new(Hwnd::NULL),
            queue: RefCell::new(VecDeque::new()),
            drain: sys::message::drain_message(),
            on_close: RefCell::new(None),
            on_timer: RefCell::new(None),
            on_display_change: RefCell::new(None),
            on_raw_message: RefCell::new(None),
            accelerators: RefCell::new(Vec::new()),
            theme: Cell::new(theme),
            title_bar: Cell::new(TitleBar::Standard),
            layout: RefCell::new(None),
            dividers: RefCell::new(Vec::new()),
            result: RefCell::new(None),
            quits_loop,
            _window: RefCell::new(None),
            menu_bar: RefCell::new(None),
            menu_popup: RefCell::new(None),
            menu_in_strip: Cell::new(false),
            menu_strip_placement: Cell::new(MenuStripPlacement::default()),
            title_menu: RefCell::new(None),
            strip_surface: RefCell::new(None),
            material_status_bar: RefCell::new(None),
            material_top_bar: RefCell::new(None),
            top_bar_events: RefCell::new(None),
            icon: RefCell::new(None),
        }
    }

    /// Keeps `icon` alive for the window's lifetime.
    pub(crate) fn keep_icon(&self, icon: crate::window::Icon) {
        *self.icon.borrow_mut() = Some(icon);
    }

    /// Whether closing this window also quits the message loop.
    pub(crate) fn quits_loop(&self) -> bool {
        self.quits_loop
    }

    /// Stores the window handle, keeping its class registered.
    pub(crate) fn set_window(&self, window: Window) {
        self._window.replace(Some(window));
    }

    /// Records the value a modal window's app produced on close.
    pub(crate) fn set_result(&self, result: Box<dyn Any>) {
        self.result.replace(Some(result));
    }

    /// Takes the recorded modal result, if it has the expected type.
    pub(crate) fn take_result<R: 'static>(&self) -> Option<R> {
        self.result
            .borrow_mut()
            .take()?
            .downcast::<R>()
            .ok()
            .map(|boxed| *boxed)
    }

    pub(crate) fn set_hwnd(&self, hwnd: Hwnd) {
        self.hwnd.set(hwnd);
    }

    pub(crate) fn hwnd(&self) -> Hwnd {
        self.hwnd.get()
    }

    /// Whether `code` is this window's private drain message.
    pub(crate) fn is_drain(&self, code: u32) -> bool {
        self.drain != 0 && code == self.drain
    }

    /// Appends `msg` and, if the queue was empty, posts the private drain
    /// message that will deliver it. Posting only on the emptyâ†’non-empty edge
    /// means a burst of messages costs a single drain.
    pub(crate) fn enqueue(&self, msg: M) {
        let was_empty = self.queue.borrow().is_empty();
        self.queue.borrow_mut().push_back(msg);
        if was_empty {
            self.post_drain();
        }
    }

    /// Pops the oldest queued message, if any.
    pub(crate) fn next(&self) -> Option<M> {
        self.queue.borrow_mut().pop_front()
    }

    /// Puts a message back at the front (used when the app is busy).
    pub(crate) fn put_back(&self, msg: M) {
        self.queue.borrow_mut().push_front(msg);
    }

    /// Posts the private drain message to the window.
    pub(crate) fn post_drain(&self) {
        if self.drain != 0 {
            let _ = sys::window::post_message(self.hwnd.get(), self.drain, 0, 0);
        }
    }

    pub(crate) fn set_on_close(&self, f: impl Fn() -> Option<M> + 'static) {
        self.on_close.replace(Some(Box::new(f)));
    }

    pub(crate) fn set_on_timer(&self, f: impl Fn(TimerId) -> Option<M> + 'static) {
        self.on_timer.replace(Some(Box::new(f)));
    }

    pub(crate) fn set_on_display_change(&self, f: impl Fn() -> Option<M> + 'static) {
        self.on_display_change.replace(Some(Box::new(f)));
    }

    pub(crate) fn set_on_raw_message(&self, f: impl Fn(*const c_void) -> bool + 'static) {
        self.on_raw_message.replace(Some(Box::new(f)));
    }

    /// Offers a raw message to the installed hook, returning whether it claimed
    /// the message. `false` when no hook is installed.
    pub(crate) fn map_raw_message(&self, msg: *const c_void) -> bool {
        self.on_raw_message
            .borrow()
            .as_ref()
            .is_some_and(|f| f(msg))
    }

    /// Maps a close request: `Some(msg)` intercepts it (the app decides),
    /// `None` means "use the default" (close and quit).
    pub(crate) fn map_close(&self) -> Option<M> {
        self.on_close.borrow().as_ref().and_then(|f| f())
    }

    /// Maps a timer tick: `Some(msg)` is enqueued by the caller.
    pub(crate) fn map_timer(&self, id: TimerId) -> Option<M> {
        self.on_timer.borrow().as_ref().and_then(|f| f(id))
    }

    /// Maps a display-layout change: `Some(msg)` is enqueued by the caller.
    pub(crate) fn map_display_change(&self) -> Option<M> {
        self.on_display_change.borrow().as_ref().and_then(|f| f())
    }

    /// The window's current theme.
    pub(crate) fn theme(&self) -> Theme {
        self.theme.get()
    }

    /// Records a new theme for the window.
    pub(crate) fn set_theme_value(&self, theme: Theme) {
        self.theme.set(theme);
    }

    /// Records the window's title-bar style.
    pub(crate) fn set_title_bar(&self, title_bar: TitleBar) {
        self.title_bar.set(title_bar);
    }

    /// The window's title-bar style.
    pub(crate) fn title_bar(&self) -> TitleBar {
        self.title_bar.get()
    }
}

impl<M: 'static> Core<M> {
    /// Installs `menu` as this window's menu bar (keeping a clone alive for
    /// owner-draw lookups and live re-theming).
    pub(crate) fn install_menu_bar(&self, menu: Menu<M>) {
        *self.menu_bar.borrow_mut() = Some(menu);
    }

    /// The installed menu bar, if any.
    pub(crate) fn menu_bar(&self) -> Option<Menu<M>> {
        self.menu_bar.borrow().clone()
    }

    /// Replaces the active popup menu, returning the previous one so the caller
    /// can restore it after tracking.
    pub(crate) fn set_popup(&self, menu: Option<Menu<M>>) -> Option<Menu<M>> {
        self.menu_popup.replace(menu)
    }

    /// The menu bar (preferred) or the active popup, for owner-draw lookups.
    fn active_menu(&self) -> Option<Menu<M>> {
        self.menu_popup.borrow().clone().or_else(|| self.menu_bar())
    }

    /// Maps a menu command id (a `WM_COMMAND` with no control, or the id
    /// returned by `TPM_RETURNCMD`) to its action's message.
    pub(crate) fn map_menu_command(&self, id: u16) -> Option<M> {
        let action = self.menu_bar.borrow().as_ref()?.find_action(id)?;
        Some(action())
    }

    /// Paints an owner-drawn menu item, if `message` is one of ours.
    pub(crate) fn draw_menu_item(&self, message: &Message) -> bool {
        let Message::DrawItem {
            data,
            dc,
            area,
            state,
            menu: true,
            ..
        } = message
        else {
            return false;
        };
        let Some(menu) = self.active_menu() else {
            return false;
        };
        let Some(item) = menu.render(*data) else {
            return false;
        };
        let dpi = sys::dpi::window_dpi(self.hwnd.get());
        let paint = MenuPaint::from_theme(&self.theme.get());
        paint_item(*dc, *area, *state, &item, &paint, dpi);
        true
    }

    /// Reports the measured size for an owner-drawn menu item, if `message` is
    /// one of ours.
    pub(crate) fn measure_menu_item(&self, message: &Message) -> bool {
        let Message::MeasureItem {
            data, menu: true, ..
        } = message
        else {
            return false;
        };
        let Some(menu) = self.active_menu() else {
            return false;
        };
        let Some(item) = menu.render(*data) else {
            return false;
        };
        let dpi = sys::dpi::window_dpi(self.hwnd.get());
        sys::message::set_measured_size(measure(&item, dpi));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controls::menu::Menu;
    use crate::message::Key;

    #[test]
    fn reinstalling_the_menu_bar_replaces_its_accelerators() {
        let core: Core<i32> = Core::new(Theme::light());
        core.add_accelerator(Shortcut::ctrl(Key::N), || Some(1));
        for _ in 0..3 {
            let menu = Menu::new().item("Quit", Shortcut::ctrl(Key::Q), || 2);
            core.set_menu_accelerators(&menu);
        }
        assert_eq!(core.accelerators.borrow().len(), 2);
    }

    #[test]
    fn accelerators_map_their_reserved_command_ids() {
        // No window is set, so `add_accelerator` skips the table build but
        // still records the mapper.
        let core: Core<u32> = Core::new(Theme::light());
        core.add_accelerator(Shortcut::ctrl(Key::N), || Some(7));
        core.add_accelerator(Shortcut::ctrl(Key::Q), || None);

        assert_eq!(core.map_accelerator(sys::looper::command_id(0)), Some(7));
        assert_eq!(core.map_accelerator(sys::looper::command_id(1)), None);
        assert_eq!(core.map_accelerator(sys::looper::command_id(2)), None);
        assert_eq!(
            core.map_accelerator(0),
            None,
            "a control id is not a shortcut"
        );
    }

    #[test]
    fn menu_commands_map_to_messages() {
        let core: Core<u32> = Core::new(Theme::light());
        let menu = Menu::new()
            .item("First", None, || 7)
            .item("Second", None, || 9);
        let ids = menu.command_ids();
        assert_eq!(ids.len(), 2);

        assert_eq!(core.map_menu_command(ids[0]), None, "no menu installed");
        core.install_menu_bar(menu);
        assert_eq!(core.map_menu_command(ids[0]), Some(7));
        assert_eq!(core.map_menu_command(ids[1]), Some(9));
        assert_eq!(core.map_menu_command(0xFFFF), None);
    }
}
