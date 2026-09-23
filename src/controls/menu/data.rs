#![forbid(unsafe_code)]

//! The built `HMENU` and its item data: id assignment, build, lookup and the
//! owner-draw description. Internal to the [`Menu`](super::Menu) builder.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::accel::Shortcut;
use crate::color::Color;
use crate::gdi::Brush;
use crate::sys;

use super::Menu;

/// What an item's action does when chosen: produce one app message.
pub(super) type Action<M> = Rc<dyn Fn() -> M>;

/// First command id assigned to a menu item. Above the low ids child controls
/// use, below the accelerator command range (`sys::looper`).
const COMMAND_BASE: u16 = 0x1000;
/// Last id in the reserved menu command range.
const COMMAND_TOP: u16 = 0xEFFF;

thread_local! {
    /// Source of unique menu command ids for this thread.
    static NEXT_COMMAND: Cell<u16> = const { Cell::new(COMMAND_BASE) };
    /// Source of unique owner-draw render ids (independent of command ids).
    static NEXT_DATA: Cell<usize> = const { Cell::new(1) };
}

pub(super) fn next_command() -> u16 {
    NEXT_COMMAND.with(|next| {
        let id = next.get();
        next.set(if id >= COMMAND_TOP {
            COMMAND_BASE
        } else {
            id + 1
        });
        id
    })
}

pub(super) fn next_data() -> usize {
    NEXT_DATA.with(|next| {
        let data = next.get();
        next.set(data.wrapping_add(1).max(1));
        data
    })
}

pub(super) struct Item<M> {
    pub(super) id: u16,
    pub(super) data: usize,
    pub(super) label: String,
    pub(super) shortcut: Option<Shortcut>,
    pub(super) checked: bool,
    pub(super) radio: bool,
    pub(super) enabled: bool,
    pub(super) action: Action<M>,
}

pub(super) struct Submenu<M> {
    pub(super) data: usize,
    pub(super) label: String,
    pub(super) menu: Menu<M>,
}

pub(super) enum Entry<M> {
    Item(Item<M>),
    Separator { data: usize },
    Submenu(Submenu<M>),
}

/// The shared menu description. The `HMENU` is built lazily and owned here; the
/// last clone destroys it.
pub(super) struct MenuData<M> {
    pub(super) entries: Vec<Entry<M>>,
    handle: Cell<isize>,
    owner_draw: Cell<bool>,
    /// The themed background brush, kept alive while the menu uses it.
    background: RefCell<Option<Brush>>,
}

impl<M> Drop for MenuData<M> {
    fn drop(&mut self) {
        sys::menu::destroy(self.handle.replace(0));
    }
}

/// One item as the owner-draw painter sees it (no allocation).
pub(crate) struct RenderItem<'a> {
    /// Display label, with `&` accelerator markers.
    pub label: &'a str,
    /// Shortcut, displayed right-aligned.
    pub shortcut: Option<Shortcut>,
    /// Whether a tick is shown.
    pub checked: bool,
    /// Whether the tick is a radio dot.
    pub radio: bool,
    /// Whether the item can be chosen.
    pub enabled: bool,
    /// Whether the item opens a submenu (a chevron is drawn).
    pub submenu: bool,
    /// Whether this entry is a separator line.
    pub separator: bool,
}

