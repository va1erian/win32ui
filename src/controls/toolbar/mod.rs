#![forbid(unsafe_code)]

//! An owner-drawn toolbar: a row of buttons, separators and spacers that paint
//! themselves (background, hover/pressed/checked/disabled states, icon and
//! label) and map clicks and toggles to the app's `Msg` through each item's
//! closures.
//!
//! The toolbar is a [`CustomWidget`](crate::CustomWidget) painted with
//! Direct2D, so its [`ToolbarIcon`]s are anti-aliased, DPI-scaled and tinted
//! from the theme. [`Custom`](crate::Custom) owns its child window and routes
//! input and theme changes to it.

mod item;
mod paint;
#[cfg(test)]
mod tests;
mod theme;
mod widget;

use std::rc::Rc;

use crate::app::Ui;
use crate::controls::control::{AsControl, Control};
use crate::controls::custom::Custom;
use crate::error::Result;
use crate::hwnd::Hwnd;
use crate::theme::{Theme, Themed};

pub use item::{LabelMode, ToolbarItem, ToolbarItemId};
pub use theme::ToolbarTheme;

pub(crate) use widget::{ToolbarEvent, ToolbarWidget};

/// An owner-drawn toolbar control.
pub struct Toolbar<M: 'static> {
    custom: Custom<ToolbarWidget<M>, M>,
}

impl<M: 'static> Toolbar<M> {
    /// Creates the toolbar as a child of the window behind `ui`, adopting
    /// `ui`'s theme. Use [`Themed::apply_theme`] for a one-off override.
    pub fn new(ui: &mut Ui<M>, items: Vec<ToolbarItem<M>>) -> Result<Toolbar<M>> {
        let widget = ToolbarWidget::new(items);
        let custom = Custom::new(ui, widget)?;
        let hwnd = custom.control().hwnd();
        let dpi = custom.dpi();
        let shared = custom.widget();

        let width = crate::sys::window::client_rect(hwnd).width();
        update_tooltips(&shared, hwnd, width, dpi);

        // A flexible spacer moves the items after it with the toolbar's width,
        // so the region tools must follow a resize (and a DPI change).
        let resized = Rc::clone(&shared);
        let ui_for_dpi = ui.clone();
        custom.on_resize(move |bounds| {
            update_tooltips(&resized, hwnd, bounds.width(), ui_for_dpi.dpi());
        });

        let custom = custom.on_event(move |event| {
            let state = shared.borrow();
            match event {
                ToolbarEvent::Click(index) => state.items.get(index)?.on_click.as_ref()?(),
                ToolbarEvent::Toggle(index, checked) => {
                    state.items.get(index)?.on_toggle.as_ref()?(checked)
                }
            }
        });
        Ok(Toolbar { custom })
    }

    /// A thin themed rule for grouping toolbar buttons; pass it in the item
    /// list. Shorthand for [`ToolbarItem::separator`].
    pub fn separator() -> ToolbarItem<M> {
        ToolbarItem::separator()
    }

    /// A fixed gap; shorthand for [`ToolbarItem::spacer`].
    pub fn spacer() -> ToolbarItem<M> {
        ToolbarItem::spacer()
    }

    /// A gap that pushes the following items to the right end; shorthand for
    /// [`ToolbarItem::flexible_spacer`].
    pub fn flexible_spacer() -> ToolbarItem<M> {
        ToolbarItem::flexible_spacer()
    }

    /// The toolbar's natural height in device pixels.
    pub fn height(&self) -> i32 {
        self.custom.widget().borrow().height_px(self.custom.dpi())
    }

    /// Enables or disables the item with `id`. A disabled button draws dimmed
    /// and emits nothing; only its own rectangle is repainted. A no-op when no
    /// item has that id.
    pub fn set_enabled(&self, id: impl Into<ToolbarItemId>, enabled: bool) {
        let widget = self.custom.widget();
        let index = {
            let mut state = widget.borrow_mut();
            let Some(index) = state.index_of(id.into()) else {
                return;
            };
            state.set_enabled(index, enabled).then_some(index)
        };
        if let Some(index) = index {
            self.invalidate_item(&widget, index);
        }
    }

    /// Sets the checked state of the item with `id`. Only its own rectangle is
    /// repainted. A no-op when no item has that id.
    pub fn set_checked(&self, id: impl Into<ToolbarItemId>, checked: bool) {
        let widget = self.custom.widget();
        let index = {
            let mut state = widget.borrow_mut();
            let Some(index) = state.index_of(id.into()) else {
                return;
            };
            state.set_checked(index, checked).then_some(index)
        };
        if let Some(index) = index {
            self.invalidate_item(&widget, index);
        }
    }

    /// Repaints the toolbar.
    pub fn invalidate(&self) {
        self.custom.invalidate();
    }

    fn invalidate_item(&self, widget: &Rc<std::cell::RefCell<ToolbarWidget<M>>>, index: usize) {
        let hwnd = self.custom.control().hwnd();
        let width = crate::sys::window::client_rect(hwnd).width();
        let dpi = self.custom.dpi();
        if let Some(rect) = widget.borrow().cell_rect_px(index, width, dpi) {
            self.custom.invalidate_rect(rect);
        }
    }
}

/// Repoints the toolbar's shared tooltip at each button's current rectangle.
fn update_tooltips<M>(
    widget: &Rc<std::cell::RefCell<ToolbarWidget<M>>>,
    hwnd: Hwnd,
    width: i32,
    dpi: u32,
) {
    for (slot, rect, text) in widget.borrow().tooltip_regions(width, dpi) {
        crate::controls::tooltip::set_region_tooltip(hwnd, slot, rect, &text);
    }
}

impl<M: 'static> AsControl for Toolbar<M> {
    fn control(&self) -> &Control {
        self.custom.control()
    }
}

impl<M: 'static> Themed for Toolbar<M> {
    fn apply_theme(&self, theme: &Theme) {
        self.custom.apply_theme(theme);
    }
}
