#![forbid(unsafe_code)]

//! The grid's data model ([`GridModel`]), the per-tile paint state
//! ([`TileState`]) and the tile-size spec ([`TileSizeSpec`]).

use std::ops::{Range, RangeInclusive};

use crate::units::Dip;

/// Supplies a [`GridView`](super::GridView) with tiles.
///
/// Modelled after [`ListModel`](crate::ListModel): the grid never stores the
/// items itself, so paint borrows `&Item` fresh from [`get`](GridModel::get)
/// for every visible tile.
///
/// # Example
///
/// ```
/// use win32ui::prelude::*;
///
/// struct Album {
///     title: String,
/// }
///
/// struct Library {
///     albums: Vec<Album>,
/// }
///
/// impl GridModel for Library {
///     type Item = Album;
///
///     fn len(&self) -> usize {
///         self.albums.len()
///     }
///
///     fn get(&self, index: usize) -> Option<&Album> {
///         self.albums.as_slice().get(index)
///     }
/// }
/// ```
pub trait GridModel {
    /// The tile type.
    type Item;

    /// The number of tiles.
    fn len(&self) -> usize;

    /// Whether the model holds no tiles.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The tile at `index`, or `None` past the end.
    fn get(&self, index: usize) -> Option<&Self::Item>;
}

impl<T> GridModel for Vec<T> {
    type Item = T;

    fn len(&self) -> usize {
        Vec::len(self)
    }

    fn get(&self, index: usize) -> Option<&T> {
        self.as_slice().get(index)
    }
}

/// The state a tile is painted in, passed to the `content` callback so it can
/// theme the caption and any selection chrome it draws itself; the widget
/// already paints the selection/hover fill behind the callback.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TileState {
    /// Whether this tile is the current (single) selection.
    pub selected: bool,
    /// Whether the pointer is currently over this tile.
    pub hovered: bool,
}

/// A tile size: either fixed, or a range the app can vary live (e.g. with a
/// [`Slider`](crate::Slider)) through
/// [`GridView::set_tile_size`](super::GridView::set_tile_size).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TileSizeSpec {
    pub(super) initial: Dip,
    pub(super) range: Option<(Dip, Dip)>,
}

impl From<Dip> for TileSizeSpec {
    fn from(size: Dip) -> TileSizeSpec {
        TileSizeSpec {
            initial: size,
            range: None,
        }
    }
}

impl From<Range<Dip>> for TileSizeSpec {
    fn from(range: Range<Dip>) -> TileSizeSpec {
        TileSizeSpec {
            initial: range.start,
            range: Some((range.start, range.end)),
        }
    }
}

impl From<RangeInclusive<Dip>> for TileSizeSpec {
    fn from(range: RangeInclusive<Dip>) -> TileSizeSpec {
        TileSizeSpec {
            initial: *range.start(),
            range: Some((*range.start(), *range.end())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::dip;

    struct Item(u32);

    #[test]
    fn vec_is_a_model() {
        let model = vec![Item(1)];
        assert_eq!(model.len(), 1);
        assert!(!model.is_empty());
        assert_eq!(model.get(0).map(|item| item.0), Some(1));
        assert!(model.get(1).is_none());
        assert!(Vec::<Item>::new().is_empty());
    }

    #[test]
    fn a_fixed_size_has_no_range() {
        let spec: TileSizeSpec = dip(148.0).into();
        assert_eq!(spec.initial, dip(148.0));
        assert!(spec.range.is_none());
    }

    #[test]
    fn a_range_starts_at_its_low_end() {
        let spec: TileSizeSpec = (dip(96.0)..dip(220.0)).into();
        assert_eq!(spec.initial, dip(96.0));
        assert_eq!(spec.range, Some((dip(96.0), dip(220.0))));
    }
}
