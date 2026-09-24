//! The demo's search box: a single-line [`Edit`] with cue text that filters
//! the track list live.
//!
//! The box stays empty (so the cue shows) until the user types; every change
//! maps to [`Msg::Search`](super::Msg::Search), which re-filters here.

use win32ui::prelude::*;

use super::Msg;
use super::library::Library;

/// Builds the search label and box. The box shows cue placeholder text while
/// empty and reports every change as `Msg::Search`.
pub(super) fn build(ui: &mut Ui<Msg>) -> Result<(Label, Edit<Msg>)> {
    let label = Label::new(ui, Rect::default(), "Search")?;
    let edit = Edit::single_line(ui)?
        .cue("Search mail")
        .on_change(|text| Some(Msg::Search(text.to_owned())));
    Ok((label, edit))
}

/// Re-filters the list on `query` (case-insensitive over title, artist and
/// album) and reports the match count on the status bar.
pub(super) fn apply(library: &mut Library, query: &str, status: &dyn super::StatusWriter) {
    let total = library.tracks.len();
    if query.is_empty() {
        library.order = (0..total).collect();
    } else {
        let needle = query.to_lowercase();
        library.order = library
            .tracks
            .iter()
            .enumerate()
            .filter(|(_, track)| {
                track.title.to_lowercase().contains(&needle)
                    || track.artist.to_lowercase().contains(&needle)
                    || track.album.to_lowercase().contains(&needle)
            })
            .map(|(index, _)| index)
            .collect();
    }
    library.list.set_model(library.model());
    let shown = library.order.len();
    status.set_text(
        0,
        &if query.is_empty() {
            "Search cleared".to_string()
        } else {
            format!("{shown} of {total} for {query}")
        },
    );
}
