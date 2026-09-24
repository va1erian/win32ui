use std::cell::RefCell;
use std::rc::Rc;

use win32ui::prelude::*;

/// One row of mock library data.
///
/// The `*_text` fields pre-format the numeric columns: column accessors
/// borrow `&str` from the row, so anything not already a string is rendered
/// once up front rather than on every owner-data request.
pub(super) struct Track {
    pub(super) title: String,
    pub(super) artist: String,
    pub(super) album: String,
    pub(super) year: u16,
    pub(super) year_text: String,
    pub(super) genre: String,
    pub(super) seconds: u32,
    pub(super) duration_text: String,
    pub(super) format: String,
    pub(super) plays: u32,
    pub(super) plays_text: String,
    pub(super) last_played: String,
}

/// Virtual list backing store: display order plus shared rows.
pub(super) struct TrackModel {
    pub(super) tracks: Rc<Vec<Track>>,
    pub(super) order: Vec<usize>,
}

impl ListModel for TrackModel {
    type Item = Track;

    fn len(&self) -> usize {
        self.order.len()
    }

    fn get(&self, index: usize) -> Option<&Track> {
        self.order
            .as_slice()
            .get(index)
            .and_then(|&row| self.tracks.as_slice().get(row))
    }
}

/// One folder in the demo's mail tree.
pub(super) struct Folder {
    pub(super) id: u32,
    pub(super) name: String,
    pub(super) unread: u32,
    /// Draws the inbox glyph instead of the folder one.
    pub(super) inbox: bool,
    pub(super) children: Vec<Folder>,
}

/// A lazily-loaded, keyed mail folder tree. The `Rc<RefCell<…>>` is shared
/// with the app, so refreshing after an unread count changes re-reads the same
/// store.
pub(super) struct FolderModel {
    pub(super) roots: Rc<RefCell<Vec<Folder>>>,
}

impl TreeModel for FolderModel {
    type Key = u32;

    fn children(&self, parent: Option<&u32>) -> Vec<Node<u32>> {
        let roots = self.roots.borrow();
        let list: &[Folder] = match parent {
            None => roots.as_slice(),
            Some(id) => find_folder(roots.as_slice(), *id)
                .map(|folder| folder.children.as_slice())
                .unwrap_or(&[]),
        };
        list.iter()
            .map(|folder| {
                if folder.children.is_empty() {
                    Node::leaf(folder.id, folder.name.as_str())
                } else {
                    Node::branch(folder.id, folder.name.as_str())
                }
            })
            .collect()
    }
}

/// Finds a folder by key at any depth.
pub(super) fn find_folder(folders: &[Folder], id: u32) -> Option<&Folder> {
    for folder in folders {
        if folder.id == id {
            return Some(folder);
        }
        if let Some(found) = find_folder(&folder.children, id) {
            return Some(found);
        }
    }
    None
}

/// The demo's starting folder tree, with a couple of unread badges.
pub(super) fn folder_tree() -> Vec<Folder> {
    fn leaf(id: u32, name: &str, unread: u32) -> Folder {
        Folder {
            id,
            name: name.to_string(),
            unread,
            inbox: false,
            children: Vec::new(),
        }
    }
    vec![
        Folder {
            id: 1,
            name: "Inbox".to_string(),
            unread: 3,
            inbox: true,
            children: vec![
                leaf(11, "Work", 2),
                leaf(12, "Personal", 1),
                leaf(13, "Receipts", 0),
            ],
        },
        Folder {
            id: 4,
            name: "Drafts".to_string(),
            unread: 0,
            inbox: false,
            children: vec![leaf(41, "Quarterly report", 0)],
        },
        leaf(2, "Sent", 0),
        Folder {
            id: 5,
            name: "Archive".to_string(),
            unread: 0,
            inbox: false,
            children: vec![leaf(51, "2025", 0), leaf(52, "2024", 0)],
        },
    ]
}

pub(super) fn generate_tracks(count: usize) -> Vec<Track> {
    // Deliberately mixed scripts so the UTF-16 rendering path is exercised:
    // CJK, emoji (astral plane), combining marks, RTL and a zero-width space.
    const TITLES: &[&str] = &[
        "Blue Horizon",
        "Midnight Drive",
        "秋の夜長",
        "🎵 Emoji Groove",
        "Café Déjà Vu",
        "Ω≈ç√∫˜µ≤≥÷",
        "𝔘𝔫𝔦𝔠𝔬𝔡𝔢 Fantasy",
        "العربية",
        "Śpiewający ptak",
        "Northern Lights",
        "Nul\u{200b}space",
        "e\u{301}\u{327} combining",
    ];
    const ARTISTS: &[&str] = &[
        "Aurora Fields",
        "The Midnight Set",
        "Cassette Ghosts",
        "Vela",
        "Junior State",
        "山田 花子",
        "🎤 The Emoji Band",
    ];
    const ALBUMS: &[&str] = &[
        "First Light",
        "Static",
        "Harbour",
        "Reverie",
        "Signal",
        "日本語アルバム",
        "★ Best Of ★",
    ];
    const GENRES: &[&str] = &[
        "Rock",
        "Jazz",
        "Blues",
        "Electronic",
        "Classical",
        "Ambient",
        "日本語",
    ];
    const FORMATS: &[&str] = &["mp3", "flac", "m4a", "ogg", "wav"];
    const LAST_PLAYED: &[&str] = &["today", "yesterday", "3 days ago", "last month", "—"];

    let mut state: u64 = 0x2545_F491_4F6C_DD1D;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };

    (0..count)
        .map(|index| {
            let title = TITLES[index % TITLES.len()];
            let artist = ARTISTS[(next() as usize) % ARTISTS.len()];
            let album = ALBUMS[(next() as usize) % ALBUMS.len()];
            let genre = GENRES[(next() as usize) % GENRES.len()];
            let format = FORMATS[(next() as usize) % FORMATS.len()];
            let last_played = LAST_PLAYED[(next() as usize) % LAST_PLAYED.len()];
            let seconds = 120 + (next() % 240) as u32;
            let year = 1950 + (next() % 75) as u16;
            let plays = (next() % 50) as u32;
            Track {
                title: format!("{title} {}", index + 1),
                artist: artist.to_string(),
                album: format!("{album} {}", index % 7 + 1),
                year,
                year_text: year.to_string(),
                genre: genre.to_string(),
                seconds,
                duration_text: format_duration(seconds),
                format: format.to_string(),
                plays,
                plays_text: plays.to_string(),
                last_played: last_played.to_string(),
            }
        })
        .collect()
}

fn format_duration(seconds: u32) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}
