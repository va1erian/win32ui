#![forbid(unsafe_code)]

//! Tabs layout node: a native tab control whose pages are layout subtrees.
//!
//! [`Tabs`] is a layout node, not a free-standing control: build it with
//! [`tabs!`](crate::tabs), size it (by default it fills its parent) and install
//! it with [`Ui::set_layout`](crate::Ui::set_layout). The node owns a
//! `SysTabControl32` child; the selected page's subtree is laid out into the
//! control's display area (from `TCM_ADJUSTRECT`) and every other page is
//! hidden, so the app never places pages by hand and hidden pages take no
//! space or focus. [`Tabs::on_change`] maps a selection to the app's `Msg`.
//!
//! The native tabs ignore dark mode, so the control is created with
//! `TCS_OWNERDRAWFIXED` and each tab is painted from theme tokens on
//! `WM_DRAWITEM`, including hover, selected and focus states. The same subclass
//! tracks the hot tab and handles `Ctrl+Tab` between pages.

mod host;
mod paint;

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use crate::app::Ui;
use crate::app::core::Core;
use crate::controls::control::Control;
use crate::controls::{next_id, registry};
use crate::gdi::Font;
use crate::geometry::Rect;
use crate::hwnd::Hwnd;
use crate::message::{Message, Notify};
use crate::sys;

use self::host::hover_handler;
use self::paint::paint_tab;
use super::{Content, IntoLayoutItem, LayoutItem, Placed, Sizing, WidgetHandle};

/// State a [`Tabs`] node shares with its native control, its subclass and the
/// themed-children registry.
pub(crate) struct TabsShared {
    hwnd: Cell<Hwnd>,
    selected: Cell<usize>,
    count: Cell<usize>,
    /// The tab currently under the pointer, tracked by the subclass.
    hot: Cell<Option<usize>>,
    titles: RefCell<Vec<String>>,
    /// The type-erased [`Tabs::on_change`] mapper; `Any` so one shared type
    /// serves every app's `Msg`.
    mapper: RefCell<Option<Box<dyn Any>>>,
    /// The bound native control and its resources, created by [`build_tabs`].
    bound: RefCell<Option<Rc<Bound>>>,
}

impl TabsShared {
    fn new() -> TabsShared {
        TabsShared {
            hwnd: Cell::new(Hwnd::NULL),
            selected: Cell::new(0),
            count: Cell::new(0),
            hot: Cell::new(None),
            titles: RefCell::new(Vec::new()),
            mapper: RefCell::new(None),
            bound: RefCell::new(None),
        }
    }

    /// The widget handle the layout tree moves for the tab control.
    fn handle(&self, bound: &Bound) -> WidgetHandle {
        WidgetHandle::new(
            bound.hwnd,
            bound.control.bounds_handle(),
            bound.control.visible_handle(),
        )
    }
}

/// A built tab control plus the resources that must outlive it. Dropping it
/// (with the layout) unregisters the widget, removes the subclass and destroys
/// the child `HWND`.
struct Bound {
    hwnd: Hwnd,
    control: Control,
    font: Font,
    _host: Option<sys::tabs::TabHost>,
}

impl Drop for Bound {
    fn drop(&mut self) {
        registry::unregister_app_events(self.hwnd);
        crate::theme::unregister_themed(self.hwnd);
    }
}

/// A node of the layout tree holding a tab control and its pages.
///
/// Built by [`tabs!`](crate::tabs) / [`Tabs`]; rarely named directly.
#[derive(Clone)]
pub(crate) struct TabsNode {
    pub(crate) shared: Rc<TabsShared>,
    pages: Vec<LayoutItem>,
}

impl TabsNode {
    /// A tab strip is always worth showing, even when a page is empty.
    pub(crate) fn is_visible(&self) -> bool {
        true
    }

    /// How many pages the node holds.
    #[cfg(test)]
    pub(crate) fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Shows or hides every page (used when a tabs node is itself a page).
    pub(crate) fn set_tree_visible(&self, visible: bool) {
        for page in &self.pages {
            page.set_tree_visible(visible);
        }
    }

    /// Positions the tab control in `rect`, then lays the selected page out in
    /// the display area and hides the rest.
    pub(crate) fn compute(&self, rect: Rect, dpi: u32, out: &mut Vec<Placed>) {
        if self.pages.is_empty() {
            return;
        }
        let shared = &self.shared;
        let selected = shared.selected.get().min(self.pages.len() - 1);
        let hwnd = shared.hwnd.get();
        let page_rect = if hwnd.is_alive() {
            sys::tabs::page_rect(hwnd, rect)
        } else {
            rect
        };

        if let Some(bound) = shared.bound.borrow().as_ref() {
            out.push(Placed {
                handle: shared.handle(bound),
                rect,
            });
        }
        for (index, page) in self.pages.iter().enumerate() {
            let visible = index == selected;
            page.set_tree_visible(visible);
            if visible {
                page.compute(page_rect, dpi, out);
            }
        }
    }
}

/// A tabs layout node: a native tab strip whose pages are layout subtrees.
///
/// Build one with [`tabs!`](crate::tabs) or [`Tabs::new`] + [`Tabs::page`], and
/// install it with [`Ui::set_layout`](crate::Ui::set_layout).
pub struct Tabs {
    shared: Rc<TabsShared>,
    pages: Vec<(String, LayoutItem)>,
}

impl Tabs {
    /// An empty tabs node (no pages yet).
    pub fn new() -> Tabs {
        Tabs {
            shared: Rc::new(TabsShared::new()),
            pages: Vec::new(),
        }
    }

    /// Appends a page shown under `title`. `page` is any widget or nested
    /// layout; it is positioned automatically.
    pub fn page(mut self, title: &str, page: impl IntoLayoutItem) -> Tabs {
        self.pages
            .push((title.to_string(), page.into_layout_item()));
        self
    }

