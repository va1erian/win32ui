#![forbid(unsafe_code)]

//! A native `CBS_DROPDOWNLIST` combo box that holds typed values.
//!
//! The control stores only the labels; the application's typed values live in a
//! [`ComboBoxItems`] list, and selection is reported as an index into it. The
//! widget layer maps a `CBN_SELCHANGE` back to the app's `Msg` through the
//! closure given to [`ComboBox::on_select`], so the app never sees indices.
//!
//! ```no_run
//! # use win32ui::prelude::*;
//! # enum Msg { Picked(u32) }
//! # fn build(ui: &mut Ui<Msg>) -> win32ui::Result<()> {
//! ComboBox::new(ui, [("Small", 1u32), ("Large", 2)])?
//!     .select(&2)
//!     .on_select(|size| Some(Msg::Picked(*size)));
//! # Ok(())
//! # }
//! ```

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::Ui;
use crate::controls::combobox_events::ComboBoxEvents;
use crate::controls::combobox_model::ComboBoxItems;
use crate::controls::control::{AsControl, Control};
use crate::controls::{create_child, next_id, style};
use crate::error::Result;
use crate::geometry::Rect;
use crate::message::Message;
use crate::sys;
use crate::theme::{Theme, Themed};

// `CBS_*` style bits, from `WinUser.h`.
const CBS_DROPDOWNLIST: u32 = 0x0003;
const CBS_HASSTRINGS: u32 = 0x0200;

/// The default number of items shown when the list drops down. Set with
/// `CB_SETMINVISIBLE`, so the height follows the item font at any DPI.
const DEFAULT_VISIBLE_ITEMS: usize = 8;

/// A drop-down list of typed values.
///
/// Build it with [`ComboBox::new`], optionally select a starting value with
/// [`ComboBox::select`], and map changes through [`ComboBox::on_select`].
pub struct ComboBox<T, M> {
    control: Control,
    items: Rc<ComboBoxItems<T>>,
    events: Rc<RefCell<ComboBoxEvents<T, M>>>,
    sink: Ui<M>,
}

impl<T: 'static, M: 'static> ComboBox<T, M> {
    /// Creates the combo box as a child of the window behind `ui`, adopting
    /// `ui`'s theme. `items` are `(label, value)` pairs, added in order.
    pub fn new<S, I>(ui: &mut Ui<M>, items: I) -> Result<ComboBox<T, M>>
    where
        S: Into<String>,
        I: IntoIterator<Item = (S, T)>,
    {
        let window = ui.hwnd();
        let style = style::WS_CHILD
            | style::WS_VISIBLE
            | style::WS_TABSTOP
            | style::WS_VSCROLL
            | CBS_DROPDOWNLIST
            | CBS_HASSTRINGS;
        let hwnd = create_child(
            "ComboBox",
            "COMBOBOX",
            window,
            style,
            0,
            next_id(),
            Rect::default(),
        )?;

        // `create_child` gave the combo the UI font, so the drop-down height
        // and the selected field scale with DPI. `CB_SETMINVISIBLE` then keeps
        // the dropped list a sensible number of rows tall at any scale.
        sys::combobox::cb_set_min_visible(hwnd, DEFAULT_VISIBLE_ITEMS);
        sys::apply_native_theme(hwnd, sys::NativeControlKind::ComboBox, ui.theme().is_dark);

        // The closed field's height comes from the font, so a layout that does
        // not size the widget still gets a sensible natural height.
        let bounds = Rect::new(0, 0, 0, sys::combobox::cb_item_height(hwnd, -1).max(1));
        sys::window::move_window(hwnd, bounds);

        let items = Rc::new(
            items
                .into_iter()
                .map(|(label, value)| (label.into(), value))
                .collect(),
        );
        let combo = ComboBox {
            control: Control::own(hwnd, bounds),
            items,
            events: Rc::new(RefCell::new(ComboBoxEvents::new())),
            sink: ui.clone(),
        };
        combo.sync_items();
        combo.register_mapper();

        crate::theme::register_themed(
            window,
            hwnd,
            Rc::new(move |applied| {
                sys::apply_native_theme(hwnd, sys::NativeControlKind::ComboBox, applied.is_dark);
                sys::window::invalidate(hwnd);
            }),
        );

        Ok(combo)
    }

    /// Maps a selection change to a message, given the selected typed value.
    pub fn on_select(self, f: impl Fn(&T) -> Option<M> + 'static) -> ComboBox<T, M> {
        self.events.borrow_mut().on_select = Some(Box::new(f));
        self
    }

