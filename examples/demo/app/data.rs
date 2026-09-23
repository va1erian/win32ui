use std::rc::Rc;

use win32ui::prelude::*;

use super::Track;

/// Virtual list backing store.
pub(super) struct TrackSource {
    pub(super) tracks: Rc<Vec<Track>>,
    pub(super) order: Vec<usize>,
    pub(super) playing: Option<usize>,
}

impl ListSource for TrackSource {
    fn item_count(&self) -> usize {
        self.order.len()
    }

    fn text(&self, item: usize, column: usize) -> String {
        let Some(&row) = self.order.get(item) else {
            return String::new();
        };
        let track = &self.tracks[row];
        match column {
            0 => (item + 1).to_string(),
            1 => {
                if self.playing == Some(item) {
                    format!("\u{25b6} {}", track.title)
                } else {
                    track.title.clone()
                }
            }
            2 => track.artist.clone(),
            3 => track.album.clone(),
            4 => track.year.to_string(),
            5 => track.genre.clone(),
            6 => format_duration(track.seconds),
            7 => track.format.clone(),
            8 => track.plays.to_string(),
            9 => track.last_played.clone(),
            _ => String::new(),
        }
    }
}

/// Lazily-populated library tree.
pub(super) struct LibraryTree;

impl TreeSource for LibraryTree {
    fn children(&self, parent: Option<i64>) -> Vec<TreeEntry> {
        match parent {
            None => vec![
                TreeEntry::branch("Music", 1),
                TreeEntry::branch("Playlists", 2),
                TreeEntry::branch("Folders", 3),
            ],
            Some(1) => ["Rock", "Jazz", "Classical", "Electronic", "Soundtrack"]
                .iter()
                .enumerate()
                .map(|(index, name)| TreeEntry::leaf(*name, 10 + index as i64))
                .collect(),
            Some(2) => ["Favourites", "Recently added", "Late night"]
                .iter()
                .enumerate()
                .map(|(index, name)| TreeEntry::leaf(*name, 20 + index as i64))
                .collect(),
            Some(3) => ["C:\\Music", "D:\\Albums"]
                .iter()
                .enumerate()
                .map(|(index, name)| TreeEntry::leaf(*name, 30 + index as i64))
                .collect(),
            _ => Vec::new(),
        }
    }
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
                genre: genre.to_string(),
                seconds,
                format: format.to_string(),
                plays,
                last_played: last_played.to_string(),
            }
        })
        .collect()
}

fn format_duration(seconds: u32) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}
