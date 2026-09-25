#![forbid(unsafe_code)]

//! Menus mapped to the app's `Msg`: a menu bar, context popups, and
//! owner-drawn dark menus.
//!
//! A [`Menu`] is built as data — labels, shortcuts, check/radio/disabled state
//! and an action per item — then either installed with
//! [`Ui::set_menu_bar`](crate::Ui::set_menu_bar) or shown with
//! [`Ui::popup`](crate::Ui::popup). The chosen item's action runs on the UI
//! thread and is delivered to [`App::update`](crate::App::update) through the
//! normal queue, so `update` is never re-entered.
//!
//! On a dark theme the items are owner-drawn (`MF_OWNERDRAW`, painted from
//! theme tokens on `WM_MEASUREITEM`/`WM_DRAWITEM`); on the light theme the
//! native Win32 menu is used unchanged.

mod access;
mod checked;
mod data;
mod draw;

use std::cell::Cell;
use std::rc::Rc;

use crate::accel::Shortcut;
use crate::color::Color;

pub(crate) use data::{BarEntry, BarKind, RenderItem};
use data::{Entry, Item, MenuData, Submenu, next_command, next_data};
pub(crate) use draw::{MenuPaint, measure, paint_item};

/// The action an item runs when chosen, typing the app's message.
pub(crate) type Action<M> = Rc<dyn Fn() -> M>;

/// A menu bar or popup, mapped to the app's `Msg` type.
///
/// Build one with [`Menu::new`] and the chaining methods; the closure each
/// action carries returns the message to deliver. `Menu` is cheap to clone
/// (it shares one underlying menu), but the builder methods require an
/// un-cloned menu: build the whole menu before installing it.
pub struct Menu<M> {
    data: Rc<MenuData<M>>,
}

impl<M> Clone for Menu<M> {
    fn clone(&self) -> Menu<M> {
        Menu {
            data: Rc::clone(&self.data),
        }
    }
}

impl<M: 'static> Menu<M> {
    /// An empty menu.
    pub fn new() -> Menu<M> {
        Menu {
            data: Rc::new(MenuData::new()),
        }
    }

    /// Appends a normal item that raises `action()` when chosen. Pass a
    /// [`Shortcut`] (or `None`) for the displayed accelerator text; add the
    /// same shortcut with [`Ui::accelerator`](crate::Ui::accelerator) to make
    /// it fire without opening the menu.
    pub fn item(
        mut self,
        label: impl Into<String>,
        shortcut: impl Into<Option<Shortcut>>,
        action: impl Fn() -> M + 'static,
    ) -> Menu<M> {
        self.push_item(label, shortcut, false, false, true, action);
        self
    }

    /// Appends a checkable item, shown with a tick when `checked`.
    pub fn checked_item(
        mut self,
        label: impl Into<String>,
        shortcut: impl Into<Option<Shortcut>>,
        checked: bool,
        action: impl Fn() -> M + 'static,
    ) -> Menu<M> {
        self.push_item(label, shortcut, checked, false, true, action);
        self
    }

    /// Appends a radio item, shown with a dot when `checked`.
    pub fn radio_item(
        mut self,
        label: impl Into<String>,
        shortcut: impl Into<Option<Shortcut>>,
        checked: bool,
        action: impl Fn() -> M + 'static,
    ) -> Menu<M> {
        self.push_item(label, shortcut, checked, true, true, action);
        self
    }

    /// Appends a disabled (greyed, unchosen) item.
    pub fn disabled_item(
        mut self,
        label: impl Into<String>,
        shortcut: impl Into<Option<Shortcut>>,
        action: impl Fn() -> M + 'static,
    ) -> Menu<M> {
        self.push_item(label, shortcut, false, false, false, action);
        self
    }

    /// Appends a separator line.
    pub fn separator(mut self) -> Menu<M> {
        self.entries_mut()
            .push(Entry::Separator { data: next_data() });
        self
    }

    /// Appends an item that opens `menu` as a submenu.
    pub fn submenu(mut self, label: impl Into<String>, menu: Menu<M>) -> Menu<M> {
        self.entries_mut().push(Entry::Submenu(Submenu {
            data: next_data(),
            label: label.into(),
            menu,
        }));
        self
    }

    fn push_item(
        &mut self,
        label: impl Into<String>,
        shortcut: impl Into<Option<Shortcut>>,
        checked: bool,
        radio: bool,
        enabled: bool,
        action: impl Fn() -> M + 'static,
    ) {
        self.entries_mut().push(Entry::Item(Item {
            id: next_command(),
            data: next_data(),
            label: label.into(),
            shortcut: shortcut.into(),
            checked: Cell::new(checked),
            key: None,
            radio,
            enabled,
            action: Rc::new(action),
        }));
    }

    /// Names the item just added so [`Menu::set_checked`] (or
    /// [`Ui::set_menu_checked`](crate::Ui::set_menu_checked)) can find it later.
    pub fn keyed(mut self, key: &'static str) -> Menu<M> {
        if let Some(Entry::Item(item)) = self.entries_mut().last_mut() {
            item.key = Some(key);
        }
        self
    }

    /// Sets the tick of the item named `key` on an installed menu, without a
    /// rebuild. Returns whether such an item exists. Callers must redraw a
    /// strip menu themselves; use [`Ui::set_menu_checked`](crate::Ui::set_menu_checked).
    pub(crate) fn set_checked(&self, key: &str, checked: bool) -> bool {
        self.data.set_checked(key, checked)
    }

    fn entries_mut(&mut self) -> &mut Vec<Entry<M>> {
        &mut Rc::get_mut(&mut self.data)
            .expect("a Menu cannot be extended after it is shared")
            .entries
    }
}