impl<M: 'static> MenuData<M> {
    /// An empty description.
    pub(super) fn new() -> MenuData<M> {
        MenuData {
            entries: Vec::new(),
            handle: Cell::new(0),
            owner_draw: Cell::new(false),
            background: RefCell::new(None),
        }
    }

    /// Builds (or rebuilds) the underlying `HMENU` for this level and its
    /// submenus, returning the handle. Any previous handle is destroyed. A
    /// `bar` menu is a top-level menu bar; otherwise it is a popup.
    pub(super) fn build(&self, bar: bool, owner_draw: bool, background: Color) -> isize {
        sys::menu::destroy(self.handle.replace(0));
        self.background.replace(None);
        let handle = if bar {
            sys::menu::create_bar()
        } else {
            sys::menu::create_popup()
        };
        if owner_draw && let Ok(brush) = Brush::solid(background) {
            sys::menu::set_background(handle, brush.raw());
            self.background.replace(Some(brush));
        }
        for entry in &self.entries {
            match entry {
                Entry::Item(item) => self.append_item(handle, item, owner_draw),
                Entry::Separator { data } => {
                    if owner_draw {
                        sys::menu::append_owned_separator(handle, *data);
                    } else {
                        sys::menu::append_separator_native(handle);
                    }
                }
                Entry::Submenu(submenu) => {
                    let child = submenu.menu.build(false, owner_draw, background);
                    if owner_draw {
                        sys::menu::append_owned_popup(handle, child, submenu.data);
                    } else {
                        sys::menu::append_popup_native(handle, child, &submenu.label);
                    }
                    submenu.menu.data.forget_handle();
                }
            }
        }
        self.handle.set(handle);
        self.owner_draw.set(owner_draw);
        handle
    }

    fn append_item(&self, handle: isize, item: &Item<M>, owner_draw: bool) {
        if owner_draw {
            sys::menu::append_owned(handle, item.id, item.data, item.enabled);
        } else {
            let mut text = item.label.clone();
            if let Some(shortcut) = item.shortcut {
                text.push('\t');
                text.push_str(&shortcut.to_string());
            }
            sys::menu::append_native(handle, item.id, &text, item.enabled);
        }
        if item.checked || item.radio {
            sys::menu::check_item(handle, item.id, item.checked, item.radio);
        }
    }

    /// Destroys the built handle (used after a one-shot popup).
    pub(super) fn destroy_handle(&self) {
        sys::menu::destroy(self.handle.replace(0));
    }

    /// Forgets the handle without destroying it, because a parent menu now owns
    /// it.
    fn forget_handle(&self) {
        self.handle.set(0);
    }

    /// Whether the menu is currently built with owner-drawn items.
    pub(super) fn is_owner_drawn(&self) -> bool {
        self.owner_draw.get()
    }

    /// The action for the item with command id `id`, if any.
    pub(super) fn find_action(&self, id: u16) -> Option<Action<M>> {
        for entry in &self.entries {
            match entry {
                Entry::Item(item) if item.id == id && item.enabled => {
                    return Some(Rc::clone(&item.action));
                }
                Entry::Submenu(submenu) => {
                    if let Some(action) = submenu.menu.data.find_action(id) {
                        return Some(action);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// The owner-draw description for the item with render id `data`, searching
    /// submenus.
    pub(super) fn render(&self, data: usize) -> Option<RenderItem<'_>> {
        for entry in &self.entries {
            match entry {
                Entry::Item(item) if item.data == data => {
                    return Some(RenderItem {
                        label: &item.label,
                        shortcut: item.shortcut,
                        checked: item.checked,
                        radio: item.radio,
                        enabled: item.enabled,
                        submenu: false,
                        separator: false,
                    });
                }
                Entry::Separator { data: entry_data } if *entry_data == data => {
                    return Some(RenderItem {
                        label: "",
                        shortcut: None,
                        checked: false,
                        radio: false,
                        enabled: false,
                        submenu: false,
                        separator: true,
                    });
                }
                Entry::Submenu(submenu) if submenu.data == data => {
                    return Some(RenderItem {
                        label: &submenu.label,
                        shortcut: None,
                        checked: false,
                        radio: false,
                        enabled: true,
                        submenu: true,
                        separator: false,
                    });
                }
                _ => {}
            }
        }
        for entry in &self.entries {
            if let Entry::Submenu(submenu) = entry
                && let Some(found) = submenu.menu.data.render(data)
            {
                return Some(found);
            }
        }
        None
    }

    /// Every enabled item's shortcut paired with its action, for auto
    /// registration by [`Ui::set_menu_bar`](crate::Ui::set_menu_bar).
    pub(super) fn shortcuts(&self) -> Vec<(Shortcut, Action<M>)> {
        let mut found = Vec::new();
        self.collect_shortcuts(&mut found);
        found
    }

    fn collect_shortcuts(&self, found: &mut Vec<(Shortcut, Action<M>)>) {
        for entry in &self.entries {
            match entry {
                Entry::Item(item) if item.enabled => {
                    if let Some(shortcut) = item.shortcut {
                        found.push((shortcut, Rc::clone(&item.action)));
                    }
                }
                Entry::Submenu(submenu) => submenu.menu.data.collect_shortcuts(found),
                _ => {}
            }
        }
    }

    /// Every command id in this menu and its submenus (test support).
    #[cfg(test)]
    pub(super) fn command_ids(&self) -> Vec<u16> {
        let mut ids = Vec::new();
        for entry in &self.entries {
            match entry {
                Entry::Item(item) => ids.push(item.id),
                Entry::Submenu(submenu) => ids.extend(submenu.menu.data.command_ids()),
                _ => {}
            }
        }
        ids
    }

    /// Every owner-draw render id in this menu and its submenus (test support).
    #[cfg(test)]
    pub(super) fn data_ids(&self) -> Vec<usize> {
        let mut ids = Vec::new();
        for entry in &self.entries {
            match entry {
                Entry::Item(item) => ids.push(item.data),
                Entry::Separator { data } => ids.push(*data),
                Entry::Submenu(submenu) => {
                    ids.push(submenu.data);
                    ids.extend(submenu.menu.data.data_ids());
                }
            }
        }
        ids
    }
}
