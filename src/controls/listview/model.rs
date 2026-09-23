#![forbid(unsafe_code)]

//! The list view's data model: the [`ListModel`] trait, [`Column`] specs with
//! typed accessors, and the [`SortDirection`] of the header arrow.

use crate::units::Dip;

/// Supplies a [`ListView`](super::ListView) with rows.
///
/// The model is borrowed for the paint: cell text comes from [`get`](ListModel::get)
/// plus the column's accessor, so nothing allocates per cell. Implement this
/// for the struct that already owns the rows (often holding an `Rc` to share
/// them with the rest of the app).
///
/// # Example
///
/// ```
/// use win32ui::prelude::*;
///
/// struct Mail {
///     sender: String,
///     subject: String,
/// }
///
/// struct Mailbox {
///     mails: Vec<Mail>,
/// }
///
/// impl ListModel for Mailbox {
///     type Item = Mail;
///
///     fn len(&self) -> usize {
///         self.mails.len()
///     }
///
///     fn get(&self, index: usize) -> Option<&Mail> {
///         self.mails.get(index)
///     }
/// }
/// ```
pub trait ListModel {
    /// The row type. Column accessors borrow from it.
    type Item;

    /// The number of rows.
    fn len(&self) -> usize;

    /// Whether the model holds no rows.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The row at `index`, or `None` past the end.
    fn get(&self, index: usize) -> Option<&Self::Item>;
}

impl<T> ListModel for Vec<T> {
    type Item = T;

    fn len(&self) -> usize {
        Vec::len(self)
    }

    fn get(&self, index: usize) -> Option<&T> {
        self.as_slice().get(index)
    }
}

/// A column's width: a fixed design value, or [`Fill`] to share the leftover
/// client width with the other `Fill` columns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColumnWidth {
    /// A fixed design value, converted once at the window's DPI.
    Fixed(Dip),
    /// Shares whatever client width the fixed columns leave behind.
    Fill,
}

impl From<Dip> for ColumnWidth {
    fn from(width: Dip) -> ColumnWidth {
        ColumnWidth::Fixed(width)
    }
}

/// A [`ColumnWidth`] that shares the leftover client width with the other
/// `Fill` columns. The widths are recomputed whenever the control is resized
/// or a header drag ends, so a `Fill` column never leaves a gap and never
/// forces a horizontal scrollbar on its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fill;

impl From<Fill> for ColumnWidth {
    fn from(_: Fill) -> ColumnWidth {
        ColumnWidth::Fill
    }
}

/// A report-mode column over rows of type `T`.
///
/// The accessor borrows the cell's text from the row, so the owner-data path
/// never allocates per cell. Add columns with
/// [`ListView::column`](super::ListView::column) rather than building this
/// directly.
pub struct Column<T> {
    /// Header label.
    pub title: String,
    /// Fixed width or [`Fill`].
    pub width: ColumnWidth,
    /// Whether the column's cells are right-aligned.
    pub align_right: bool,
    /// Whether the user may resize the column by dragging the header divider.
    /// `true` (the native default); dragging is vetoed otherwise.
    pub resizable: bool,
    pub(crate) text: Box<dyn for<'a> Fn(&'a T) -> &'a str>,
}

impl<T> Column<T> {
    /// A left-aligned column showing `text(row)`.
    pub fn new(
        title: impl Into<String>,
        width: impl Into<ColumnWidth>,
        text: impl for<'a> Fn(&'a T) -> &'a str + 'static,
    ) -> Column<T> {
        Column {
            title: title.into(),
            width: width.into(),
            align_right: false,
            resizable: true,
            text: Box::new(text),
        }
    }

    /// A right-aligned column (numbers, durations) showing `text(row)`.
    pub fn right(
        title: impl Into<String>,
        width: impl Into<ColumnWidth>,
        text: impl for<'a> Fn(&'a T) -> &'a str + 'static,
    ) -> Column<T> {
        Column {
            title: title.into(),
            width: width.into(),
            align_right: true,
            resizable: true,
            text: Box::new(text),
        }
    }

    /// Whether the user may resize this column by dragging its header
    /// divider. Programmatic widths via
    /// [`ListView::set_column_width`](super::ListView::set_column_width) still
    /// apply.
    pub fn resizable(mut self, resizable: bool) -> Column<T> {
        self.resizable = resizable;
        self
    }
}

/// Which way a column is sorted, for the header arrow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDirection {
    /// Ascending (`HDF_SORTUP`).
    Ascending,
    /// Descending (`HDF_SORTDOWN`).
    Descending,
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Row {
        name: String,
    }

    #[test]
    fn vec_is_a_model() {
        let model = vec![Row {
            name: "a".to_string(),
        }];
        assert_eq!(model.len(), 1);
        assert!(!model.is_empty());
        assert_eq!(model.get(0).map(|row| row.name.as_str()), Some("a"));
        assert!(model.get(1).is_none());
        assert!(Vec::<Row>::new().is_empty());
    }

    #[test]
    fn accessors_borrow_from_the_row() {
        let column = Column::new("Name", Dip::new(80.0), |row: &Row| row.name.as_str());
        let row = Row {
            name: "x".to_string(),
        };
        let text: &str = (column.text)(&row);
        assert_eq!(text, "x");
        assert!(column.resizable);
        assert!(!column.resizable(false).resizable);
    }
}