impl<M: 'static> Menu<M> {
    /// Builds (or rebuilds) the underlying `HMENU`; see
    /// [`MenuData::build`](MenuData).
    pub(crate) fn build(&self, bar: bool, owner_draw: bool, background: Color) -> isize {
        self.data.build(bar, owner_draw, background)
    }

    /// Destroys the built handle (used after a one-shot popup).
    pub(crate) fn destroy_handle(&self) {
        self.data.destroy_handle();
    }

    /// Whether the menu is currently built with owner-drawn items.
    pub(crate) fn is_owner_drawn(&self) -> bool {
        self.data.is_owner_drawn()
    }

    /// The action for the item with command id `id`, if any.
    pub(crate) fn find_action(&self, id: u16) -> Option<Action<M>> {
        self.data.find_action(id)
    }

    /// The number of top-level items on the bar (used by the strip menu).
    pub(crate) fn bar_len(&self) -> usize {
        self.data.bar_len()
    }

    /// The `index`-th top-level bar item, for the strip menu widget.
    pub(crate) fn bar_entry(&self, index: usize) -> Option<BarEntry<'_, M>> {
        self.data.bar_entry(index)
    }

    /// The owner-draw description for the item with render id `data`.
    pub(crate) fn render(&self, data: usize) -> Option<RenderItem<'_>> {
        self.data.render(data)
    }

    /// Every enabled item's shortcut paired with its action.
    pub(crate) fn shortcuts(&self) -> Vec<(Shortcut, Action<M>)> {
        self.data.shortcuts()
    }

    /// Every command id in this menu and its submenus (test support).
    #[cfg(test)]
    pub(crate) fn command_ids(&self) -> Vec<u16> {
        self.data.command_ids()
    }

    /// Every owner-draw render id in this menu and its submenus (test support).
    #[cfg(test)]
    pub(crate) fn data_ids(&self) -> Vec<usize> {
        self.data.data_ids()
    }
}

