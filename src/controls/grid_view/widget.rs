#![forbid(unsafe_code)]

//! [`GridWidget`]: the [`CustomWidget`] a [`GridView`](super::GridView) hosts
//! its content in — paints the visible tiles and turns mouse/keyboard input
//! into [`GridEvent`]s.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::controls::custom::{CustomWidget, Input, WidgetCx};
use crate::controls::grid_view::layout::{self, Direction};
use crate::controls::grid_view::model::{GridModel, TileState};
use crate::controls::grid_view::theme::GridViewTheme;
use crate::gdi::Canvas;
use crate::geometry::Rect;
use crate::message::{Key, MouseButton};
use crate::theme::Theme;

/// Draws one tile: the item, a canvas clipped to the tile's rectangle (in the
/// content's own coordinates) and its paint state.
pub(super) type ContentFn<T> = dyn Fn(&T, &Canvas, Rect, TileState);

/// The events [`GridWidget`] raises; mapped to the app's `Msg` through
/// [`Custom::on_event`](crate::Custom::on_event) by [`GridView::new`](super::GridView::new).
pub(super) enum GridEvent {
    /// The selection changed to this index.
    Select(usize),
    /// This tile was activated (double-click or Enter).
    Activate(usize),
}

/// The [`CustomWidget`] behind [`GridView`](super::GridView): virtualized
/// paint over a [`GridModel`], selection and keyboard navigation. The paint
/// colours come straight from the `theme` [`CustomWidget::paint`] is given
/// each frame, via [`GridViewTheme::from_theme`], so no palette is cached here.
pub(super) struct GridWidget<T> {
    model: RefCell<Option<Box<dyn GridModel<Item = T>>>>,
    content: RefCell<Option<Rc<ContentFn<T>>>>,
    tile_px: Cell<i32>,
    tile_range_px: Cell<Option<(i32, i32)>>,
    spacing_px: Cell<i32>,
    selected: Cell<Option<usize>>,
    hovered: Cell<Option<usize>>,
}

impl<T: 'static> GridWidget<T> {
    pub(super) fn new(
        tile_px: i32,
        tile_range_px: Option<(i32, i32)>,
        spacing_px: i32,
    ) -> GridWidget<T> {
        GridWidget {
            model: RefCell::new(None),
            content: RefCell::new(None),
            tile_px: Cell::new(tile_px.max(1)),
            tile_range_px: Cell::new(tile_range_px),
            spacing_px: Cell::new(spacing_px.max(0)),
            selected: Cell::new(None),
            hovered: Cell::new(None),
        }
    }

    pub(super) fn set_content(&self, content: Rc<ContentFn<T>>) {
        self.content.replace(Some(content));
    }

    pub(super) fn set_model(&self, model: impl GridModel<Item = T> + 'static) {
        self.model.replace(Some(Box::new(model)));
        let selected = self.selected.get();
        if selected.is_some_and(|index| index >= self.len()) {
            self.selected.set(None);
        }
    }

    pub(super) fn len(&self) -> usize {
        self.model.borrow().as_ref().map_or(0, |model| model.len())
    }

    pub(super) fn selected(&self) -> Option<usize> {
        self.selected.get()
    }

    pub(super) fn set_selected(&self, index: Option<usize>) {
        self.selected.set(index.filter(|&index| index < self.len()));
    }

    pub(super) fn tile_px(&self) -> i32 {
        self.tile_px.get()
    }

    pub(super) fn set_tile_range_px(&self, range_px: Option<(i32, i32)>) {
        self.tile_range_px.set(range_px);
    }

    pub(super) fn set_tile_px(&self, tile_px: i32) {
        let clamped = match self.tile_range_px.get() {
            Some((min, max)) => tile_px.clamp(min, max),
            None => tile_px,
        };
        self.tile_px.set(clamped.max(1));
    }

    /// Columns that fit `viewport_width`, device pixels.
    pub(super) fn columns(&self, viewport_width: i32) -> usize {
        layout::columns_for_width(viewport_width, self.tile_px.get(), self.spacing_px.get())
    }

    /// The tile index at `(x, y)`, content-relative device pixels.
    fn index_at(&self, x: i32, y: i32, viewport_width: i32) -> Option<usize> {
        layout::index_at_point(
            x,
            y,
            self.tile_px.get(),
            self.spacing_px.get(),
            self.columns(viewport_width),
            self.len(),
        )
    }

    fn navigate(&self, viewport_width: i32, direction: Direction) -> Option<usize> {
        let len = self.len();
        if len == 0 {
            return None;
        }
        let columns = self.columns(viewport_width);
        let current = self.selected.get().unwrap_or(0);
        Some(layout::navigate(len, columns, current, direction))
    }
}

