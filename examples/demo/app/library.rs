//! The demo's Library tab: a draggable tree/list split, the typed sort combo,
//! the live search box and the list's context menu.
//!
//! The tab is the demo's largest feature, so it owns its widgets, its model and
//! the messages they raise. `App` only holds the resulting [`Library`] and
//! forwards matching messages to [`Library::update`].

use std::cell::RefCell;
use std::rc::Rc;

use win32ui::prelude::*;
use win32ui::{column, row, split_row};

use super::data::{
    Folder, FolderModel, Track, TrackModel, find_folder, folder_tree, generate_tracks,
};
use super::{Msg, menus, search};

/// The sort keys the demo's combo box holds as typed values.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SortKey {
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

/// The Library tab's widgets and view state.
pub(super) struct Library {
    pub(super) tree: TreeView<u32, Msg>,
    folders: Rc<RefCell<Vec<Folder>>>,
    pub(super) list: ListView<Track, Msg>,
    pub(super) search: Edit<Msg>,
    search_label: Label,
    pub(super) sort_combo: ComboBox<SortKey, Msg>,
    sort_label: Label,
    context: Menu<Msg>,
    pub(super) tracks: Rc<Vec<Track>>,
    pub(super) order: Vec<usize>,
    sort: Option<(usize, bool)>,
    split_position: Dip,
}

impl Library {
    /// Builds the tab's widgets, model and starting selection.
    pub(super) fn build(ui: &mut Ui<Msg>) -> Library {
        // A keyed folder tree: unread counts show as trailing badges, the
        // inbox gets its own icon, and selecting/expanding maps to `Msg`.
        let folders = Rc::new(RefCell::new(folder_tree()));
        let icon_px = dip(16.0).to_px(ui.dpi()).value();
        let mut images = ImageList::new(icon_px).expect("icons");
        images.add_rgba(icon_px, icon_px, &solid_icon(icon_px, (240, 190, 80)));
        images.add_rgba(icon_px, icon_px, &solid_icon(icon_px, (70, 140, 220)));
        let style_folders = Rc::clone(&folders);
        let tree = TreeView::new(
            ui,
            FolderModel {
                roots: Rc::clone(&folders),
            },
        )
        .expect("tree")
        .images(images)
        .style(move |id| {
            let (unread, inbox) = find_folder(style_folders.borrow().as_slice(), *id)
                .map(|folder| (folder.unread, folder.inbox))
                .unwrap_or((0, false));
            let style = NodeStyle::new()
                .bold(unread > 0)
                .icon(if inbox { 1 } else { 0 });
            if unread > 0 {
                style.badge(unread.to_string())
            } else {
                style
            }
        })
        .on_select(|id| Some(Msg::TreeSelect(*id)))
        .on_toggle(|id, expanded| Some(Msg::TreeFold(*id, expanded)));
        tree.select(&1);
        tree.expand(&1, true);

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

        let sort_label = Label::new(ui, Rect::default(), "Sort by").expect("label");
        let sort_combo = ComboBox::new(
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

        Library {
            tree,
            folders,
            list,
            search,
            search_label,
            sort_combo,
            sort_label,
            context: menus::context(),
            tracks,
            order,
            sort: None,
            split_position: dip(220.0),
        }
    }

    /// The Library tab page: the tree on the left, the search box and list on
    /// the right, in a draggable split.
    pub(super) fn page(&self) -> Split {
        split_row![
            self.tree,
            column![
                row![self.search_label.width(dip(60.0)), self.search.fill(1)].height(dip(28.0)),
                self.list.fill(1),
            ]
        ]
        .position(dip(220.0))
        .min(dip(120.0), dip(220.0))
        .on_moved(|position| Some(Msg::SplitMoved(position)))
    }

    /// The sort row shown above the tabs: a label and the typed combo.
    pub(super) fn sort_row(&self) -> Layout {
        row![
            self.sort_label.width(dip(60.0)),
            self.sort_combo.width(dip(180.0))
        ]
    }

    /// The virtual list's backing model: display order plus shared rows.
    pub(super) fn model(&self) -> TrackModel {
        TrackModel {
            tracks: Rc::clone(&self.tracks),
            order: self.order.clone(),
        }
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

    /// Handles the Library tab's messages. Returns whether `msg` was one.
    pub(super) fn update(&mut self, msg: &Msg, ui: &mut Ui<Msg>, status: &StatusBar<Msg>) -> bool {
        match msg {
            Msg::Refresh => {
                // Bump the inbox count and refresh in place: expansion and
                // selection survive because nodes are matched by key.
                if let Some(inbox) = self.folders.borrow_mut().first_mut() {
                    inbox.unread += 1;
                }
                self.tree.refresh();
                return false;
            }
            Msg::TreeSelect(id) => {
                let label = self
                    .folders
                    .borrow()
                    .iter()
                    .find(|folder| folder.id == *id)
                    .map(|folder| folder.name.clone())
                    .unwrap_or_else(|| format!("node {id}"));
                status.set_text(0, &format!("Selected folder: {label}"));
            }
            Msg::TreeFold(id, expanded) => status.set_text(
                0,
                &format!(
                    "Folder {id} {}",
                    if *expanded { "expanded" } else { "collapsed" }
                ),
            ),
            Msg::Play(item) => {
                let item = *item;
                let title = self
                    .order
                    .as_slice()
                    .get(item)
                    .and_then(|&row| self.tracks.as_slice().get(row))
                    .map(|track| track.title.clone())
                    .unwrap_or_default();
                status.set_text(0, &format!("Playing: {title}"));
            }
            Msg::Selected(rows) => match rows.as_slice() {
                [] => status.set_text(0, "No selection"),
                [only] => status.set_text(0, &format!("Selected row {}", only + 1)),
                _ => status.set_text(0, &format!("{} rows selected", rows.len())),
            },
            Msg::Sort(column) => {
                self.sort_by(*column);
                status.set_text(0, &format!("Sorted by column {}", column + 1));
            }
            Msg::SortChanged(key) => status.set_text(0, &format!("Sorted by {}", key.label())),
            Msg::Search(query) => search::apply(self, query, status),
            Msg::Copy => match self.list.selected() {
                None => status.set_text(0, "Nothing selected to copy"),
                Some(row) => match clipboard::set_text(ui.hwnd(), &self.row_text(row)) {
                    Ok(()) => status.set_text(0, &format!("Copied row {}", row + 1)),
                    Err(error) => status.set_text(0, &format!("Copy failed: {error}")),
                },
            },
            Msg::SplitMoved(position) => {
                self.split_position = *position;
                status.set_text(0, &format!("Split at {:.0} dip", position.value()));
            }
            Msg::ShowListMenu => ui.popup(&self.context, ui.cursor_position()),
            Msg::ContextPlay => status.set_text(0, "Context: play"),
            Msg::ContextDelete => status.set_text(0, "Context: delete"),
            Msg::OpenCombo => self.sort_combo.show_drop_down(true),
            _ => return false,
        }
        true
    }
}

/// A `size`-square RGBA icon: a solid fill with a transparent one-pixel edge.
fn solid_icon(size: i32, (r, g, b): (u8, u8, u8)) -> Vec<u8> {
    let mut data = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let edge = x == 0 || y == 0 || x == size - 1 || y == size - 1;
            if edge {
                data.extend_from_slice(&[0, 0, 0, 0]);
            } else {
                data.extend_from_slice(&[r, g, b, 255]);
            }
        }
    }
    data
}
