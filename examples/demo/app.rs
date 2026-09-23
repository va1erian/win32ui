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

use self::data::{LibraryTree, TrackSource, generate_tracks};
use self::icons::dot_icon;

pub(crate) fn main() {
    let theme = Theme::dark();
    let result = win32ui::run_app(
        WindowSpec::new("win32ui demo")
            .size(dip(1080.0), dip(680.0))
            .theme(theme),
        |ui| {
            let dpi = ui.dpi();

            let toolbar = Toolbar::new(
                ui,
                vec![
                    ToolbarItem::new("Scan")
                        .with_icon(dot_icon(theme.accent))
                        .on_click(|| Some(Msg::Scan)),
                    ToolbarItem::new("Shuffle").on_click(|| Some(Msg::Shuffle)),
                    ToolbarItem::new("Refresh")
                        .with_icon(dot_icon(theme.text_weak))
                        .on_click(|| Some(Msg::Refresh)),
                ],
                ToolbarTheme::from_theme(&theme),
            )
            .expect("toolbar");

            let tree = TreeView::new(ui, Rect::default(), Box::new(LibraryTree))
                .expect("tree")
                .on_select(|item| Some(Msg::TreeSelect(item)));
            tree.set_colors(theme.background, theme.text);

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
                ListViewTheme::from_theme(&theme),
            )
            .expect("list")
            .on_activate(|item| Some(Msg::Play(item)))
            .on_select(|item| Some(Msg::Select(item)));

            let status = StatusBar::new(ui, StatusBarTheme::from_theme(&theme)).expect("status");
            status.set_parts(&[-1]);
            status.set_text(0, "Ready");

            let mut app = App {
                dpi,
                toolbar,
                tree,
                list,
                status,
                tracks,
                order,
                now_playing: None,
            };
            app.layout(ui);

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
    TreeSelect(Option<i64>),
    Play(usize),
    Select(usize),
    AutoClose,
}

struct App {
    dpi: u32,
    toolbar: Toolbar<Msg>,
    tree: TreeView<Msg>,
    list: ListView<Msg>,
    status: StatusBar,
    tracks: Rc<Vec<Track>>,
    order: Vec<usize>,
    now_playing: Option<usize>,
}

impl App {
    fn layout(&mut self, ui: &Ui<Msg>) {
        let client = ui.client_rect();
        let dpi = self.dpi;
        let toolbar_height = self.toolbar.height();

        let areas = Dock::new()
            .top_px(Px(toolbar_height))
            .bottom(dip(22.0))
            .split(client, dpi);
        let columns = Stack::horizontal()
            .fixed(dip(220.0))
            .fill(1)
            .split(areas.fill, dpi);

        self.toolbar.set_bounds(areas.top.unwrap_or_default());
        self.tree.set_bounds(columns[0]);
        self.list.set_bounds(columns[1]);
        self.status.set_bounds(areas.bottom.unwrap_or_default());
    }

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
}

impl win32ui::App for App {
    type Msg = Msg;

    fn update(&mut self, msg: Msg, ui: &mut Ui<Msg>) {
        match msg {
            Msg::Scan => self.set_status("Scanning… (not wired in this PoC)"),
            Msg::Shuffle => self.set_status("Shuffle requested"),
            Msg::Refresh => self.set_status("Refreshed"),
            Msg::TreeSelect(item) => {
                let label = item
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
            Msg::AutoClose => {
                screenshot::capture_if_requested(ui);
                ui.quit();
            }
        }
    }
}
