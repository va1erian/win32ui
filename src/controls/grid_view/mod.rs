#![forbid(unsafe_code)]

//! A virtualized, themed grid of tiles over a typed [`GridModel`] — a wrapped
//! image grid (cover art, thumbnails, …), not a report list.
//!
//! There is no native tiled-image-grid control, so [`GridView`] is a
//! [`CustomWidget`](crate::CustomWidget) (see `custom.rs`) hosted with
//! [`Custom::with_vscroll`](crate::Custom::with_vscroll): the widget window is
//! the *viewport* size, the scroll host translates the canvas by the scroll
//! offset, and [`GridWidget`](widget::GridWidget) paints only the tiles that
//! intersect the invalidated rectangle each `WM_PAINT` gives it. The window
//! never grows to the full document, so a Direct2D target stays viewport-sized
//! and off-screen tiles are spared both the paint and the [`GridModel::get`].
//!
//! ```rust
//! use win32ui::gdi::Canvas;
//! use win32ui::prelude::*;
//!
//! struct Album {
//!     title: String,
//! }
//!
//! enum Msg {
//!     AlbumSelected(usize),
//!     OpenAlbum(usize),
//! }
//!
//! fn build(ui: &mut Ui<Msg>, albums: Vec<Album>) -> win32ui::Result<GridView<Album, Msg>> {
//!     let grid = GridView::<Album, Msg>::new(ui)?
//!         .tile_size(dip(148.0))
//!         .content(|album: &Album, canvas: &Canvas, rect: Rect, state: TileState| {
//!             let color = if state.selected {
//!                 Color::rgb(80, 120, 200)
//!             } else {
//!                 Color::rgb(60, 60, 60)
//!             };
//!             canvas.fill_rect(rect, color);
//!             let _ = &album.title;
//!         })
//!         .on_select(|index| Some(Msg::AlbumSelected(index)))
//!         .on_activate(|index| Some(Msg::OpenAlbum(index)));
//!     grid.set_model(albums);
//!     Ok(grid)
//! }
//! ```

mod layout;
mod model;
mod theme;
mod widget;

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::Ui;
use crate::controls::control::{AsControl, Control};
use crate::controls::custom::Custom;
use crate::d2d::{D2dCanvas, RectF};
use crate::error::Result;
use crate::geometry::Rect;
use crate::theme::{Theme, Themed};
use crate::units::{Dip, Px, dip};

pub use self::model::{GridModel, TileSizeSpec, TileState};
pub use self::theme::GridViewTheme;

use self::widget::{ContentFn, D2dContentFn, GridEvent, GridWidget};

/// The default tile size, used until [`GridView::tile_size`] is called.
const DEFAULT_TILE_DIP: f32 = 148.0;
/// The gap between tiles, in both directions.
const SPACING_DIP: f32 = 8.0;

type EventMapper<M> = RefCell<Option<Box<dyn Fn(usize) -> Option<M>>>>;

/// The app's closures for the grid's two events.
struct Handlers<M> {
    select: EventMapper<M>,
    activate: EventMapper<M>,
}

impl<M> Handlers<M> {
    fn map(&self, event: GridEvent) -> Option<M> {
        let (mapper, index) = match event {
            GridEvent::Select(index) => (&self.select, index),
            GridEvent::Activate(index) => (&self.activate, index),
        };
        mapper.borrow().as_ref().and_then(|map| map(index))
    }
}

/// A virtualized grid of tiles over rows of type `T`, mapping its events to
/// the app's `Msg`.
///
/// Build it with [`GridView::new`] and the chaining setters, give it a
/// [`GridModel`] with [`GridView::set_model`], and place it in the layout
/// tree.
pub struct GridView<T: 'static, M: 'static> {
    custom: Custom<GridWidget<T>, M>,
    handlers: Rc<Handlers<M>>,
}