    /// Selects the page at `index` initially.
    pub fn selected(self, index: usize) -> Tabs {
        self.shared.selected.set(index);
        self
    }

    /// Maps a selection to an app message. The closure returns `Some(msg)` to
    /// raise it, or `None` to ignore the change (it is still applied).
    pub fn on_change<M: 'static>(self, f: impl Fn(usize) -> Option<M> + 'static) -> Tabs {
        let mapper: Box<dyn Fn(usize) -> Option<M>> = Box::new(f);
        self.shared.mapper.replace(Some(Box::new(mapper)));
        self
    }

    fn node(&self) -> TabsNode {
        let titles: Vec<String> = self.pages.iter().map(|(title, _)| title.clone()).collect();
        self.shared.count.set(titles.len());
        *self.shared.titles.borrow_mut() = titles;
        TabsNode {
            shared: Rc::clone(&self.shared),
            pages: self.pages.iter().map(|(_, page)| page.clone()).collect(),
        }
    }
}

impl Default for Tabs {
    fn default() -> Tabs {
        Tabs::new()
    }
}

impl IntoLayoutItem for &Tabs {
    fn into_layout_item(self) -> LayoutItem {
        LayoutItem {
            content: Content::Tabs(Box::new(self.node())),
            sizing: Sizing::Fill(1),
        }
    }
}

impl IntoLayoutItem for Tabs {
    fn into_layout_item(self) -> LayoutItem {
        (&self).into_layout_item()
    }
}

/// Calls the app's `on_change` mapper, downcasting the erased closure.
fn map<M: 'static>(shared: &TabsShared, index: usize) -> Option<M> {
    let erased = shared.mapper.borrow();
    let mapper = erased
        .as_ref()?
        .downcast_ref::<Box<dyn Fn(usize) -> Option<M>>>()?;
    mapper(index)
}

/// Applies a selection read back from the control: repages, relayouts and, when
/// `emit` is set, raises the app's message.
fn apply<M: 'static>(shared: &Rc<TabsShared>, core: &Weak<Core<M>>, emit: bool) {
    let Some(selected) = sys::tabs::cur_sel(shared.hwnd.get()) else {
        return;
    };
    shared.hot.set(None);
    shared.selected.set(selected);
    let Some(core) = core.upgrade() else {
        return;
    };
    let ui = Ui::new(core);
    ui.relayout();
    if emit && let Some(msg) = map::<M>(shared, selected) {
        ui.emit(msg);
    }
}

/// Creates and binds the native tab control for `node`.
///
/// Called by [`Core`](crate::app::Core) when a layout is installed; it is a
/// no-op if the node already has a live control.
pub(crate) fn build_tabs<M: 'static>(ui: &Ui<M>, node: &TabsNode) {
    let shared = Rc::clone(&node.shared);
    if shared.hwnd.get().is_alive() || node.pages.is_empty() {
        return;
    }
    shared.count.set(node.pages.len());
    let parent = ui.hwnd();
    let dpi = ui.dpi();
    let Ok(hwnd) = sys::tabs::create(parent, next_id(), Rect::default()) else {
        return;
    };
    let Ok(font) = Font::system_ui(dpi) else {
        sys::window::destroy(hwnd);
        return;
    };
    sys::control::apply_ui_font(hwnd, dpi);
    // The tabs themselves are owner-drawn, but `DarkMode_Explorer` darkens the
    // display-area frame the control still paints itself.
    sys::apply_native_theme(hwnd, sys::NativeControlKind::Scrollable, ui.theme().is_dark);
    let titles = shared.titles.borrow().clone();
    for (index, title) in titles.iter().enumerate() {
        sys::tabs::insert_item(hwnd, index as i32, title);
    }
    sys::tabs::fit_items(hwnd, font.raw(), &titles);
    let count = node.pages.len();
    sys::tabs::set_cur_sel(hwnd, shared.selected.get().min(count - 1));
    // The tab strip was created after the page widgets, so it starts on top;
    // push it behind them so the selected page shows through.
    sys::window::send_to_back(hwnd);

    let core = ui.core_weak();
    let host =
        sys::tabs::TabHost::install(hwnd, hover_handler(Rc::downgrade(&shared), core.clone()));
    let control = Control::own(hwnd, Rect::default());
    *shared.bound.borrow_mut() = Some(Rc::new(Bound {
        hwnd,
        control,
        font,
        _host: host,
    }));
    shared.hwnd.set(hwnd);

    let weak = Rc::downgrade(&shared);
    let core_for_events = core.clone();
    let mapper: Rc<dyn Fn(&Message) -> bool> = Rc::new(move |message| {
        let Some(shared) = weak.upgrade() else {
            return false;
        };
        match message {
            Message::Notify(Notify::Other {
                code, hwnd: from, ..
            }) if *from == hwnd && *code == sys::tabs::sel_change_code() => {
                apply(&shared, &core_for_events, true);
                true
            }
            Message::DrawItem {
                control,
                item,
                state,
                dc,
                area,
                ..
            } if *control == hwnd => {
                paint_tab(&shared, &core_for_events, *item, Some(*state), *dc, *area);
                true
            }
            _ => false,
        }
    });
    registry::register_app_events(hwnd, mapper);

    let weak = Rc::downgrade(&shared);
    crate::theme::register_themed(
        parent,
        hwnd,
        Rc::new(move |applied| {
            if let Some(shared) = weak.upgrade() {
                let hwnd = shared.hwnd.get();
                sys::apply_native_theme(hwnd, sys::NativeControlKind::Scrollable, applied.is_dark);
                sys::window::invalidate(hwnd);
            }
        }),
    );
}
