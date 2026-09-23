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
mod document;
mod icons;
mod primitives;
mod screenshot;
mod search;
mod secondary;
mod settings;
mod swatch;
mod text_specimen;

use std::rc::Rc;

use win32ui::prelude::*;
// `column!` is also a std prelude macro (an array helper), so the layout macros
// are imported explicitly to disambiguate.
use win32ui::{column, row, split_row};

use self::data::{LibraryTree, TrackModel, generate_tracks};
use self::document::DocumentWidget;
use self::icons::dot_icon;
use self::secondary::PrefsMsg;
use self::swatch::Swatch;

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
            text_specimen::open_if_requested(theme, ui.dpi());

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
                    ToolbarItem::new("Clear").on_click(|| Some(Msg::Clear)),
                    ToolbarItem::new("Prefs").on_click(|| Some(Msg::OpenPrefs)),
                    ToolbarItem::new("Confirm").on_click(|| Some(Msg::OpenConfirm)),
                ],
            )
            .expect("toolbar");

            let tree = TreeView::new(ui, Rect::default(), Box::new(LibraryTree))
                .expect("tree")
                .on_select(|_| Some(Msg::TreeSelect));

            let tracks = Rc::new(generate_tracks(20_000));
            let order: Vec<usize> = (0..tracks.len()).collect();
            let list = ListView::new(ui)
                .expect("list")
                .column("Title", Fill, |row: &Track| row.title.as_str())
                .column("Artist", dip(180.0), |row: &Track| row.artist.as_str())
                .column("Album", dip(180.0), |row: &Track| row.album.as_str())
                .column_right("Year", dip(60.0), |row: &Track| row.year_text.as_str())
                .column("Genre", dip(110.0), |row: &Track| row.genre.as_str())
                .column_right("Time", dip(64.0), |row: &Track| row.duration_text.as_str())
                .column("Format", dip(60.0), |row: &Track| row.format.as_str())
                .column_right("Plays", dip(54.0), |row: &Track| row.plays_text.as_str())
                .column("Last played", dip(100.0), |row: &Track| {
                    row.last_played.as_str()
                })
                .multi_select(true)
                .on_activate(|item| Some(Msg::Play(item)))
                .on_select(|rows| Some(Msg::Selected(rows.to_vec())))
                .on_sort(|column| Some(Msg::Sort(column)))
                .on_key(|key, modifiers| {
                    if modifiers.ctrl && key == Key::C {
                        Some(Msg::Copy)
                    } else {
                        None
                    }
                })
                .on_context(|_item| Some(Msg::ShowListMenu));
            list.set_model(TrackModel {
                tracks: Rc::clone(&tracks),
                order: order.clone(),
            });
            // Start with a few rows selected, showing off multi-select (and
            // giving the screenshots something to show).
            list.set_selection(&[1, 2, 3]);

            let status = StatusBar::new(ui).expect("status");
            status.set_parts(&[-1]);
            status.set_text(0, "Ready");

            // A menu bar mapped to `Msg`; enabled items with a shortcut also
            // register that shortcut as an accelerator, so they fire while any
            // widget has focus. A submenu, a radio item, a checked item and a
            // disabled item are all exercised here.
            let file_menu = Menu::new()
                .item("&Scan", Shortcut::ctrl(Key::S), || Msg::Scan)
                .item("&Refresh", Shortcut::ctrl(Key::R), || Msg::Refresh)
                .separator()
                .item("&Clear", Shortcut::key(Key::DELETE), || Msg::Clear)
                .separator()
                .item("E&xit", Shortcut::ctrl(Key::Q), || Msg::Quit);
            let view_menu = Menu::new()
                .radio_item("&Light", None, !theme.is_dark, || {
                    Msg::SetTheme(ThemeChoice::Light)
                })
                .radio_item("&Dark", None, theme.is_dark, || {
                    Msg::SetTheme(ThemeChoice::Dark)
                })
                .separator()
                .checked_item("Load &remote images", None, false, || {
                    Msg::RemoteImages(true)
                })
                .disabled_item("Always disabled", None, || Msg::Refresh);
            let theme_switch =
                Menu::new().item("&Toggle", Shortcut::ctrl(Key::T), || Msg::ToggleTheme);
            let menubar = Menu::new()
                .submenu("&File", file_menu)
                .submenu("&View", view_menu)
                .submenu("&Theme", theme_switch);
            ui.set_menu_bar(menubar);

            // A context menu for the list, shown by `Msg::ShowListMenu` at the
            // cursor. `ui.popup` runs `TrackPopupMenuEx(TPM_RETURNCMD)` and
            // queues the chosen item's message.
            let context = Menu::new()
                .item("&Play", None, || Msg::ContextPlay)
                .checked_item("&Loop", None, true, || Msg::ContextPlay)
                .disabled_item("&Transcode", None, || Msg::Refresh)
                .separator()
                .submenu(
                    "&Copy to",
                    Menu::new()
                        .item("&Clipboard", Shortcut::ctrl(Key::C), || Msg::Copy)
                        .item("&File…", None, || Msg::Clear),
                )
                .separator()
                .item("&Delete", Shortcut::key(Key::DELETE), || Msg::ContextDelete);

            let sort_label = Label::new(ui, Rect::default(), "Sort by").expect("label");
            let sort = ComboBox::new(
                ui,
                [
                    ("Title", SortKey::Title),
                    ("Artist", SortKey::Artist),
                    ("Album", SortKey::Album),
                    ("Year", SortKey::Year),
                ],
            )
            .expect("combo")
            .select(&SortKey::Title)
            .on_select(|key| Some(Msg::SortChanged(*key)));

            let (search_label, search) = search::build(ui).expect("search");
            search.focus();

            let progress = ProgressBar::new(ui)
                .expect("progress")
                .range(0..=100)
                .value(40);

            // A custom owner-drawn widget: a colour swatch that raises
            // `Clicked`, mapped to `Msg::SwatchClicked` below.
            let swatch = Custom::new(ui, Swatch::new(theme.accent))
                .expect("swatch")
                .on_event(|_| Some(Msg::SwatchClicked));

            // The Direct2D primitives panel (gradients, bitmaps, rounded
            // corners, clips and paths).
            let primitives = primitives::PrimitivesPanel::panel(ui);

            // A tall Direct2D document with its own vertical scroll host,
            // mapping the scroll offset to `Msg::DocumentScrolled`.
            let document = document::build(ui);
            // Options panel: a default push button, a check box, a labelled
            // group of typed radios and a disabled button. The radios report
            // values, not indices.
            let send = Button::new(ui, "Send")
                .expect("send")
                .default()
                .on_click(|| Some(Msg::Send));
            let remote = CheckBox::new(ui, "Load remote images")
                .expect("remote")
                .checked(false)
                .on_toggle(|on| Some(Msg::RemoteImages(on)));
            let theme_group = GroupBox::new(ui, "Theme").expect("theme group");
            let initial_choice = if theme.is_dark {
                ThemeChoice::Dark
            } else {
                ThemeChoice::Light
            };
            let themes = RadioGroup::new(
                ui,
                [
                    ("Light", ThemeChoice::Light),
                    ("Dark", ThemeChoice::Dark),
                    ("System", ThemeChoice::System),
                ],
            )
            .expect("themes")
            .selected(initial_choice)
            .on_select(|choice| Some(Msg::SetTheme(*choice)));
            let disabled = Button::new(ui, "Disabled").expect("disabled");
            disabled.set_enabled(false);

            // Shortcuts are data and fire whichever widget has focus. The menu
            // bar already auto-registers its items' shortcuts; these explicit
            // ones show the `accelerator` API and would be added by hand for
            // actions that have no menu item.
            ui.accelerator(Shortcut::ctrl(Key::Q), || Some(Msg::Quit));
            ui.accelerator(Shortcut::ctrl(Key::T), || Some(Msg::ToggleTheme));

            // The window owns the layout: it re-runs this tree on every resize
            // and DPI change, so the app never handles `WM_SIZE`.
            let options = column![
                send,
                remote,
                theme_group.height(dip(20.0)),
                themes.layout(),
                disabled,
            ]
            .spacing(dip(6.0));
            ui.set_layout(
                column![
                    toolbar,
                    row![sort_label.width(dip(60.0)), sort.width(dip(180.0))].height(dip(30.0)),
                    progress.height(dip(8.0)),
                    swatch.height(dip(24.0)),
                    primitives.height(dip(170.0)),
                    document.height(dip(240.0)),
                    row![
                        // A draggable split: the tree on the left, the search
                        // box and list on the right. `on_moved` lets the app
                        // persist the divider position.
                        split_row![
                            tree,
                            column![
                                row![search_label.width(dip(60.0)), search.fill(1)]
                                    .height(dip(28.0)),
                                list.fill(1),
                            ]
                        ]
                        .position(dip(220.0))
                        .min(dip(120.0), dip(220.0))
                        .on_moved(|position| Some(Msg::SplitMoved(position))),
                        options.width(dip(220.0))
                    ]
                    .fill(1),
                    status,
                ]
                .spacing(dip(4.0)),
            );

            let app = App {
                toolbar,
                tree,
                list,
                status,
                progress,
                sort_combo: sort,
                _sort_label: sort_label,
                _search: search,
                _search_label: search_label,
                swatch,
                document,
                context,
                _primitives: primitives,
                options: Options {
                    send,
                    remote,
                    themes,
                    theme_group,
                    disabled,
                },
                tracks,
                order,
                sort: None,
                now_playing: None,
                split_position: dip(220.0),
                prefs: None,
            };

            // A worker thread ticks a counter into the status bar through the
            // proxy. When the window is gone `send` hands the message back, so
            // the worker stops itself instead of panicking.
            let worker = ui.proxy();
            std::thread::spawn(move || {
                let mut tick = 0u64;
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    tick += 1;
                    if worker.send(Msg::Tick(tick)).is_err() {
                        break;
                    }
                }
            });

            // `WIN32UI_DEMO_AUTOCLOSE_MS` makes the demo quit itself; handy for
            // a headless smoke run of the example. `WIN32UI_DEMO_COMBO_OPEN`
            // drops the combo box's list down before the screenshot is taken.
            let auto_close = std::env::var("WIN32UI_DEMO_AUTOCLOSE_MS")
                .ok()
                .and_then(|millis| millis.parse().ok())
                .and_then(|millis| ui.set_timer(millis).ok());
            let combo_open = if std::env::var("WIN32UI_DEMO_COMBO_OPEN").is_ok() {
                ui.set_timer(1000).ok()
            } else {
                None
            };
            // `WIN32UI_DEMO_CONTEXT_OPEN` shows the list's context popup so a
            // dark (owner-drawn) menu can be inspected.
            let context_open = if std::env::var("WIN32UI_DEMO_CONTEXT_OPEN").is_ok() {
                ui.set_timer(1000).ok()
            } else {
                None
            };
            if auto_close.is_some() || combo_open.is_some() || context_open.is_some() {
                ui.on_timer(move |id| {
                    if Some(id) == auto_close {
                        Some(Msg::AutoClose)
                    } else if Some(id) == combo_open {
                        Some(Msg::OpenCombo)
                    } else if Some(id) == context_open {
                        Some(Msg::ShowListMenu)
                    } else {
                        None
                    }
                });
            }

            // `WIN32UI_DEMO_SECONDARY_SCREENSHOT` names a directory to write the
            // light/dark composite of both secondary window kinds to.
            if std::env::var("WIN32UI_DEMO_SECONDARY_SCREENSHOT").is_ok() {
                ui.emit(Msg::SecondaryScreenshot);
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
///
/// The `*_text` fields pre-format the numeric columns: column accessors
/// borrow `&str` from the row, so anything not already a string is rendered
/// once up front rather than on every owner-data request.
struct Track {
    title: String,
    artist: String,
    album: String,
    year: u16,
    year_text: String,
    genre: String,
    seconds: u32,
    duration_text: String,
    format: String,
    plays: u32,
    plays_text: String,
    last_played: String,
}

enum Msg {
    Scan,
    Shuffle,
    Refresh,
    ToggleTheme,
    Clear,
    SplitMoved(Dip),
    TreeSelect,
    Play(usize),
    Selected(Vec<usize>),
    Sort(usize),
    Copy,
    Send,
    RemoteImages(bool),
    SetTheme(ThemeChoice),
    Tick(u64),
    SortChanged(SortKey),
    Search(String),
    OpenCombo,
    SwatchClicked,
    DocumentScrolled(Dip),
    OpenPrefs,
    OpenConfirm,
    SecondaryScreenshot,
    ShowListMenu,
    ContextPlay,
    ContextDelete,
    Quit,
    AutoClose,
}

/// The sort keys the demo's combo box holds as typed values.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SortKey {
    Title,
    Artist,
    Album,
    Year,
}

