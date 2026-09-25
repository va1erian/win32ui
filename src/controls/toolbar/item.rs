#![forbid(unsafe_code)]

//! One item of a [`Toolbar`](super::Toolbar): a button, a separator or a
//! spacer, built with the chaining methods.

use crate::accel::Shortcut;
use crate::controls::toolbar_icon::ToolbarIcon;

/// A typed identifier for a toolbar item, so state can be updated without
/// naming a raw control id. An item without an explicit id is addressed by its
/// position.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ToolbarItemId(u64);

impl ToolbarItemId {
    /// The id no item has.
    pub const NONE: ToolbarItemId = ToolbarItemId(u64::MAX);

    /// Creates an id from an app-chosen number.
    pub const fn new(value: u64) -> ToolbarItemId {
        ToolbarItemId(value)
    }

    /// The app-chosen number.
    pub const fn value(self) -> u64 {
        self.0
    }
}

impl From<u32> for ToolbarItemId {
    fn from(value: u32) -> ToolbarItemId {
        ToolbarItemId(value as u64)
    }
}

impl From<u64> for ToolbarItemId {
    fn from(value: u64) -> ToolbarItemId {
        ToolbarItemId(value)
    }
}

impl From<usize> for ToolbarItemId {
    fn from(value: usize) -> ToolbarItemId {
        ToolbarItemId(value as u64)
    }
}

/// How a button shows its icon and label.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LabelMode {
    /// The icon alone, centred.
    IconOnly,
    /// The icon and its label side by side (the default).
    #[default]
    IconText,
    /// The icon above the label.
    TextUnder,
    /// The label appears only while the button is checked; otherwise the icon
    /// alone. The button reserves room for the label either way, so checking it
    /// does not reflow the toolbar.
    TextWhenChecked,
}

/// What an item is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ItemKind {
    /// A clickable button.
    Button,
    /// A thin vertical rule.
    Separator,
    /// A gap; a flexible spacer absorbs the toolbar's leftover width.
    Spacer { flexible: bool },
}

/// One toolbar button, separator or spacer.
pub struct ToolbarItem<M> {
    pub(crate) id: Option<ToolbarItemId>,
    pub(crate) kind: ItemKind,
    pub(crate) label: String,
    pub(crate) icon: Option<ToolbarIcon>,
    pub(crate) tooltip: Option<String>,
    pub(crate) shortcut: Option<Shortcut>,
    pub(crate) label_mode: LabelMode,
    pub(crate) checked: bool,
    pub(crate) enabled: bool,
    pub(crate) toggled: bool,
    pub(crate) on_click: Option<Box<dyn Fn() -> Option<M>>>,
    pub(crate) on_toggle: Option<Box<dyn Fn(bool) -> Option<M>>>,
}

impl<M> ToolbarItem<M> {
    /// A button with a label and no icon.
    pub fn new(label: impl Into<String>) -> ToolbarItem<M> {
        ToolbarItem {
            id: None,
            kind: ItemKind::Button,
            label: label.into(),
            icon: None,
            tooltip: None,
            shortcut: None,
            label_mode: LabelMode::default(),
            checked: false,
            enabled: true,
            toggled: false,
            on_click: None,
            on_toggle: None,
        }
    }

    /// A thin themed rule separating groups of buttons.
    pub fn separator() -> ToolbarItem<M> {
        ToolbarItem {
            id: None,
            kind: ItemKind::Separator,
            label: String::new(),
            icon: None,
            tooltip: None,
            shortcut: None,
            label_mode: LabelMode::default(),
            checked: false,
            enabled: true,
            toggled: false,
            on_click: None,
            on_toggle: None,
        }
    }

    /// A fixed gap between buttons.
    pub fn spacer() -> ToolbarItem<M> {
        ToolbarItem::gap(false)
    }

    /// A gap that absorbs the toolbar's leftover width, pushing the items after
    /// it to the right end.
    pub fn flexible_spacer() -> ToolbarItem<M> {
        ToolbarItem::gap(true)
    }

    fn gap(flexible: bool) -> ToolbarItem<M> {
        ToolbarItem {
            id: None,
            kind: ItemKind::Spacer { flexible },
            label: String::new(),
            icon: None,
            tooltip: None,
            shortcut: None,
            label_mode: LabelMode::default(),
            checked: false,
            enabled: true,
            toggled: false,
            on_click: None,
            on_toggle: None,
        }
    }

    /// Sets the item's typed id, so [`Toolbar::set_enabled`](super::Toolbar::set_enabled)
    /// and [`Toolbar::set_checked`](super::Toolbar::set_checked) can address it.
    pub fn id(mut self, id: impl Into<ToolbarItemId>) -> ToolbarItem<M> {
        self.id = Some(id.into());
        self
    }

    /// Adds a vector [`ToolbarIcon`], drawn anti-aliased at the toolbar's DPI.
    pub fn with_icon(mut self, icon: ToolbarIcon) -> ToolbarItem<M> {
        self.icon = Some(icon);
        self
    }

    /// How the button lays out its icon and label.
    pub fn label_mode(mut self, mode: LabelMode) -> ToolbarItem<M> {
        self.label_mode = mode;
        self
    }

    /// Marks the button as a toggle: a click flips its checked state and raises
    /// [`on_toggle`](ToolbarItem::on_toggle) instead of `on_click`.
    pub fn toggle(mut self) -> ToolbarItem<M> {
        self.toggled = true;
        self
    }

    /// Sets the button's initial checked state.
    pub fn checked(mut self, checked: bool) -> ToolbarItem<M> {
        self.checked = checked;
        self
    }

    /// Sets the button's initial enabled state. A disabled button draws dimmed
    /// and emits nothing.
    pub fn enabled(mut self, enabled: bool) -> ToolbarItem<M> {
        self.enabled = enabled;
        self
    }

    /// Sets the tooltip shown while the pointer rests on this button.
    pub fn tooltip(mut self, text: impl Into<String>) -> ToolbarItem<M> {
        self.tooltip = Some(text.into());
        self
    }

    /// Records the button's shortcut, shown after the tooltip text — the same
    /// [`Shortcut`] display text a menu shows; register the accelerator
    /// yourself (the menu bar does).
    pub fn shortcut(mut self, shortcut: Shortcut) -> ToolbarItem<M> {
        self.shortcut = Some(shortcut);
        self
    }

    /// Maps a click on this button to an app message.
    pub fn on_click(mut self, f: impl Fn() -> Option<M> + 'static) -> ToolbarItem<M> {
        self.on_click = Some(Box::new(f));
        self
    }

    /// Maps a toggle of this button to an app message, receiving the new
    /// checked state. Only called for a button marked with
    /// [`toggle`](ToolbarItem::toggle).
    pub fn on_toggle(mut self, f: impl Fn(bool) -> Option<M> + 'static) -> ToolbarItem<M> {
        self.on_toggle = Some(Box::new(f));
        self
    }

    /// Whether this item is a clickable button (not a separator or spacer).
    pub(crate) fn is_button(&self) -> bool {
        self.kind == ItemKind::Button
    }

    /// The tooltip this button should show: its text, its shortcut, or both.
    pub(crate) fn tooltip_text(&self) -> Option<String> {
        match (&self.tooltip, self.shortcut) {
            (Some(text), Some(shortcut)) => Some(format!("{text} ({shortcut})")),
            (Some(text), None) => Some(text.clone()),
            (None, Some(shortcut)) => Some(shortcut.to_string()),
            (None, None) => None,
        }
    }
}
