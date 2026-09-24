#![forbid(unsafe_code)]

//! Layout binding and relayout for [`Core`](super::Core): install a layout tree,
//! create the child windows node kinds own, and move every leaf into place.

use crate::app::Ui;
use crate::app::layout::split;
use crate::app::layout::tabs;
use crate::app::layout::{Content, Layout, LayoutItem, Placed};
use crate::geometry::Rect;
use crate::hwnd::Hwnd;
use crate::sys;

use super::Core;

impl<M> Core<M> {
    /// Installs the layout tree, binds any split dividers, and lays it out
    /// immediately.
    pub(crate) fn set_layout(&self, layout: Layout, ui: Ui<M>)
    where
        M: 'static,
    {
        self.bind_splits(&layout, &ui);
        *self.layout.borrow_mut() = Some(layout);
        self.relayout();
    }

    /// Creates the divider window for every split node in `layout`.
    fn bind_splits(&self, layout: &Layout, ui: &Ui<M>)
    where
        M: 'static,
    {
        for item in layout.items() {
            self.bind_item(item, ui);
        }
    }

    fn bind_item(&self, item: &LayoutItem, ui: &Ui<M>)
    where
        M: 'static,
    {
        match item.content() {
            Content::Nested(nested) => self.bind_splits(nested, ui),
            Content::Split(node) => {
                if let Some(window) = split::build_divider(ui, node) {
                    self.dividers.borrow_mut().push(window);
                }
            }
            Content::Tabs(node) => tabs::build_tabs(ui, node),
            Content::Widget(_) => {}
        }
    }

    /// Whether a layout tree has been installed.
    pub(crate) fn has_layout(&self) -> bool {
        self.layout.borrow().is_some()
    }

    /// Lays the tree out again at the window's current DPI.
    pub(crate) fn relayout(&self) {
        let hwnd = self.hwnd.get();
        if hwnd.is_null() {
            return;
        }
        self.relayout_at(sys::dpi::window_dpi(hwnd));
    }

    /// Lays the tree out again at `dpi` (used on `WM_DPICHANGED`, where the
    /// message carries the new value).
    pub(crate) fn relayout_with_dpi(&self, dpi: u32) {
        self.relayout_at(dpi);
    }

    fn relayout_at(&self, dpi: u32) {
        let hwnd = self.hwnd.get();
        if hwnd.is_null() {
            return;
        }
        let Some(layout) = self.layout.borrow().clone() else {
            return;
        };

        let client = sys::window::client_rect(hwnd);
        let placed: Vec<Placed> = layout
            .compute(client, dpi)
            .into_iter()
            .filter(|placed| placed.handle.hwnd().is_alive())
            .collect();
        let moves: Vec<(Hwnd, Rect)> = placed
            .iter()
            .map(|placed| (placed.handle.hwnd(), placed.rect))
            .collect();

        // Update the widgets' cached bounds before the batched OS move, so the
        // layout and the widgets agree even if a child handles `WM_SIZE`.
        for placed in &placed {
            placed.handle.set_bounds(placed.rect);
        }
        sys::layout::apply(&moves);
        // Paging (tabs) and hide/show change which children exist in the client
        // area, and a moved child's previous area belongs to the parent; mark
        // the whole tree for repaint so nothing waits for an interaction.
        sys::window::redraw_children(hwnd);
    }
}