impl<M: 'static> Default for Menu<M> {
    fn default() -> Menu<M> {
        Menu::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A menu builds both ways and reports which style it is in.
    #[test]
    fn set_checked_updates_a_keyed_item() {
        let menu = Menu::new()
            .checked_item("Wrap", None, false, || 1)
            .keyed("wrap");
        assert!(menu.set_checked("wrap", true));
        assert!(!menu.set_checked("missing", true));
        let data = menu.data.entries.iter().find_map(|e| match e {
            Entry::Item(item) => Some(item.data),
            _ => None,
        });
        assert!(menu.render(data.unwrap()).unwrap().checked);
    }

    #[test]
    fn builds_native_and_owner_drawn() {
        let menu = Menu::<u8>::new()
            .item("Open", None, || 1)
            .checked_item("Wrap", None, true, || 2)
            .radio_item("Left", None, true, || 3)
            .disabled_item("Disabled", None, || 4)
            .separator()
            .submenu("More", Menu::new().item("Deep", None, || 5));

        let native = menu.build(true, false, crate::theme::Theme::light().raised);
        assert_ne!(native, 0, "the native menu did not build");
        assert!(!menu.is_owner_drawn());

        let drawn = menu.build(true, true, crate::theme::Theme::dark().raised);
        assert_ne!(drawn, 0, "the owner-drawn menu did not build");
        assert!(menu.is_owner_drawn());
    }

    /// A submenu item on a menu *bar* draws no chevron, while the same item in
    /// a popup does; nested items are always popup items (#67).
    #[test]
    fn bar_items_have_no_chevron() {
        let menu = Menu::<u8>::new()
            .item("&Plain", None, || 1)
            .submenu("&More", Menu::new().item("&Deep", None, || 2));
        let ids = menu.data_ids();
        let theme = crate::theme::Theme::dark();

        menu.build(true, true, theme.raised);
        let plain = menu.render(ids[0]).expect("bar item");
        assert!(plain.bar_item, "a top-level bar item is a bar item");
        assert!(!plain.submenu);

        let more = menu.render(ids[1]).expect("bar submenu");
        assert!(more.bar_item);
        assert!(!more.submenu, "a bar submenu draws no chevron");

        let deep = menu.render(ids[2]).expect("nested item");
        assert!(!deep.bar_item, "a nested item is a popup item");

        // The same menu as a popup: a submenu *does* draw a chevron.
        menu.build(false, true, theme.raised);
        let popup = menu.render(ids[1]).expect("popup submenu");
        assert!(popup.submenu && !popup.bar_item);
    }

    /// Every enabled shortcut is collected, recursively, for auto-registration.
    #[test]
    fn collects_shortcuts() {
        let menu = Menu::<u8>::new()
            .item("Open", Shortcut::ctrl(crate::message::Key::O), || 1)
            .disabled_item("Gone", Shortcut::ctrl(crate::message::Key::G), || 2)
            .submenu(
                "More",
                Menu::new().item("Deep", Shortcut::ctrl(crate::message::Key::D), || 3),
            );
        let shortcuts = menu.shortcuts();
        assert_eq!(shortcuts.len(), 2, "enabled shortcuts only");
        assert_eq!(shortcuts[0].0, Shortcut::ctrl(crate::message::Key::O));
        assert_eq!(shortcuts[1].0, Shortcut::ctrl(crate::message::Key::D));
    }

    /// Commands are looked up by id and owner-draw items expose their state.
    #[test]
    fn commands_and_render_round_trip() {
        let menu = Menu::<u8>::new()
            .item("Open", None, || 1)
            .checked_item("Wrap", None, true, || 2)
            .separator()
            .submenu("More", Menu::new().item("Deep", None, || 3));

        let commands = menu.command_ids();
        assert_eq!(commands.len(), 3);
        assert!(menu.find_action(commands[0]).is_some());
        assert!(menu.find_action(commands[2]).is_some(), "submenu command");
        assert!(menu.find_action(0xFFFF).is_none());

        let data = menu.data_ids();
        assert_eq!(data.len(), 5, "item, item, separator, submenu, deep item");
        let wrap = menu.render(data[1]).expect("checked item");
        assert!(wrap.checked && !wrap.radio && wrap.enabled);
        let separator = menu.render(data[2]).expect("separator");
        assert!(separator.separator);
        // The submenu's own item is found recursively.
        assert_eq!(menu.render(data[4]).expect("deep item").label, "Deep");
    }
}