impl SortKey {
    fn label(self) -> &'static str {
        match self {
            SortKey::Title => "Title",
            SortKey::Artist => "Artist",
            SortKey::Album => "Album",
            SortKey::Year => "Year",
        }
    }
}

/// The typed choices the options panel's radio group reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ThemeChoice {
    Light,
    Dark,
    System,
}

/// The typed choices the demo's task dialog can return.
#[derive(Clone, PartialEq, Eq)]
enum DialogChoice {
    Delete,
    Cancel,
}

struct App {
    toolbar: Toolbar<Msg>,
    tree: TreeView<Msg>,
    list: ListView<Track, Msg>,
    status: StatusBar<Msg>,
    progress: ProgressBar,
    sort_combo: ComboBox<SortKey, Msg>,
    _sort_label: Label,
    _search: Edit<Msg>,
    _search_label: Label,
    swatch: Custom<Swatch, Msg>,
    /// Owns the document widget's window; it paints and scrolls through its
    /// `HWND` and reports the offset as `Msg::DocumentScrolled`.
    #[allow(dead_code)]
    document: Custom<DocumentWidget, Msg>,
    /// The list's context menu, kept alive for the window's lifetime.
    context: Menu<Msg>,
    // The Direct2D primitives panel (kept alive; never read).
    _primitives: Custom<primitives::PrimitivesPanel, Msg>,
    // Owns the options panel's windows; read through their `HWND`s.
    #[allow(dead_code)]
    options: Options,
    tracks: Rc<Vec<Track>>,
    order: Vec<usize>,
    sort: Option<(usize, bool)>,
    now_playing: Option<usize>,
    split_position: Dip,
    prefs: Option<WindowHandle<PrefsMsg>>,
}

