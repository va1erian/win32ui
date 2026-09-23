//! Proof of concept for the `win32ui` widget layer: an `App` with a `Msg`
//! enum, an owner-drawn toolbar, a lazily-populated side tree and a virtual
//! (owner-data) list view.
//!
//! Run with:
//!
//! ```text
//! cargo run -p win32ui --example demo
//! ```

mod data;
mod icons;
mod screenshot;

use std::rc::Rc;

use win32ui::prelude::*;
// `column!` is also a std prelude macro (an array helper), so the layout macros
// are imported explicitly to disambiguate.
use win32ui::{column, row};

use self::data::{LibraryTree, TrackSource, generate_tracks};
use self::icons::dot_icon;

pub(crate) fn main() {
    // `WIN32UI_DEMO_THEME` / `WIN32UI_DEMO_WIDTH` / `WIN32UI_DEMO_HEIGHT` let a
    // screenshot run pick the palette and the window size without editing code.
    let initial = std::env::var("WIN32UI_DEMO_THEME").unwrap_or_else(|_| "dark".to_string());
    let theme = if initial.eq_ignore_ascii_case("light") {
        Theme::light()
    } else {
        Theme::dark()
    };
    let width = env_dip("WIN32UI_DEMO_WIDTH", 1080.0);
    let height = env_dip("WIN32UI_DEMO_HEIGHT", 680.0);
    let result = win32ui::run_app(
        WindowSpec::new("win32ui demo")
            .size(dip(width), dip(height))
            .theme(theme),
        |ui| {
            let theme = ui.theme();

            let toolbar = Toolbar::new(
                ui,
                vec![
                    ToolbarItem::new("Scan")
                        .with_icon(dot_icon(theme.accent))
                        .on_click(|| Some(Msg::Scan)),
                    ToolbarItem::new("Shuffle").on_click(|| Some(Msg::Shuffle)),
                    ToolbarItem::new("Refresh")
                        .with_icon(dot_icon(theme.text_secondary))
                        .on_click(|| Some(Msg::Refresh)),
                    ToolbarItem::new("Theme").on_click(|| Some(Msg::ToggleTheme)),
                ],
            )
            .expect("toolbar");

            let tree = TreeView::new(ui, Rect::default(), Box::new(LibraryTree))
                .expect("tree")
                .on_select(|_| Some(Msg::TreeSelect));

            let columns = [
                Column::right("#", dip(44.0)),
                Column::new("Title", dip(260.0)),
                Column::new("Artist", dip(180.0)),
                Column::new("Album", dip(180.0)),
                Column::right("Year", dip(50.0)),
                Column::new("Genre", dip(110.0)),
                Column::right("Time", dip(64.0)),
                Column::new("Format", dip(60.0)),
                Column::right("Plays", dip(54.0)),
                Column::new("Last played", dip(100.0)),
            ];
            let tracks = Rc::new(generate_tracks(20_000));
            let order: Vec<usize> = (0..tracks.len()).collect();
            let list = ListView::new(
                ui,
                Rect::default(),
                &columns,
                Box::new(TrackSource {
                    tracks: Rc::clone(&tracks),
                    order: order.clone(),
                    playing: None,
                }),
            )
            .expect("list")
            .on_activate(|item| Some(Msg::Play(item)))
            .on_select(|item| Some(Msg::Select(item)))
            .on_key(|key, modifiers| {
                if modifiers.ctrl && key == Key::C {
                    Some(Msg::Copy)
                } else {
                    None
                }
            });

            let status = StatusBar::new(ui).expect("status");
            status.set_parts(&[-1]);
            status.set_text(0, "Ready");

            // The window owns the layout: it re-runs this tree on every resize
            // and DPI change, so the app never handles `WM_SIZE`.
            ui.set_layout(
                column![
                    toolbar,
                    row![tree.width(dip(220.0)), list.fill(1)].fill(1),
                    status,
                ]
                .spacing(dip(4.0)),
            );

            let app = App {
                toolbar,
                tree,
                list,
                status,
                tracks,
                order,
                column_count: columns.len(),
                now_playing: None,
            };

            // `WIN32UI_DEMO_AUTOCLOSE_MS` makes the demo quit itself; handy for
            // a headless smoke run of the example.
            if let Ok(millis) = std::env::var("WIN32UI_DEMO_AUTOCLOSE_MS") {
                let auto_close = ui.set_timer(millis.parse().unwrap_or(2000)).ok();
                ui.on_timer(move |id| {
                    if Some(id) == auto_close {
                        Some(Msg::AutoClose)
                    } else {
                        None
                    }
                });
            }

            app
        },
    );
    if let Err(error) = result {
        eprintln!("demo failed: {error}");
        std::process::exit(1);
    }
}

