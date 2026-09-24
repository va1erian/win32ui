//! The demo's Grid tab: a virtualized grid of image tiles standing in for
//! cover art, with a live tile-size slider.
//!
//! Tiles are painted through `GridView::content_d2d`: a Direct2D bitmap
//! (`draw_image`) scaled to the tile. Every tile starts as a flat placeholder
//! and switches to its generated cover once the demo's worker tick has
//! "loaded" it, so the placeholder while loading and the image path are both
//! exercised.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use win32ui::d2d::{
    FontSpec, ImageId, Interpolation, Layout as TextLayout, PointF, RectF, Stroke, TextSystem,
};
use win32ui::prelude::*;
use win32ui::{column, row};

use super::Msg;

/// A palette that cycles, so the tiles are visibly distinct without depending
/// on any image-decoding crate.
const PALETTE: [Color; 6] = [
    Color::hex(0xE07A5F),
    Color::hex(0x3D405B),
    Color::hex(0x81B29A),
    Color::hex(0xF2CC8F),
    Color::hex(0x577590),
    Color::hex(0x9B5DE5),
];

/// The generated cover image's edge, in pixels.
const COVER_PX: u32 = 56;
/// How many tiles the loader produces per worker tick.
const LOAD_BATCH: usize = 24;
/// The tile caption's font size, in design units.
const LABEL_DIP: f32 = 12.0;
/// How often, at most, a live tile-size change rebuilds the grid while the
/// slider is dragged. Repainting the whole (tall) tile content is the expensive
/// part; capping it keeps the slider itself responsive, and the final size is
/// applied on release.
const TILE_RESIZE_THROTTLE: Duration = Duration::from_millis(40);

/// One tile's cover: `None` while it is still loading, so the tile paints its
/// placeholder. The Direct2D image handle and the measured caption layout are
/// built once, on first paint, and reused.
struct Cover {
    image: Option<RgbaImage>,
    id: Cell<Option<ImageId>>,
    layout: RefCell<Option<TextLayout>>,
}

/// A stand-in for a cover-art album: a title, a placeholder colour and the
/// shared cover state the loader fills in.
pub(super) struct Album {
    title: String,
    color: Color,
    cover: Rc<RefCell<Cover>>,
}

/// A procedural cover: a two-tone diagonal pattern from the tile's palette
/// colour, with a corner accent that varies by album, so the image tiles are
/// visually distinct without an image-decoding dependency.
fn cover_image(color: Color, index: usize) -> RgbaImage {
    let size = COVER_PX;
    let shade = color.lerp(Color::rgb(0, 0, 0), 0.35);
    let accent = PALETTE[(index + 3) % PALETTE.len()];
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let band = (x + y) / 7;
            let mut pixel = if band % 2 == 0 { color } else { shade };
            if x >= size - 12 && y < 12 {
                pixel = accent;
            }
            pixels.extend_from_slice(&[pixel.r, pixel.g, pixel.b, 0xFF]);
        }
    }
    RgbaImage {
        width: size,
        height: size,
        pixels,
    }
}

/// Builds `count` albums, every one still loading.
fn albums(count: usize) -> (Vec<Album>, Vec<Rc<RefCell<Cover>>>) {
    let mut covers = Vec::with_capacity(count);
    let albums = (0..count)
        .map(|index| {
            let cover = Rc::new(RefCell::new(Cover {
                image: None,
                id: Cell::new(None),
                layout: RefCell::new(None),
            }));
            covers.push(Rc::clone(&cover));
            Album {
                title: format!("Album {}", index + 1),
                color: PALETTE[index % PALETTE.len()],
                cover,
            }
        })
        .collect();
    (albums, covers)
}

/// What the Grid tab tells the app.
pub(super) enum GridMsg {
    /// The selection changed.
    Select(usize),
    /// A tile was activated (double-click or Enter).
    Activate(usize),
    /// The tile-size slider moved.
    TileSize(f64),
    /// The tile-size slider gesture ended; apply the final value.
    TileSizeCommit(f64),
}

pub(super) struct Grid {
    grid: GridView<Album, Msg>,
    caption: Label,
    size_label: Label,
    size_slider: Slider<Msg>,
    /// Every tile's cover, in index order, so the loader can fill them in.
    covers: Vec<Rc<RefCell<Cover>>>,
    /// How many covers have been produced so far.
    loaded: usize,
    /// When the grid last rebuilt for a live tile-size change.
    last_resize: Option<Instant>,
}

