#![forbid(unsafe_code)]

//! The acrylic title strip's self-drawn menu bar ([`TitleBarMenu`]).
//!
//! In the strip mode the native `HMENU` bar is not attached: the menu items are
//! painted by us with Direct2D/DirectWrite into the extended strip, where GDI
//! text would write zero alpha and disappear over the DWM material. The popups
//! stay native `TrackPopupMenuEx`, owner-drawn on a dark theme like the native
//! bar's, so a chosen item still reaches [`App::update`](crate::App::update)
//! through the normal queue. A [`TitleBarMenu`] is created only when the strip
//! mode is active; when the material cannot be shown the caller keeps the
//! native bar (see [`Ui::set_menu_bar`](crate::Ui::set_menu_bar)).
//!
//! [`MenuStripPlacement`] chooses whether the menu sits on its own row under the
//! caption ([`Stacked`](MenuStripPlacement::Stacked), the default) or on the
//! caption row itself after the window title
//! ([`Inline`](MenuStripPlacement::Inline), Terminal style).

mod layout;
mod paint;

use std::cell::{Cell, RefCell};

use crate::app::spec::MenuStripPlacement;
use crate::controls::menu::{BarKind, Menu};
use crate::d2d::{Font, FontSpec, Layout, TextSystem};
use crate::geometry::{Point, Rect};
use crate::units::dip;

use layout::{first_enabled, hit, layout_items, mnemonic, without_mnemonics};

/// The stacked menu row's design height, in device-independent pixels.
const ROW_HEIGHT: f32 = 26.0;
/// Menu font size, in device-independent pixels.
const FONT_SIZE: f32 = 12.0;
/// Horizontal padding inside each item.
const PAD: f32 = 10.0;
/// Gap between items.
const GAP: f32 = 2.0;
/// Left margin before the title and the first item.
const MARGIN: f32 = 8.0;
/// Gap between the window title and the first inline menu item.
const TITLE_GAP: f32 = 14.0;

/// The shared menu font, resolved once per UI thread. `None` when DirectWrite
/// is unavailable, in which case the strip menu cannot be built.
fn font() -> Option<Font> {
    thread_local! {
        static FONT: RefCell<Option<Font>> = const { RefCell::new(None) };
    }
    FONT.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            let system = TextSystem::new().ok()?;
            let spec = FontSpec::new("system-ui, Segoe UI, sans-serif", FONT_SIZE);
            *slot = Some(system.font(&spec).ok()?);
        }
        slot.clone()
    })
}

/// One top-level item, flattened from the [`Menu`] at construction so painting
/// and hit-testing never re-read the menu or re-lay-out text.
struct Item {
    /// Display label with mnemonics stripped.
    label: String,
    /// The mnemonic key, lowercased, if the label has one.
    mnemonic: Option<char>,
    /// Whether the item can be chosen.
    enabled: bool,
    /// The laid-out label, drawn as-is.
    layout: Layout,
    /// The label width in device-independent pixels.
    width_dip: f32,
    /// Byte offset of the mnemonic character in `label`, for its underline.
    mnemonic_index: Option<usize>,
}

/// What choosing a bar item does.
pub(crate) enum OpenAction<M> {
    /// Raise the item's message directly.
    Command(M),
    /// Open this submenu as a native popup.
    Popup(Menu<M>),
}

/// A menu bar drawn on the acrylic strip, mapped to the app's `Msg`.
///
/// Built from a [`Menu`] once the strip mode is active; the caller owns it and
/// drives its hover/focus state from the top-level window's input. The top-level
/// window lays its items out ([`TitleBarMenu::relayout`]) and paints them from
/// the stored rectangles ([`TitleBarMenu::paint`]), so hit-testing and drawing
/// always agree.
pub(crate) struct TitleBarMenu<M> {
    /// The menu this strip draws.
    pub(crate) menu: Menu<M>,
    items: Vec<Item>,
    /// The line height of the menu font, in device-independent pixels.
    line_height_dip: f32,
    placement: Cell<MenuStripPlacement>,
    /// The item rectangles in client coordinates (device pixels), from the last
    /// [`TitleBarMenu::relayout`].
    layout: RefCell<Vec<Rect>>,
    /// The laid-out window title, drawn in the caption row because DWM draws
    /// only the caption buttons in an extended frame.
    title: RefCell<Option<Layout>>,
    /// The caption row's height in device pixels, from the last
    /// [`TitleBarMenu::relayout`]; the title is centred in it.
    caption_px: Cell<i32>,
    focus: Cell<Option<usize>>,
    hover: Cell<Option<usize>>,
    pressed: Cell<Option<usize>>,
    active: Cell<bool>,
    cues: Cell<bool>,
}