impl<T: 'static, M: 'static> GridView<T, M> {
    /// Creates an empty grid as a child of the window behind `ui`, adopting
    /// `ui`'s theme, at the default tile size (148 DIP). No `content` and no
    /// model yet — the builders below shape it before it is placed in the
    /// layout.
    pub fn new(ui: &mut Ui<M>) -> Result<GridView<T, M>> {
        let dpi = ui.dpi();
        let tile_px = dip(DEFAULT_TILE_DIP).to_px(dpi).value();
        let spacing_px = dip(SPACING_DIP).to_px(dpi).value();

        let handlers = Rc::new(Handlers {
            select: RefCell::new(None),
            activate: RefCell::new(None),
        });
        let mapper = Rc::clone(&handlers);
        let widget = GridWidget::new(tile_px, None, spacing_px);
        let custom = Custom::new(ui, widget)?
            .on_event(move |event| mapper.map(event))
            .with_vscroll();
        let widget_handle = custom.widget();
        let scroll = custom.scroll_handle();
        let dpi = custom.dpi();

        // The layout resizes the viewport on every window resize, which fires
        // this. Recompute the extent from the new width so the scrollbar range
        // never goes stale between `set_model`/`set_tile_size` calls.
        custom.on_resize(move |bounds| {
            let widget = widget_handle.borrow();
            let columns = widget.columns(bounds.width()).max(1);
            let height =
                layout::content_height_px(widget.len(), columns, widget.tile_px(), spacing_px);
            drop(widget);
            if let Some(scroll) = &scroll {
                scroll.set_content_height(Px(height.max(0)).to_dip(dpi), dpi);
            }
        });

        Ok(GridView { custom, handlers })
    }

    /// Sets the (square) tile size: a fixed [`Dip`], or a `Dip` range (e.g.
    /// `dip(96.0)..dip(220.0)`) the app can vary live with
    /// [`GridView::set_tile_size`] — for a tile-size slider like the one in
    /// `examples/demo`.
    pub fn tile_size(self, spec: impl Into<TileSizeSpec>) -> GridView<T, M> {
        let spec = spec.into();
        let dpi = self.custom.dpi();
        let range_px = spec
            .range
            .map(|(min, max)| (min.to_px(dpi).value(), max.to_px(dpi).value()));
        self.with_widget(|widget| widget.set_tile_range_px(range_px));
        self.set_tile_size(spec.initial);
        self
    }

    /// Sets the `content` painter: called for every visible tile with the
    /// item, a [`Canvas`](crate::gdi::Canvas) clipped to the tile's rectangle
    /// and its [`TileState`]. The widget already paints the selection/hover
    /// fill behind it, from theme tokens.
    pub fn content(
        self,
        f: impl Fn(&T, &crate::gdi::Canvas, Rect, TileState) + 'static,
    ) -> GridView<T, M> {
        let content: Rc<ContentFn<T>> = Rc::new(f);
        self.with_widget(|widget| widget.set_content(Rc::clone(&content)));
        self
    }

