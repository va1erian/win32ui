//! The demo's Grid tab: a virtualized grid of solid-colour placeholder tiles
//! standing in for cover art, with a live tile-size slider.

use win32ui::gdi::{Canvas, TextFormat};
use win32ui::prelude::*;
use win32ui::{column, row};

use super::Msg;

/// A palette that cycles, so the placeholder tiles are visibly distinct
/// without depending on any image-decoding crate.
const PALETTE: [Color; 6] = [
    Color::hex(0xE07A5F),
    Color::hex(0x3D405B),
    Color::hex(0x81B29A),
    Color::hex(0xF2CC8F),
    Color::hex(0x577590),
    Color::hex(0x9B5DE5),
];

/// A stand-in for a cover-art album: this demo never decodes an image, so the
/// "art" is just a colour.
pub(super) struct Album {
    title: String,
    color: Color,
}

fn albums(count: usize) -> Vec<Album> {
    (0..count)
        .map(|index| Album {
            title: format!("Album {}", index + 1),
            color: PALETTE[index % PALETTE.len()],
        })
        .collect()
}

/// What the Grid tab tells the app.
pub(super) enum GridMsg {
    /// The selection changed.
    Select(usize),
    /// A tile was activated (double-click or Enter).
    Activate(usize),
    /// The tile-size slider moved.
    TileSize(f64),
}

pub(super) struct Grid {
    grid: GridView<Album, Msg>,
    caption: Label,
    size_label: Label,
    size_slider: Slider<Msg>,
}

impl Grid {
    pub(super) fn build(ui: &mut Ui<Msg>) -> Grid {
        let grid = GridView::<Album, Msg>::new(ui)
            .expect("grid")
            .tile_size(dip(96.0)..dip(220.0))
            .content(
                |album: &Album, canvas: &Canvas, rect: Rect, state: TileState| {
                    canvas.fill_rect(rect, album.color);
                    if state.selected || state.hovered {
                        let border = if state.selected {
                            Color::rgb(255, 255, 255)
                        } else {
                            Color::rgb(200, 200, 200)
                        };
                        canvas.outline(rect, border);
                    }
                    let caption_rect = Rect::new(
                        rect.left + 4,
                        rect.bottom - 20,
                        rect.right - 4,
                        rect.bottom - 2,
                    );
                    canvas.draw_text(
                        caption_rect,
                        &album.title,
                        Color::rgb(255, 255, 255),
                        TextFormat::left().single_line().end_ellipsis(),
                    );
                },
            )
            .on_select(|index| Some(Msg::Grid(GridMsg::Select(index))))
            .on_activate(|index| Some(Msg::Grid(GridMsg::Activate(index))));
        grid.set_model(albums(120));

        let size_slider = Slider::new(ui, 96.0..=220.0)
            .expect("grid tile size")
            .value(148.0)
            .on_change(|value| Some(Msg::Grid(GridMsg::TileSize(value))));

        Grid {
            grid,
            caption: Label::new(ui, Rect::default(), "No tile selected").expect("caption"),
            size_label: Label::new(ui, Rect::default(), "Tile size").expect("size label"),
            size_slider,
        }
    }

    pub(super) fn page(&self) -> Layout {
        column![
            row![self.size_label.width(dip(70.0)), self.size_slider.fill(1)].spacing(dip(8.0)),
            self.caption.height(dip(20.0)),
            self.grid.fill(1),
        ]
        .spacing(dip(6.0))
    }

    /// Handles the tab's messages. Returns whether `msg` was one.
    pub(super) fn update(&mut self, msg: &Msg) -> bool {
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
            GridMsg::TileSize(value) => self.grid.set_tile_size(dip(*value as f32)),
        }
        true
    }
}