impl<T: 'static> CustomWidget for GridWidget<T> {
    type Event = GridEvent;

    fn paint(&self, canvas: &Canvas, bounds: Rect, theme: &Theme) {
        let grid_theme = GridViewTheme::from_theme(theme);
        canvas.fill_rect(bounds, grid_theme.background);

        let content = self.content.borrow();
        let model = self.model.borrow();
        let (Some(content), Some(model)) = (content.as_ref(), model.as_ref()) else {
            return;
        };
        let len = model.len();
        if len == 0 {
            return;
        }

        let tile = self.tile_px.get();
        let spacing = self.spacing_px.get();
        let columns = self.columns(bounds.width()).max(1);
        let stride = tile + spacing;

        let paint_rect = canvas.paint_rect();
        let items = layout::visible_item_range(
            paint_rect.top,
            (paint_rect.bottom - paint_rect.top).max(0),
            tile,
            spacing,
            columns,
            len,
        );

        let selected = self.selected.get();
        let hovered = self.hovered.get();
        for index in items {
            let Some(item) = model.get(index) else {
                continue;
            };
            let row = (index / columns) as i32;
            let col = (index % columns) as i32;
            let x = col * stride;
            let y = row * stride;
            let rect = Rect::new(x, y, x + tile, y + tile);
            let state = TileState {
                selected: selected == Some(index),
                hovered: hovered == Some(index),
            };
            if state.selected {
                canvas.fill_rect(rect, grid_theme.selection);
            } else if state.hovered {
                canvas.fill_rect(rect, grid_theme.hover);
            }
            content(item, canvas, rect, state);
        }
    }

    fn wants_arrow_keys(&self) -> bool {
        true
    }

    fn input(&self, input: Input, cx: &mut WidgetCx<GridEvent>) {
        let width = cx.bounds().width();
        match input {
            Input::MouseMove { x, y } => {
                let next = self.index_at(x, y, width);
                if self.hovered.replace(next) != next {
                    cx.invalidate();
                }
            }
            Input::MouseLeave => {
                if self.hovered.replace(None).is_some() {
                    cx.invalidate();
                }
            }
            Input::MouseDown {
                x,
                y,
                button: MouseButton::Left,
            } => {
                cx.focus();
                if let Some(index) = self.index_at(x, y, width) {
                    self.selected.set(Some(index));
                    cx.invalidate();
                    cx.emit(GridEvent::Select(index));
                }
            }
            Input::MouseDoubleClick {
                x,
                y,
                button: MouseButton::Left,
            } => {
                if let Some(index) = self.index_at(x, y, width) {
                    cx.emit(GridEvent::Activate(index));
                }
            }
            Input::KeyDown { key, .. } => {
                let direction = match key {
                    Key::LEFT => Direction::Left,
                    Key::RIGHT => Direction::Right,
                    Key::UP => Direction::Up,
                    Key::DOWN => Direction::Down,
                    Key::RETURN | Key::SPACE => {
                        if let Some(index) = self.selected.get() {
                            cx.emit(GridEvent::Activate(index));
                        }
                        return;
                    }
                    _ => return,
                };
                if let Some(index) = self.navigate(width, direction) {
                    self.selected.set(Some(index));
                    cx.invalidate();
                    cx.emit(GridEvent::Select(index));
                }
            }
            _ => {}
        }
    }
}
