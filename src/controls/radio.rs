#![forbid(unsafe_code)]

//! A typed radio group: one value per button, selected by value rather than
//! by index.
//!
//! The native radio text ignores dark mode (it stays `COLOR_BTNTEXT` black),
//! so each option is an owner-drawn (`BS_OWNERDRAW`) auto radio: the ring, dot
//! and label are painted from theme tokens on `WM_DRAWITEM` (a documented
//! API), while the native button behaviour (arrow-key exclusivity, Space,
//! `BN_CLICKED`) is kept.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::app::{Layout, Ui};
use crate::controls::control::{AsControl, Control, ControlExt, HasText};
use crate::controls::registry;
use crate::controls::{create_child, next_id, style};
use crate::error::{Error, Result};
use crate::gdi::Font;
use crate::geometry::Rect;
use crate::hwnd::Hwnd;
use crate::message::{CommandNotification, Message};
use crate::sys;
use crate::theme::{Theme, Themed};
use crate::units::dip;

/// One owner-drawn radio button owned by a [`RadioGroup`].
///
/// It is a widget in its own right (`AsControl`, so it takes part in layouts),
/// but selection is managed by the group: clicking one button checks it and
/// unchecks the rest, and the group maps the click to the button's value.
pub struct RadioOption {
    control: Control,
    label: Rc<RefCell<String>>,
    theme: Rc<Cell<Theme>>,
    font: Rc<Font>,
}

impl RadioOption {
    /// Creates the native button: an owner-drawn auto radio whose paint and
    /// theme handles are shared with the group's `WM_DRAWITEM` mapper.
    fn create(
        parent: Hwnd,
        bounds: Rect,
        label: &str,
        first: bool,
        font: Rc<Font>,
        theme: &Theme,
    ) -> Result<RadioOption> {
        let style = style::WS_CHILD
            | style::WS_VISIBLE
            | style::WS_TABSTOP
            | sys::button_draw::owner_drawn(sys::button::radio_style(first));
        let hwnd = create_child("Radio", "BUTTON", parent, style, 0, next_id(), bounds)?;
        let _ = sys::window::set_title(hwnd, label);
        let theme_cell = Rc::new(Cell::new(*theme));
        let theme_for_callback = Rc::clone(&theme_cell);
        crate::theme::register_themed(
            parent,
            hwnd,
            Rc::new(move |applied| {
                theme_for_callback.set(*applied);
                sys::window::invalidate(hwnd);
            }),
        );
        Ok(RadioOption {
            control: Control::own(hwnd, bounds),
            label: Rc::new(RefCell::new(label.to_string())),
            theme: theme_cell,
            font,
        })
    }
}

impl AsControl for RadioOption {
    fn control(&self) -> &Control {
        &self.control
    }
}

impl Themed for RadioOption {
    fn apply_theme(&self, theme: &Theme) {
        self.theme.set(*theme);
        sys::window::invalidate(self.control.hwnd());
    }
}

impl Drop for RadioOption {
    fn drop(&mut self) {
        registry::unregister_app_events(self.control.hwnd());
        crate::theme::unregister_themed(self.control.hwnd());
    }
}

impl HasText for RadioOption {
    fn text(&self) -> String {
        self.label.borrow().clone()
    }

    fn set_text(&self, text: &str) {
        self.label.replace(text.to_string());
        let _ = sys::window::set_title(self.control.hwnd(), text);
        sys::window::invalidate(self.control.hwnd());
    }
}

/// Maps a radio selection to an optional app message.
type SelectMapper<T, M> = Box<dyn Fn(&T) -> Option<M>>;

/// State shared between a [`RadioGroup`] and its buttons' mappers.
struct RadioShared<T, M> {
    hwnds: Vec<Hwnd>,
    values: Vec<T>,
    selected: Cell<Option<usize>>,
    on_select: RefCell<Option<SelectMapper<T, M>>>,
}