/// Handles for the options panel. The widgets paint and notify through their
/// `HWND`s; holding them here keeps those windows alive.
#[allow(dead_code)]
struct Options {
    send: Button<Msg>,
    remote: CheckBox<Msg>,
    themes: RadioGroup<ThemeChoice, Msg>,
    theme_group: GroupBox,
    disabled: Button<Msg>,
}

impl App {
    fn model(&self) -> TrackModel {
        TrackModel {
            tracks: Rc::clone(&self.tracks),
            order: self.order.clone(),
        }
    }

    fn set_status(&self, text: &str) {
        self.status.set_text(0, text);
    }

    /// The selected row's cells as tab-separated text, as shown in the list.
    fn row_text(&self, row: usize) -> String {
        (0..self.list.column_count())
            .map(|column| self.list.cell_text(row, column))
            .collect::<Vec<_>>()
            .join("\t")
    }

    /// Sorts the display order by `column`, toggling the direction when the
    /// same header is clicked twice, and refreshes the view.
    fn sort_by(&mut self, column: usize) {
        let ascending = match self.sort {
            Some((sorted, was_ascending)) if sorted == column => !was_ascending,
            _ => true,
        };
        if let Some((sorted, _)) = self.sort
            && sorted != column
        {
            self.list.clear_sort_indicator(sorted);
        }
        let tracks = Rc::clone(&self.tracks);
        let key = |&row: &usize| &tracks[row];
        match column {
            1 => self.order.sort_by(|a, b| key(a).artist.cmp(&key(b).artist)),
            2 => self.order.sort_by(|a, b| key(a).album.cmp(&key(b).album)),
            3 => self.order.sort_by_key(|&row| key(&row).year),
            4 => self.order.sort_by(|a, b| key(a).genre.cmp(&key(b).genre)),
            5 => self.order.sort_by_key(|&row| key(&row).seconds),
            6 => self.order.sort_by(|a, b| key(a).format.cmp(&key(b).format)),
            7 => self.order.sort_by_key(|&row| key(&row).plays),
            8 => self
                .order
                .sort_by(|a, b| key(a).last_played.cmp(&key(b).last_played)),
            _ => self.order.sort_by(|a, b| key(a).title.cmp(&key(b).title)),
        }
        if !ascending {
            self.order.reverse();
        }
        self.sort = Some((column, ascending));
        let direction = if ascending {
            SortDirection::Ascending
        } else {
            SortDirection::Descending
        };
        self.list.set_sort_indicator(column, direction);
        self.list.set_model(self.model());
    }
}