impl<M: 'static> TitleBarMenu<M> {
    /// Flattens `menu` into drawable items, or `None` when DirectWrite is
    /// unavailable (the caller then keeps the native menu bar).
    pub(crate) fn new(menu: Menu<M>, placement: MenuStripPlacement) -> Option<TitleBarMenu<M>> {
        let font = font()?;
        let line_height_dip = font.metrics().line_height();
        let mut items = Vec::with_capacity(menu.bar_len());
        for index in 0..menu.bar_len() {
            let entry = menu.bar_entry(index)?;
            let label = without_mnemonics(entry.label);
            let mnemonic = mnemonic(entry.label);
            let layout = font.layout(&label, f32::INFINITY).ok()?;
            let mnemonic_index = mnemonic.and_then(|key| label.find(key));
            items.push(Item {
                width_dip: font.width(&label),
                label,
                mnemonic,
                enabled: entry.enabled,
                layout,
                mnemonic_index,
            });
        }
        Some(TitleBarMenu {
            menu,
            items,
            line_height_dip,
            placement: Cell::new(placement),
            layout: RefCell::new(Vec::new()),
            title: RefCell::new(None),
            caption_px: Cell::new(0),
            focus: Cell::new(None),
            hover: Cell::new(None),
            pressed: Cell::new(None),
            active: Cell::new(false),
            cues: Cell::new(false),
        })
    }

    /// The design height of the row this placement adds below the caption:
    /// `ROW_HEIGHT` for a stacked menu, zero for an inline one.
    pub(crate) fn added_row_px(&self, dpi: u32) -> i32 {
        match self.placement.get() {
            MenuStripPlacement::Stacked => dip(ROW_HEIGHT).to_px(dpi).value(),
            MenuStripPlacement::Inline => 0,
        }
    }

    /// Lays the title and items out for the given strip geometry and stores the
    /// rectangles: `caption_px` is the caption row's height. In `Inline` mode
    /// the items follow the title on the caption row; in `Stacked` mode they
    /// get their own row below it. Returns the added menu row height (which the
    /// window extends its frame by).
    pub(crate) fn relayout(&self, dpi: u32, caption_px: i32, title: &str) -> i32 {
        let scale = dpi as f32 / 96.0;
        *self.title.borrow_mut() = font()
            .filter(|_| !title.is_empty())
            .and_then(|font| font.layout(title, f32::INFINITY).ok());
        self.caption_px.set(caption_px);
        let title_dip = self.title.borrow().as_ref().map_or(0.0, Layout::width);

        let added = self.added_row_px(dpi);
        let start_x = dip(MARGIN).to_px(dpi).value();
        let (start_x, y, height) = match self.placement.get() {
            MenuStripPlacement::Stacked => (start_x, caption_px, added.max(1)),
            MenuStripPlacement::Inline => (
                (start_x as f32 + title_dip * scale + dip(TITLE_GAP).to_px(dpi).value() as f32)
                    as i32,
                0,
                caption_px.max(1),
            ),
        };
        let widths: Vec<i32> = self
            .items
            .iter()
            .map(|item| (item.width_dip * scale).round() as i32)
            .collect();
        let rects = layout_items(
            &widths,
            start_x,
            y,
            height,
            dip(PAD).to_px(dpi).value(),
            dip(GAP).to_px(dpi).value(),
        );
        *self.layout.borrow_mut() = rects;
        added
    }

    /// The rectangle of the `index`-th item in client coordinates, from the
    /// last [`TitleBarMenu::relayout`].
    pub(crate) fn item_rect(&self, index: usize) -> Option<Rect> {
        self.layout.borrow().get(index).copied()
    }

    /// Every item rectangle from the last [`TitleBarMenu::relayout`], for the
    /// window's hit-testing store.
    pub(crate) fn layout_rects(&self) -> Vec<Rect> {
        self.layout.borrow().clone()
    }

    /// The index of the item under `point` (client coordinates), if any.
    pub(crate) fn hit(&self, point: Point) -> Option<usize> {
        hit(&self.layout.borrow(), point)
    }

    /// The mnemonic item for `key` (case-insensitive), if any.
    pub(crate) fn mnemonic_index(&self, key: char) -> Option<usize> {
        let key = key.to_ascii_lowercase();
        self.items
            .iter()
            .position(|item| item.mnemonic == Some(key))
    }

    /// The first enabled item, if any.
    pub(crate) fn first_enabled(&self) -> Option<usize> {
        let enabled: Vec<bool> = self.items.iter().map(|item| item.enabled).collect();
        first_enabled(&enabled)
    }

    /// Moves the keyboard focus `step` items (wrapping over enabled items), or
    /// to the first enabled item when nothing is focused.
    pub(crate) fn move_focus(&self, step: i32) {
        let Some(current) = self.focus.get() else {
            self.focus.set(self.first_enabled());
            return;
        };
        let len = self.items.len();
        if len == 0 {
            return;
        }
        let mut index = current as i32;
        for _ in 0..len {
            index = (index + step).rem_euclid(len as i32);
            if self.items[index as usize].enabled {
                self.focus.set(Some(index as usize));
                return;
            }
        }
    }

    /// The currently focused item.
    pub(crate) fn focused(&self) -> Option<usize> {
        self.focus.get()
    }

    /// Sets the focused item.
    pub(crate) fn set_focus(&self, index: Option<usize>) {
        self.focus.set(index);
    }

    /// Sets the hovered item.
    pub(crate) fn set_hover(&self, index: Option<usize>) {
        self.hover.set(index);
    }

    /// Sets the pressed item.
    pub(crate) fn set_pressed(&self, index: Option<usize>) {
        self.pressed.set(index);
    }

    /// Whether keyboard navigation is active (Alt held, or a menu was opened).
    pub(crate) fn is_active(&self) -> bool {
        self.active.get()
    }

    /// Turns keyboard navigation on or off.
    pub(crate) fn set_active(&self, active: bool) {
        self.active.set(active);
        if !active {
            self.focus.set(None);
        }
    }

    /// Shows or hides the mnemonic underlines.
    pub(crate) fn set_cues(&self, cues: bool) {
        self.cues.set(cues);
    }

    /// What choosing the `index`-th item does, or `None` when it is disabled or
    /// out of range.
    pub(crate) fn open(&self, index: usize) -> Option<OpenAction<M>> {
        let entry = self.menu.bar_entry(index)?;
        if !entry.enabled {
            return None;
        }
        match entry.kind {
            BarKind::Command(id) => self
                .menu
                .find_action(id)
                .map(|action| OpenAction::Command(action())),
            BarKind::Submenu(menu) => Some(OpenAction::Popup(menu.clone())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip_menu() -> Option<TitleBarMenu<u8>> {
        let menu = Menu::<u8>::new()
            .item("&File", None, || 1)
            .item("&Edit", None, || 2)
            .separator()
            .disabled_item("&Hidden", None, || 3)
            .submenu("&View", Menu::new().item("Zoom", None, || 4));
        TitleBarMenu::new(menu, MenuStripPlacement::Stacked)
    }

    #[test]
    fn flattens_bar_items_and_finds_mnemonics() {
        let Some(tm) = strip_menu() else {
            return; // DirectWrite unavailable; the caller keeps the native bar.
        };
        assert_eq!(tm.added_row_px(96), dip(ROW_HEIGHT).to_px(96).value());
        assert_eq!(tm.mnemonic_index('f'), Some(0));
        assert_eq!(tm.mnemonic_index('V'), Some(3), "case-insensitive");
        assert_eq!(tm.mnemonic_index('z'), None);
        assert_eq!(tm.first_enabled(), Some(0));
    }

    #[test]
    fn stacked_items_sit_below_the_caption() {
        let Some(tm) = strip_menu() else {
            return;
        };
        tm.relayout(96, 40, "Title");
        let first = tm.item_rect(0).expect("first item");
        assert_eq!(first.top, 40, "below the caption");
        assert_eq!(first.height(), dip(ROW_HEIGHT).to_px(96).value());
        assert_eq!(tm.hit(Point::new(first.left + 1, first.top + 1)), Some(0));
    }

    #[test]
    fn inline_items_sit_on_the_caption_after_the_title() {
        let Some(menu) = maybe_inline() else {
            return;
        };
        let first = menu.item_rect(0).expect("first item");
        assert_eq!(first.top, 0, "on the caption row");
        assert!(
            first.left >= dip(MARGIN).to_px(96).value(),
            "the items start after the title margin"
        );
    }

    fn maybe_inline() -> Option<TitleBarMenu<u8>> {
        let menu = Menu::<u8>::new().item("&File", None, || 1);
        let tm = TitleBarMenu::new(menu, MenuStripPlacement::Inline)?;
        tm.relayout(96, 34, "Title");
        Some(tm)
    }

    #[test]
    fn keyboard_focus_moves_over_enabled_items_and_wraps() {
        let Some(tm) = strip_menu() else {
            return;
        };
        tm.set_active(true);
        tm.set_focus(None);
        tm.move_focus(1);
        assert_eq!(tm.focused(), Some(0), "from none, focus the first enabled");
        // 'Hidden' (index 2) is disabled, so moving forward skips it to 3.
        tm.set_focus(Some(1));
        tm.move_focus(1);
        assert_eq!(tm.focused(), Some(3));
        tm.move_focus(1);
        assert_eq!(tm.focused(), Some(0), "wraps past the end");
        tm.move_focus(-1);
        assert_eq!(tm.focused(), Some(3), "wraps before the start");
        tm.set_active(false);
        assert_eq!(tm.focused(), None, "deactivating clears the focus");
    }

    #[test]
    fn opening_returns_a_command_or_a_submenu() {
        let Some(tm) = strip_menu() else {
            return;
        };
        assert!(matches!(tm.open(0), Some(OpenAction::Command(1))));
        assert!(matches!(tm.open(3), Some(OpenAction::Popup(_))));
        assert!(tm.open(2).is_none(), "a disabled item cannot be chosen");
    }
}