impl<T, M: 'static> RadioShared<T, M> {
    /// Records `index` as selected, checks exactly that button and repaints
    /// the group. (`BM_SETCHECK` alone does not repaint an owner-drawn
    /// button.)
    fn select(&self, index: usize) {
        self.selected.set(Some(index));
        for (i, hwnd) in self.hwnds.iter().enumerate() {
            sys::button::set_checked(*hwnd, i == index);
            sys::window::invalidate(*hwnd);
        }
    }
}

/// A row of owner-drawn auto radio buttons carrying typed values.
///
/// The group owns one button per value; selection is held by value, not by
/// index. Place the buttons with [`RadioGroup::layout`], which stacks them as
/// a nested column for a layout tree.
///
/// A group owns several `HWND`s, so it cannot expose a single [`Control`]:
/// group-wide `set_enabled`/`set_visible` iterate over the buttons instead of
/// going through [`ControlExt`].
pub struct RadioGroup<T, M> {
    options: Vec<RadioOption>,
    shared: Rc<RadioShared<T, M>>,
}

impl<T: 'static, M: 'static> RadioGroup<T, M> {
    /// Creates one radio button per `(label, value)` pair as a child of the
    /// window behind `ui`, adopting `ui`'s theme. The first value starts
    /// selected; use [`RadioGroup::selected`] for another initial value.
    pub fn new<L>(
        ui: &mut Ui<M>,
        options: impl IntoIterator<Item = (L, T)>,
    ) -> Result<RadioGroup<T, M>>
    where
        L: Into<String>,
    {
        let collected: Vec<(String, T)> = options
            .into_iter()
            .map(|(label, value)| (label.into(), value))
            .collect();
        if collected.is_empty() {
            return Err(Error::CreateControl("RadioGroup"));
        }
        let dpi = ui.dpi();
        let parent = ui.hwnd();
        let theme = ui.theme();
        let font = Font::shared_ui(dpi)?;
        let widest = collected
            .iter()
            .map(|(label, _)| sys::gdi::measure_text(font.raw(), label).width)
            .max()
            .unwrap_or(0);
        let width = (widest + dip(28.0).to_px(dpi).value()).max(dip(64.0).to_px(dpi).value());
        let row =
            (font.pixel_height() + dip(10.0).to_px(dpi).value()).max(dip(20.0).to_px(dpi).value());
        let gap = dip(2.0).to_px(dpi).value();

        let mut buttons = Vec::with_capacity(collected.len());
        for (index, (label, _)) in collected.iter().enumerate() {
            let top = index as i32 * (row + gap);
            let bounds = Rect::new(0, top, width, top + row);
            buttons.push(RadioOption::create(
                parent,
                bounds,
                label,
                index == 0,
                Rc::clone(&font),
                &theme,
            )?);
        }

        let shared = Rc::new(RadioShared {
            hwnds: buttons.iter().map(|button| button.control.hwnd()).collect(),
            values: collected.into_iter().map(|(_, value)| value).collect(),
            selected: Cell::new(None),
            on_select: RefCell::new(None),
        });
        // One mapper per button handles both its click (`WM_COMMAND`) and
        // its paint (`WM_DRAWITEM`).
        for (index, button) in buttons.iter().enumerate() {
            let hwnd = button.control.hwnd();
            let shared_for_mapper = Rc::clone(&shared);
            let option_for_mapper = RadioMapperOption {
                hwnd,
                label: Rc::clone(&button.label),
                theme: Rc::clone(&button.theme),
                font: Rc::clone(&button.font),
            };
            let sink = ui.clone();
            let mapper: Rc<dyn Fn(&Message) -> bool> = Rc::new(move |message| match message {
                Message::Command(command)
                    if command.control == Some(hwnd)
                        && command.notification == CommandNotification::Clicked =>
                {
                    shared_for_mapper.select(index);
                    let msg = shared_for_mapper
                        .on_select
                        .borrow()
                        .as_ref()
                        .and_then(|f| f(&shared_for_mapper.values[index]));
                    if let Some(msg) = msg {
                        sink.emit(msg);
                    }
                    true
                }
                Message::DrawItem {
                    control,
                    dc,
                    state,
                    area,
                    ..
                } if *control == hwnd => {
                    option_for_mapper.draw(
                        *dc,
                        *area,
                        *state,
                        shared_for_mapper.selected.get() == Some(index),
                    );
                    true
                }
                _ => false,
            });
            registry::register_app_events(hwnd, mapper);
        }

        let group = RadioGroup {
            options: buttons,
            shared,
        };
        group.shared.select(0);
        Ok(group)
    }