    /// Selects the item whose value matches `value` (by `PartialEq`), or clears
    /// the selection when no item matches. Chainable.
    pub fn select(self, value: &T) -> ComboBox<T, M>
    where
        T: PartialEq,
    {
        self.set_selected(value);
        self
    }

    /// Selects the item whose value matches `value`, or clears the selection
    /// when no item matches.
    pub fn set_selected(&self, value: &T)
    where
        T: PartialEq,
    {
        let index = self.items.position(value);
        sys::combobox::cb_set_cur_sel(self.control.hwnd(), index);
    }

    /// The selected value, if any.
    pub fn selected(&self) -> Option<&T> {
        let index = sys::combobox::cb_get_cur_sel(self.control.hwnd())?;
        self.items.value(index)
    }

    /// The selected item's index, if any.
    pub fn selected_index(&self) -> Option<usize> {
        sys::combobox::cb_get_cur_sel(self.control.hwnd())
    }

    /// Replaces the items, keeping the current selection when its value is
    /// still present.
    pub fn set_items<S, I>(&mut self, items: I)
    where
        S: Into<String>,
        T: PartialEq,
        I: IntoIterator<Item = (S, T)>,
    {
        let new_items: ComboBoxItems<T> = items
            .into_iter()
            .map(|(label, value)| (label.into(), value))
            .collect();
        // Preserve the selected value across the swap, not its old index.
        let keep = sys::combobox::cb_get_cur_sel(self.control.hwnd())
            .and_then(|index| self.items.value(index))
            .and_then(|value| new_items.position(value));
        self.items = Rc::new(new_items);
        self.sync_items();
        sys::combobox::cb_set_cur_sel(self.control.hwnd(), keep);
        self.register_mapper();
    }

    /// The number of items.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether the combo box has no items.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The label at `index`, if it is in range.
    pub fn label(&self, index: usize) -> Option<&str> {
        self.items.label(index)
    }

    /// Reads back an item's label from the native control, re-entering its
    /// UTF-16 storage (used for tests and accessibility).
    pub fn item_text(&self, index: usize) -> String {
        sys::combobox::cb_item_text(self.control.hwnd(), index)
    }

    /// Opens or closes the dropped list.
    pub fn show_drop_down(&self, show: bool) {
        sys::combobox::cb_show_drop_down(self.control.hwnd(), show);
    }

    /// Rewrites the native list from [`ComboBox::items`], preserving order.
    fn sync_items(&self) {
        let hwnd = self.control.hwnd();
        sys::combobox::cb_reset_content(hwnd);
        for label in self.items.labels() {
            sys::combobox::cb_add_string(hwnd, label);
        }
    }

    /// Registers the `WM_COMMAND` mapper that turns a `CBN_SELCHANGE` into the
    /// app's `Msg`. The mapper reads `items` lazily, so it must be re-registered
    /// after [`ComboBox::set_items`] swaps the list.
    fn register_mapper(&self) {
        let hwnd = self.control.hwnd();
        let items = Rc::clone(&self.items);
        let events = Rc::clone(&self.events);
        let sink = self.sink.clone();
        let mapper: Rc<dyn Fn(&Message) -> bool> = Rc::new(move |message| {
            let mapped = {
                let selected = sys::combobox::cb_get_cur_sel(hwnd);
                crate::controls::combobox_events::selection_message(
                    message,
                    hwnd,
                    selected,
                    &items,
                    &events.borrow(),
                )
            };
            match mapped {
                None => false,
                Some(msg) => {
                    if let Some(msg) = msg {
                        sink.emit(msg);
                    }
                    true
                }
            }
        });
        crate::controls::registry::register_app_events(hwnd, mapper);
    }
}

impl<T, M> AsControl for ComboBox<T, M> {
    fn control(&self) -> &Control {
        &self.control
    }
}

impl<T, M> Themed for ComboBox<T, M> {
    fn apply_theme(&self, theme: &Theme) {
        // The system paints the closed field and the dropped list through the
        // `DarkMode_CFD` visual style: that common-file-dialog/combo theme is
        // the one that darkens both. `DarkMode_Explorer` (used by the other
        // controls) left the field bright white in the dark theme (#67), and
        // the central `WM_CTLCOLOR*` path only covers statics, edits and list
        // boxes, not the combo's own field.
        sys::apply_native_theme(
            self.control.hwnd(),
            sys::NativeControlKind::ComboBox,
            theme.is_dark,
        );
        sys::window::invalidate(self.control.hwnd());
    }
}

impl<T, M> Drop for ComboBox<T, M> {
    fn drop(&mut self) {
        crate::controls::registry::unregister_app_events(self.control.hwnd());
        crate::theme::unregister_themed(self.control.hwnd());
    }
}