impl win32ui::App for App {
    type Msg = Msg;

    fn update(&mut self, msg: Msg, ui: &mut Ui<Msg>) {
        match msg {
            Msg::Scan => {
                self.progress.set_marquee(true);
                self.set_status("Scanning… (indeterminate)");
            }
            Msg::Shuffle => self.set_status("Shuffle requested"),
            Msg::Refresh => {
                self.toolbar.invalidate();
                self.progress.set_marquee(false);
                self.progress.set_range(0..=100);
                self.progress.set_value(40);
                self.set_status("Refreshed");
            }
            Msg::Clear => match TaskDialog::new("Delete 3 messages?")
                .content("They will be moved to Trash.")
                .buttons([
                    ("Delete", DialogChoice::Delete),
                    ("Cancel", DialogChoice::Cancel),
                ])
                .default(DialogChoice::Cancel)
                .icon(TaskDialogIcon::Warning)
                .verification("Don't ask again")
                .show(ui)
            {
                Ok((DialogChoice::Delete, dont_ask)) => {
                    self.set_status(if dont_ask {
                        "Deleted 3 messages (and won't ask again)"
                    } else {
                        "Deleted 3 messages"
                    });
                }
                Ok((DialogChoice::Cancel, _)) => self.set_status("Delete cancelled"),
                Err(error) => self.set_status(&format!("Task dialog unavailable: {error}")),
            },
            Msg::ToggleTheme => {
                let next = if ui.theme().is_dark {
                    Theme::light()
                } else {
                    Theme::dark()
                };
                ui.set_theme(next);
                self.set_status("Theme switched");
            }
            Msg::Send => self.set_status("Send clicked"),
            Msg::RemoteImages(on) => self.set_status(if on {
                "Remote images on"
            } else {
                "Remote images off"
            }),
            Msg::SetTheme(choice) => {
                // "System" follows the light palette until #23 lands.
                let next = if choice == ThemeChoice::Dark {
                    Theme::dark()
                } else {
                    Theme::light()
                };
                ui.set_theme(next);
                self.set_status(&format!("Theme: {choice:?}"));
            }
            Msg::SplitMoved(position) => {
                self.split_position = position;
                self.set_status(&format!("Split at {:.0} dip", position.value()));
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
                self.list.set_playing(Some(item));
                let title = self
                    .order
                    .get(item)
                    .and_then(|&row| self.tracks.get(row))
                    .map(|track| track.title.clone())
                    .unwrap_or_default();
                self.set_status(&format!("Playing: {title}"));
            }
            Msg::Selected(rows) => match rows.as_slice() {
                [] => self.set_status("No selection"),
                [only] => self.set_status(&format!("Selected row {}", only + 1)),
                _ => self.set_status(&format!("{} rows selected", rows.len())),
            },
            Msg::Sort(column) => {
                self.sort_by(column);
                self.set_status(&format!("Sorted by column {}", column + 1));
            }
            Msg::Tick(tick) => self.set_status(&format!("Worker tick {tick}")),
            Msg::SwatchClicked => {
                // Mutate the custom widget through its `Cell` state, then ask it
                // to repaint — the same pattern apps use for their own widgets.
                let theme = ui.theme();
                let next = match self.swatch.widget().borrow().color() {
                    c if c == theme.accent => theme.selection,
                    c if c == theme.selection => theme.warning,
                    _ => theme.accent,
                };
                self.swatch.widget().borrow().set_color(next);
                self.swatch.invalidate();
                self.set_status("Swatch clicked");
            }
            Msg::DocumentScrolled(offset) => {
                self.set_status(&format!("Document scrolled to {:.0} dip", offset.value()));
            }
            Msg::Copy => match self.list.selected() {
                None => self.set_status("Nothing selected to copy"),
                Some(row) => match clipboard::set_text(ui.hwnd(), &self.row_text(row)) {
                    Ok(()) => self.set_status(&format!("Copied row {}", row + 1)),
                    Err(error) => self.set_status(&format!("Copy failed: {error}")),
                },
            },
            Msg::OpenPrefs => {
                if self.prefs.is_none() {
                    match secondary::open_prefs(ui) {
                        Ok(handle) => {
                            self.set_status("Preferences open");
                            self.prefs = Some(handle);
                        }
                        Err(error) => self.set_status(&format!("Preferences failed: {error}")),
                    }
                }
            }
            Msg::OpenConfirm => {
                let result = secondary::open_confirm(ui);
                self.set_status(&format!("Confirm returned {result:?}"));
            }
            Msg::SecondaryScreenshot => {
                self.set_status(if secondary::run_screenshot(ui) {
                    "Secondary windows captured"
                } else {
                    "Secondary screenshot failed"
                });
                ui.quit();
            }
            Msg::Quit => ui.quit(),
            Msg::SortChanged(key) => {
                self.set_status(&format!("Sorted by {}", key.label()));
            }
            Msg::Search(query) => search::apply(self, &query),
            Msg::OpenCombo => self.sort_combo.show_drop_down(true),
            Msg::ShowListMenu => ui.popup(&self.context, ui.cursor_position()),
            Msg::ContextPlay => self.set_status("Context: play"),
            Msg::ContextDelete => self.set_status("Context: delete"),
            Msg::AutoClose => {
                screenshot::capture_if_requested(ui);
                primitives::PrimitivesPanel::capture_if_requested(ui, &self._primitives);
                ui.quit();
            }
        }
    }
}