    /// Selects `value` initially, returning the group for chaining.
    pub fn selected(self, value: T) -> Self
    where
        T: PartialEq,
    {
        self.set_selected(&value);
        self
    }

    /// Maps a selection change to an app message, receiving the new value.
    pub fn on_select(self, f: impl Fn(&T) -> Option<M> + 'static) -> RadioGroup<T, M> {
        self.shared.on_select.replace(Some(Box::new(f)));
        self
    }

    /// The group's buttons, in creation order.
    pub fn options(&self) -> &[RadioOption] {
        &self.options
    }

    /// The buttons as a nested column layout for a layout tree.
    pub fn layout(&self) -> Layout {
        let mut layout = Layout::column().spacing(dip(2.0));
        for option in &self.options {
            layout = layout.item(option);
        }
        layout
    }

    /// The selected value, cloned.
    pub fn selected_value(&self) -> Option<T>
    where
        T: Clone,
    {
        self.shared
            .selected
            .get()
            .and_then(|index| self.shared.values.get(index).cloned())
    }

    /// The selected index, if any.
    pub fn selected_index(&self) -> Option<usize> {
        self.shared.selected.get()
    }

    /// Selects the button carrying `value`. Returns whether it was found.
    pub fn set_selected(&self, value: &T) -> bool
    where
        T: PartialEq,
    {
        let Some(index) = self.shared.values.iter().position(|known| known == value) else {
            return false;
        };
        self.shared.select(index);
        true
    }

    /// Selects the button at `index`. Returns whether it existed.
    pub fn set_selected_index(&self, index: usize) -> bool {
        if index >= self.shared.values.len() {
            return false;
        }
        self.shared.select(index);
        true
    }

    /// Simulates a user click on the button at `index`.
    pub fn click(&self, index: usize) -> bool {
        let Some(hwnd) = self.shared.hwnds.get(index) else {
            return false;
        };
        sys::button::click(*hwnd);
        true
    }

    /// Enables or disables every button in the group.
    pub fn set_enabled(&self, enabled: bool) {
        for option in &self.options {
            option.set_enabled(enabled);
        }
    }

    /// Shows or hides every button in the group.
    pub fn set_visible(&self, visible: bool) {
        for option in &self.options {
            option.set_visible(visible);
        }
    }

    /// Gives the selected button (or the first) keyboard focus.
    pub fn focus(&self) {
        let index = self.shared.selected.get().unwrap_or(0);
        if let Some(option) = self.options.get(index) {
            option.focus();
        }
    }
}

/// The paint handles one `WM_DRAWITEM` mapper needs. `RadioOption` itself is
/// not `Clone`, so the mapper holds these shared handles instead.
struct RadioMapperOption {
    hwnd: Hwnd,
    label: Rc<RefCell<String>>,
    theme: Rc<Cell<Theme>>,
    font: Rc<Font>,
}

impl RadioMapperOption {
    fn draw(&self, dc: isize, area: Rect, state: u32, checked: bool) {
        let theme = self.theme.get();
        let paint = sys::button_draw::RadioPaint {
            text: theme.text,
            text_disabled: theme.text_disabled,
            edge: theme.text_secondary,
            dot: theme.accent,
            focus: theme.border_focused,
            background: theme.background,
        };
        sys::button_draw::draw_radio(
            dc,
            area,
            &self.label.borrow(),
            sys::control::current_font(self.hwnd).unwrap_or(self.font.raw()),
            checked,
            state,
            &paint,
        );
    }
}