impl Grid {
    pub(super) fn build(ui: &mut Ui<Msg>) -> Grid {
        // The DirectWrite font for the tile captions; `None` if Direct2D is
        // unavailable, in which case the labels are simply skipped.
        let font = TextSystem::new()
            .ok()
            .and_then(|system| system.font(&FontSpec::new("system-ui", LABEL_DIP)).ok());

        let grid = GridView::<Album, Msg>::new(ui)
            .expect("grid")
            .tile_size(dip(96.0)..dip(220.0))
            .content_d2d(
                move |album: &Album, canvas, rect: RectF, state: TileState| {
                    let cover = album.cover.borrow();
                    match cover.image.as_ref() {
                        Some(image) => {
                            let id = match cover.id.get() {
                                Some(id) => id,
                                None => {
                                    let id = canvas.image(image);
                                    cover.id.set(Some(id));
                                    id
                                }
                            };
                            canvas.draw_image(id, rect, None, 1.0, Interpolation::Linear);
                        }
                        // Still loading: a flat placeholder in the album colour.
                        None => canvas.fill_rect(rect, album.color),
                    }

                    if state.selected || state.hovered {
                        let border = if state.selected {
                            Color::rgb(255, 255, 255)
                        } else {
                            Color::rgb(200, 200, 200)
                        };
                        canvas.stroke_rect(rect, border, Stroke::solid(1.0));
                    }

                    if let Some(font) = &font {
                        // The caption layout is measured once, at the tile's font
                        // size, and reused for every paint; the clip trims it to
                        // the tile.
                        if cover.layout.borrow().is_none()
                            && let Ok(layout) = font.layout(&album.title, f32::INFINITY)
                        {
                            *cover.layout.borrow_mut() = Some(layout);
                        }
                        if let Some(layout) = cover.layout.borrow().as_ref() {
                            let origin = PointF::new(rect.left + 4.0, rect.bottom - 18.0);
                            canvas.push_clip(rect);
                            canvas.draw_text(layout, origin, text_on(album.color));
                            let _ = canvas.pop_clip();
                        }
                    }
                },
            )
            .on_select(|index| Some(Msg::Grid(GridMsg::Select(index))))
            .on_activate(|index| Some(Msg::Grid(GridMsg::Activate(index))));
        let (model, covers) = albums(120);
        grid.set_model(model);
        grid.set_tile_size(dip(148.0));

        let size_slider = Slider::new(ui, 96.0..=220.0)
            .expect("grid tile size")
            .value(148.0)
            .on_change(|value| Some(Msg::Grid(GridMsg::TileSize(value))))
            .on_commit(|value| Some(Msg::Grid(GridMsg::TileSizeCommit(value))));

        Grid {
            grid,
            caption: Label::new(ui, Rect::default(), "No tile selected").expect("caption"),
            size_label: Label::new(ui, Rect::default(), "Tile size").expect("size label"),
            size_slider,
            covers,
            loaded: 0,
            last_resize: None,
        }
    }

    pub(super) fn page(&self) -> Layout {
        // The slider row is a nested layout, whose default sizing is `Fill(1)`
        // in a column; without an explicit height it would take half the tab
        // and squeeze the grid to a couple of rows.
        column![
            row![self.size_label.width(dip(70.0)), self.size_slider.fill(1)]
                .spacing(dip(8.0))
                .height(dip(28.0)),
            self.caption.height(dip(20.0)),
            self.grid.fill(1),
        ]
        .spacing(dip(6.0))
    }

    /// Handles the tab's messages. Returns whether `msg` was one.
    pub(super) fn update(&mut self, msg: &Msg) -> bool {
        // The worker tick doubles as the image loader; it is not claimed, so
        // the status bar still sees it.
        if matches!(msg, Msg::Tick(_)) {
            self.load_next_batch();
            return false;
        }
        let Msg::Grid(msg) = msg else {
            return false;
        };
        match msg {
            GridMsg::Select(index) => {
                self.caption.set_text(&format!("Selected tile {index}"));
            }
            GridMsg::Activate(index) => {
                self.caption.set_text(&format!("Activated tile {index}"));
            }
            GridMsg::TileSize(value) => {
                // Repainting the whole tile content on every slider frame is
                // what makes a drag feel slow; cap it and let the release below
                // apply the final size.
                let due = self
                    .last_resize
                    .is_none_or(|at| at.elapsed() >= TILE_RESIZE_THROTTLE);
                if due {
                    self.apply_tile_size(*value);
                }
            }
            GridMsg::TileSizeCommit(value) => self.apply_tile_size(*value),
        }
        true
    }

    fn apply_tile_size(&mut self, value: f64) {
        self.last_resize = Some(Instant::now());
        self.grid.set_tile_size(dip(value as f32));
    }

    /// Produces the next batch of covers, as a loader thread would, and asks
    /// the grid to repaint so the loaded tiles replace their placeholders.
    fn load_next_batch(&mut self) {
        let end = (self.loaded + LOAD_BATCH).min(self.covers.len());
        if end == self.loaded {
            return;
        }
        for (offset, cover) in self.covers[self.loaded..end].iter().enumerate() {
            let index = self.loaded + offset;
            let mut cover = cover.borrow_mut();
            if cover.image.is_none() {
                cover.image = Some(cover_image(PALETTE[index % PALETTE.len()], index));
            }
        }
        self.loaded = end;
        self.grid.invalidate();
    }
}

/// The text colour to draw a label on `background`: near-black on a light fill,
/// near-white on a dark one, chosen by WCAG relative luminance.
fn text_on(background: Color) -> Color {
    if background.luminance() > 0.55 {
        Color::rgb(0x1A, 0x1A, 0x1A)
    } else {
        Color::rgb(0xF5, 0xF5, 0xF5)
    }
}
