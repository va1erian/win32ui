#![forbid(unsafe_code)]

//! The list view's data model: [`Column`] specs, the [`ListSource`] trait and
//! the [`SortDirection`] of the header arrow.

use crate::units::Dip;

/// A report-mode column.
#[derive(Clone, Debug)]
pub struct Column {
    /// Header label.
    pub title: String,
    /// Initial width as a [`Dip`] design value.
    pub width: Dip,
    /// Whether the column's cells are right-aligned.
    pub align_right: bool,
}

impl Column {
    /// A left-aligned column.
    pub fn new(title: impl Into<String>, width: Dip) -> Column {
        Column {
            title: title.into(),
            width,
            align_right: false,
        }
    }

    /// A right-aligned column (numbers, durations).
    pub fn right(title: impl Into<String>, width: Dip) -> Column {
        Column {
            title: title.into(),
            width,
            align_right: true,
        }
    }
}

/// Supplies the virtual list view with its row count and cell text.
pub trait ListSource {
    /// The number of rows.
    fn item_count(&self) -> usize;

    /// The text of the cell at `item`/`column` (both zero-based).
    fn text(&self, item: usize, column: usize) -> String;
}

/// Which way a column is sorted, for the header arrow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDirection {
    /// Ascending (`HDF_SORTUP`).
    Ascending,
    /// Descending (`HDF_SORTDOWN`).
    Descending,
}
