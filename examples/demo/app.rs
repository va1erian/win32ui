//! Proof of concept for `win32ui`: a window with an owner-drawn toolbar, a
//! lazily-populated side tree and a virtual (owner-data) list view.
//!
//! Run with:
//!
//! ```text
//! cargo run -p win32ui --example demo
//! ```

mod data;
mod icons;

use std::cell::RefCell;
use std::rc::Rc;

use win32ui::prelude::*;

use self::data::{LibraryTree, TrackSource, generate_tracks};
use self::icons::dot_icon;

const TOOLBAR_SCAN: u16 = 100;
const TOOLBAR_SHUFFLE: u16 = 101;
const TOOLBAR_REFRESH: u16 = 102;

const ID_TREE: usize = 1000;
const ID_LIST: usize = 1001;
const ID_STATUS: usize = 1002;

pub(crate) fn main() {
    win32ui::init();

    let theme = Theme::dark();
    let Ok(class) = WindowClass::register("emusic.demo", theme.background) else {
        return;
    };

    let app = App::new(theme);
    let Ok(window) = Window::create(
        class,
        None,
        WindowStyle::overlapped().min_max(),
        WindowExStyle::new(),
        Rect::new(80, 80, 1080, 680),
        "win32ui demo",
        app,
    ) else {
        return;
    };

    window.show();
    let code = win32ui::run();
    std::process::exit(code);
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

struct App {
    theme: Theme,
    dpi: u32,
    toolbar: Option<Toolbar>,
    tree: Option<TreeView>,
    tracks: Rc<Vec<Track>>,
    order: RefCell<Vec<usize>>,
    list: Option<ListView>,
    status: Option<StatusBar>,
    sort_column: Option<usize>,
    ascending: bool,
    now_playing: Option<usize>,
    auto_close: Option<TimerId>,
}

impl App {
    fn new(theme: Theme) -> App {
        let tracks = generate_tracks(20_000);
        let order = (0..tracks.len()).collect();
        App {
            theme,
            dpi: 96,
            toolbar: None,
            tree: None,
            tracks: Rc::new(tracks),
            order: RefCell::new(order),
            list: None,
            status: None,
            sort_column: None,
            ascending: true,
            now_playing: None,
            auto_close: None,
        }
    }

    fn setup(&mut self, window: &Window) {
        self.dpi = window.dpi();
        let theme = self.theme;

        let toolbar_theme = ToolbarTheme::from_theme(&theme);
        self.toolbar = Toolbar::new(
            window.hwnd(),
            vec![
                ToolbarItem::new(TOOLBAR_SCAN, "Scan").with_icon(dot_icon(theme.accent)),
                ToolbarItem::new(TOOLBAR_SHUFFLE, "Shuffle"),
                ToolbarItem::new(TOOLBAR_REFRESH, "Refresh").with_icon(dot_icon(theme.text_weak)),
            ],
            toolbar_theme,
            self.dpi,
        )
        .ok();

        self.tree = TreeView::new(
            window.hwnd(),
            ID_TREE,
            Rect::default(),
            Box::new(LibraryTree),
            self.dpi,
        )
        .ok();
        if let Some(tree) = &self.tree {
            tree.set_colors(theme.background, theme.text);
        }

        let columns = [
            Column::right("#", 44),
            Column::new("Title", 260),
            Column::new("Artist", 180),
            Column::new("Album", 180),
            Column::right("Year", 50),
            Column::new("Genre", 110),
            Column::right("Time", 64),
            Column::new("Format", 60),
            Column::right("Plays", 54),
            Column::new("Last played", 100),
        ];
        let source = self.source();
        self.list = ListView::new(
            window.hwnd(),
            ID_LIST,
            Rect::default(),
            &columns,
            source,
            ListViewTheme::from_theme(&theme),
            self.dpi,
        )
        .ok();

        self.status = StatusBar::new(
            window.hwnd(),
            ID_STATUS,
            StatusBarTheme::from_theme(&theme),
            self.dpi,
        )
        .ok();
        if let Some(status) = &self.status {
            status.set_parts(&[-1]);
            status.set_text(0, "Ready");
        }

        self.layout(window);

        // `WIN32UI_DEMO_AUTOCLOSE_MS` makes the demo quit itself; handy for a
        // headless smoke run of the example.
        if let Ok(millis) = std::env::var("WIN32UI_DEMO_AUTOCLOSE_MS") {
            self.auto_close = window.set_timer(millis.parse().unwrap_or(1500)).ok();
        }
    }

    fn layout(&self, window: &Window) {
        let client = window.client_rect();
        let dpi = self.dpi;
        let toolbar_height = self.toolbar.as_ref().map(Toolbar::height).unwrap_or(0);

        let areas = Dock::new()
            .top_px(toolbar_height)
            .bottom(22)
            .split(client, dpi);
        let columns = Stack::horizontal()
            .fixed(220)
            .fill(1)
            .split(areas.fill, dpi);

        if let Some(toolbar) = &self.toolbar {
            toolbar.set_bounds(areas.top.unwrap_or_default());
        }
        if let Some(tree) = &self.tree {
            tree.set_bounds(columns[0]);
        }
        if let Some(list) = &self.list {
            list.set_bounds(columns[1]);
        }
        if let Some(status) = &self.status {
            status.set_bounds(areas.bottom.unwrap_or_default());
        }
    }

    fn source(&self) -> Box<dyn ListSource> {
        Box::new(TrackSource {
            tracks: Rc::clone(&self.tracks),
            order: self.order.borrow().clone(),
            playing: self.now_playing,
        })
    }

    fn rebuild_list(&self) {
        let Some(list) = &self.list else {
            return;
        };
        list.set_source(self.source());
        list.set_playing(self.now_playing);
    }

    fn sort_by(&mut self, column: usize) {
        if column == 0 {
            return;
        }
        if self.sort_column == Some(column) {
            self.ascending = !self.ascending;
        } else {
            self.sort_column = Some(column);
            self.ascending = true;
        }
        let ascending = self.ascending;
        let tracks = Rc::clone(&self.tracks);
        let mut order = self.order.borrow_mut();
        order.sort_by(|&a, &b| {
            let key = |index: usize| -> (String, String) {
                let track = &tracks[index];
                match column {
                    2 => (track.artist.to_lowercase(), track.title.to_lowercase()),
                    3 => (track.album.to_lowercase(), track.title.to_lowercase()),
                    4 => (track.year.to_string(), track.title.to_lowercase()),
                    5 => (track.genre.to_lowercase(), track.title.to_lowercase()),
                    6 => (track.seconds.to_string(), track.title.to_lowercase()),
                    7 => (track.format.clone(), track.title.to_lowercase()),
                    8 => (track.plays.to_string(), track.title.to_lowercase()),
                    9 => (track.last_played.clone(), track.title.to_lowercase()),
                    _ => (track.title.to_lowercase(), track.artist.to_lowercase()),
                }
            };
            let left = key(a);
            let right = key(b);
            if ascending {
                left.cmp(&right)
            } else {
                right.cmp(&left)
            }
        });
        drop(order);

        if let Some(list) = &self.list {
            list.clear_sort_indicator(column);
        }
        self.rebuild_list();
        if let Some(list) = &self.list {
            let direction = if self.ascending {
                SortDirection::Ascending
            } else {
                SortDirection::Descending
            };
            list.set_sort_indicator(column, direction);
        }
    }

    fn set_status(&self, text: &str) {
        if let Some(status) = &self.status {
            status.set_text(0, text);
        }
    }
}

impl WindowHandler for App {
    fn message(&mut self, window: &Window, message: Message) -> Option<LResult> {
        match message {
            Message::Create => {
                self.setup(window);
                Some(0)
            }
            Message::Size { .. } => {
                self.layout(window);
                Some(0)
            }
            Message::DpiChanged { dpi, .. } => {
                self.dpi = dpi;
                self.layout(window);
                Some(0)
            }
            Message::Timer { id } if self.auto_close == Some(id) => {
                window.destroy();
                win32ui::quit(0);
                Some(0)
            }
            Message::Command(command) => {
                match command.id {
                    TOOLBAR_SCAN => self.set_status("Scanning… (not wired in this PoC)"),
                    TOOLBAR_SHUFFLE => self.set_status("Shuffle requested"),
                    TOOLBAR_REFRESH => self.set_status("Refreshed"),
                    _ => {}
                }
                Some(0)
            }
            Message::Notify(Notify::ListView { event, .. }) => {
                match event {
                    ListViewEvent::ColumnClick { column } => self.sort_by(column as usize),
                    ListViewEvent::DoubleClick { item } if item >= 0 => {
                        self.now_playing = Some(item as usize);
                        self.rebuild_list();
                        let title = self
                            .tracks
                            .get(item as usize)
                            .map(|track| track.title.clone())
                            .unwrap_or_default();
                        self.set_status(&format!("Playing: {title}"));
                    }
                    ListViewEvent::ItemChanged { item, selected } if selected && item >= 0 => {
                        self.set_status(&format!("Selected row {}", item + 1));
                    }
                    _ => {}
                }
                Some(0)
            }
            Message::Notify(Notify::TreeView { event, .. }) => {
                if let TreeViewEvent::SelectionChanged { item } = event {
                    let label = item
                        .map(|id| format!("node {id}"))
                        .unwrap_or_else(|| "nothing".to_string());
                    self.set_status(&format!("Tree selection: {label}"));
                }
                Some(0)
            }
            Message::Close => {
                window.destroy();
                win32ui::quit(0);
                Some(0)
            }
            _ => None,
        }
    }
}