    /// Sets a Direct2D `content` painter, used instead of [`GridView::content`]:
    /// every visible tile is drawn with anti-aliased shapes and images. The
    /// closure receives the tile's rectangle in device-independent pixels; the
    /// widget still paints the selection/hover fill behind it and only calls it
    /// for tiles that intersect the invalidated region.
    ///
    /// Setting this switches the whole grid to Direct2D, so the GDI `content`
    /// painter is then unused. If neither is set, tiles are blank.
    pub fn content_d2d(
        self,
        f: impl Fn(&T, &mut D2dCanvas<'_>, RectF, TileState) + 'static,
    ) -> GridView<T, M> {
        let content: Rc<D2dContentFn<T>> = Rc::new(f);
        self.with_widget(|widget| widget.set_content_d2d(Rc::clone(&content)));
        self
    }

    /// Maps the selection to a message whenever it changes (click or
    /// keyboard navigation).
    pub fn on_select(self, f: impl Fn(usize) -> Option<M> + 'static) -> GridView<T, M> {
        self.handlers.select.replace(Some(Box::new(f)));
        self
    }

    /// Maps a tile activation (double-click or Enter/Space on the selected
    /// tile) to a message.
    pub fn on_activate(self, f: impl Fn(usize) -> Option<M> + 'static) -> GridView<T, M> {
        self.handlers.activate.replace(Some(Box::new(f)));
        self
    }

    /// Replaces the data source: the tile count is re-read from the model,
    /// the scrollable extent is resynced and the grid repaints. A selection
    /// that is now out of range is cleared, silently.
    pub fn set_model(&self, model: impl GridModel<Item = T> + 'static) {
        self.with_widget(|widget| widget.set_model(model));
        self.resync();
    }

    /// The current (single) selection, if any.
    pub fn selected(&self) -> Option<usize> {
        self.with_widget(GridWidget::selected)
    }

    /// Selects `index` (clamped to the model, `None` clears it) without
    /// raising [`GridView::on_select`].
    pub fn set_selected(&self, index: Option<usize>) {
        self.with_widget(|widget| widget.set_selected(index));
        self.custom.invalidate();
    }

    /// Schedules a repaint of the grid: for when the model's items changed in
    /// place (a tile's image finished loading, say) rather than through
    /// [`GridView::set_model`].
    pub fn invalidate(&self) {
        self.custom.invalidate();
    }

    /// Drops the grid's renderer surface and its uploaded cover images, for a
    /// view being hidden. The next paint recreates the surface, so the caller
    /// must also drop the cover handles its tiles cached (e.g. by rebuilding
    /// the model or clearing each tile's image id).
    pub fn release_renderer(&self) {
        self.custom.release_renderer();
    }

    /// The tile size, in design units.
    pub fn current_tile_size(&self) -> Dip {
        Px(self.with_widget(GridWidget::tile_px)).to_dip(self.custom.dpi())
    }

    /// Sets the tile size, clamped to the range given to
    /// [`GridView::tile_size`] if any. Meant for a live tile-size slider.
    ///
    /// A size that rounds to the same device-pixel tile as the current one is a
    /// no-op, so a slider dragged in sub-pixel steps does not rebuild the whole
    /// grid (and its scroll extent) for every move.
    pub fn set_tile_size(&self, size: Dip) {
        let px = size.to_px(self.custom.dpi()).value();
        let changed = self.with_widget(|widget| {
            let before = widget.tile_px();
            widget.set_tile_px(px);
            widget.tile_px() != before
        });
        if changed {
            self.resync();
        }
    }

    /// Recomputes the scrollable extent from the model, the tile size and the
    /// current viewport width, and repaints.
    fn resync(&self) {
        let width = crate::sys::window::client_rect(self.custom.control().hwnd()).width();
        let spacing = self.spacing_px();
        let height = self.with_widget(|widget| {
            let columns = widget.columns(width).max(1);
            layout::content_height_px(widget.len(), columns, widget.tile_px(), spacing)
        });
        self.custom
            .set_content_height(Px(height.max(0)).to_dip(self.custom.dpi()));
        self.custom.invalidate();
    }

    fn spacing_px(&self) -> i32 {
        dip(SPACING_DIP).to_px(self.custom.dpi()).value()
    }

    /// Borrows the shared widget state for the duration of `f`.
    fn with_widget<R>(&self, f: impl FnOnce(&GridWidget<T>) -> R) -> R {
        let widget = self.custom.widget();
        let widget = widget.borrow();
        f(&widget)
    }
}

impl<T: 'static, M: 'static> AsControl for GridView<T, M> {
    fn control(&self) -> &Control {
        self.custom.control()
    }
}

impl<T: 'static, M: 'static> Themed for GridView<T, M> {
    fn apply_theme(&self, theme: &Theme) {
        self.custom.apply_theme(theme);
    }
}