/// A design-value size from an environment variable, or `default`.
fn env_dip(name: &str, default: f32) -> f32 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

/// One row of mock library data.
struct Track {
    title: String,
    artist: String,
    album: String,
    year: u16,
    genre: String,
    seconds: u32,
    format: String,
    plays: u32,
    last_played: String,
}

enum Msg {
    Scan,
    Shuffle,
    Refresh,
    ToggleTheme,
    TreeSelect,
    Play(usize),
    Select(usize),
    Copy,
    AutoClose,
}

struct App {
    toolbar: Toolbar<Msg>,
    tree: TreeView<Msg>,
    list: ListView<Msg>,
    status: StatusBar,
    tracks: Rc<Vec<Track>>,
    order: Vec<usize>,
    column_count: usize,
    now_playing: Option<usize>,
}

impl App {
    fn source(&self) -> Box<dyn ListSource> {
        Box::new(TrackSource {
            tracks: Rc::clone(&self.tracks),
            order: self.order.clone(),
            playing: self.now_playing,
        })
    }

    fn rebuild_list(&mut self) {
        self.list.set_source(self.source());
        self.list.set_playing(self.now_playing);
    }

    fn set_status(&self, text: &str) {
        self.status.set_text(0, text);
    }

    /// The selected row's cells as tab-separated text, as shown in the list.
    fn row_text(&self, row: usize) -> String {
        (0..self.column_count)
            .map(|column| self.list.cell_text(row, column))
            .collect::<Vec<_>>()
            .join("\t")
    }
}

impl win32ui::App for App {
    type Msg = Msg;

    fn update(&mut self, msg: Msg, ui: &mut Ui<Msg>) {
        match msg {
            Msg::Scan => self.set_status("Scanning… (not wired in this PoC)"),
            Msg::Shuffle => self.set_status("Shuffle requested"),
            Msg::Refresh => {
                self.toolbar.invalidate();
                self.set_status("Refreshed");
            }
            Msg::ToggleTheme => {
                let next = if ui.theme().is_dark {
                    Theme::light()
                } else {
                    Theme::dark()
                };
                ui.set_theme(next);
                self.set_status("Theme switched");
            }
            Msg::TreeSelect => {
                let label = self
                    .tree
                    .selected()
                    .map(|id| format!("node {id}"))
                    .unwrap_or_else(|| "nothing".to_string());
                self.set_status(&format!("Tree selection: {label}"));
            }
            Msg::Play(item) => {
                self.now_playing = Some(item);
                self.rebuild_list();
                let title = self
                    .tracks
                    .get(item)
                    .map(|track| track.title.clone())
                    .unwrap_or_default();
                self.set_status(&format!("Playing: {title}"));
            }
            Msg::Select(item) => self.set_status(&format!("Selected row {}", item + 1)),
            Msg::Copy => match self.list.selected() {
                None => self.set_status("Nothing selected to copy"),
                Some(row) => match clipboard::set_text(ui.hwnd(), &self.row_text(row)) {
                    Ok(()) => self.set_status(&format!("Copied row {}", row + 1)),
                    Err(error) => self.set_status(&format!("Copy failed: {error}")),
                },
            },
            Msg::AutoClose => {
                screenshot::capture_if_requested(ui);
                ui.quit();
            }
        }
    }
}
