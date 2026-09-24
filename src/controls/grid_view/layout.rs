#![forbid(unsafe_code)]

//! Pure tile-grid arithmetic: virtualization ranges and keyboard navigation.
//! Kept free of any Win32 type so it is exercised with plain unit tests.

use std::ops::Range;

/// A keyboard navigation direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Direction {
    Left,
    Right,
    Up,
    Down,
}

/// How many tiles fit across `viewport_width`, at least one.
pub(super) fn columns_for_width(viewport_width: i32, tile: i32, spacing: i32) -> usize {
    if tile <= 0 {
        return 1;
    }
    let stride = tile + spacing.max(0);
    ((viewport_width + spacing.max(0)) / stride).max(1) as usize
}

/// The number of rows `len` tiles need at `columns` per row.
pub(super) fn row_count(len: usize, columns: usize) -> usize {
    let columns = columns.max(1);
    len.div_ceil(columns)
}

/// The full content height, in device pixels, for `len` tiles at `columns`
/// per row.
pub(super) fn content_height_px(len: usize, columns: usize, tile: i32, spacing: i32) -> i32 {
    let rows = row_count(len, columns);
    if rows == 0 {
        return 0;
    }
    let stride = tile + spacing.max(0);
    rows as i32 * stride - spacing.max(0)
}

/// The rows that intersect `[offset, offset + viewport_height)`, clamped to
/// `[0, row_count)`.
pub(super) fn visible_row_range(
    offset: i32,
    viewport_height: i32,
    tile: i32,
    spacing: i32,
    row_count: usize,
) -> Range<usize> {
    if tile <= 0 || row_count == 0 || viewport_height <= 0 {
        return 0..0;
    }
    let stride = tile + spacing.max(0);
    let first = (offset.max(0) / stride) as usize;
    let last = (offset.max(0) + viewport_height).div_ceil(stride).max(0) as usize;
    first.min(row_count)..last.min(row_count)
}

/// The tile indices visible in `[offset, offset + viewport_height)`.
pub(super) fn visible_item_range(
    offset: i32,
    viewport_height: i32,
    tile: i32,
    spacing: i32,
    columns: usize,
    len: usize,
) -> Range<usize> {
    let columns = columns.max(1);
    let rows = visible_row_range(
        offset,
        viewport_height,
        tile,
        spacing,
        row_count(len, columns),
    );
    let start = (rows.start * columns).min(len);
    let end = (rows.end * columns).min(len);
    start..end
}

/// The tile index under `(x, y)` (in the same coordinate space `tile` and
/// `spacing` are given in), or `None` outside every tile — including the
/// spacing gaps and past the last item.
pub(super) fn index_at_point(
    x: i32,
    y: i32,
    tile: i32,
    spacing: i32,
    columns: usize,
    len: usize,
) -> Option<usize> {
    if tile <= 0 || x < 0 || y < 0 {
        return None;
    }
    let stride = tile + spacing.max(0);
    let col = x / stride;
    let row = y / stride;
    if x - col * stride >= tile || y - row * stride >= tile {
        return None; // inside a spacing gap
    }
    let columns = columns.max(1) as i32;
    if col < 0 || col >= columns {
        return None;
    }
    let index = (row * columns + col) as usize;
    (index < len).then_some(index)
}

/// Moves `current` one step in `direction` over `len` tiles laid out at
/// `columns` per row, wrapping at every edge: `Left`/`Right` wrap around the
/// whole model, `Up`/`Down` wrap to the same column in the last/first row
/// (clamped onto that row when it is shorter than `columns`).
pub(super) fn navigate(len: usize, columns: usize, current: usize, direction: Direction) -> usize {
    if len == 0 {
        return 0;
    }
    let columns = columns.max(1);
    let current = current.min(len - 1);
    match direction {
        Direction::Left => {
            if current == 0 {
                len - 1
            } else {
                current - 1
            }
        }
        Direction::Right => {
            if current + 1 >= len {
                0
            } else {
                current + 1
            }
        }
        Direction::Up => {
            if current >= columns {
                current - columns
            } else {
                wrap_vertical(
                    len,
                    columns,
                    current,
                    row_count(len, columns).saturating_sub(1),
                )
            }
        }
        Direction::Down => {
            let last_row = row_count(len, columns).saturating_sub(1);
            let current_row = current / columns;
            if current_row < last_row {
                (current + columns).min(len - 1)
            } else {
                wrap_vertical(len, columns, current, 0)
            }
        }
    }
}

/// The tile in `target_row`, same column as `current`, clamped onto that row
/// when it runs short of `columns` tiles.
fn wrap_vertical(len: usize, columns: usize, current: usize, target_row: usize) -> usize {
    let col = current % columns;
    (target_row * columns + col).min(len - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_fit_the_viewport() {
        assert_eq!(columns_for_width(400, 100, 8), 3); // 3*100+2*8=316<=400, 4th needs 424
        assert_eq!(columns_for_width(50, 100, 8), 1); // never zero
        assert_eq!(columns_for_width(400, 0, 8), 1); // no divide by zero
    }

    #[test]
    fn content_height_covers_every_row() {
        assert_eq!(content_height_px(0, 4, 100, 8), 0);
        assert_eq!(content_height_px(4, 4, 100, 8), 100); // one row, no trailing gap
        assert_eq!(content_height_px(5, 4, 100, 8), 208); // two rows: 2*108-8
    }

    #[test]
    fn visible_rows_match_the_scrolled_viewport() {
        // 10 rows of 108px stride (100 tile + 8 spacing); scrolled 250px into a
        // 300px-tall viewport should show rows 2..6 (rows start at 216, 324...).
        let rows = visible_row_range(250, 300, 100, 8, 10);
        assert_eq!(rows, 2..6);
    }

    #[test]
    fn visible_rows_clamp_to_the_model() {
        let rows = visible_row_range(0, 10_000, 100, 8, 3);
        assert_eq!(rows, 0..3);
    }

    #[test]
    fn visible_items_span_only_the_visible_rows() {
        // 4 columns, 10 items -> 3 rows; scrolled to show only the middle row.
        let items = visible_item_range(108, 100, 100, 8, 4, 10);
        assert_eq!(items, 4..8);
    }

    #[test]
    fn empty_or_zero_height_viewport_shows_nothing() {
        assert_eq!(visible_item_range(0, 0, 100, 8, 4, 10), 0..0);
        assert_eq!(visible_item_range(0, 300, 100, 8, 4, 0), 0..0);
    }

    #[test]
    fn hit_test_finds_the_tile_and_skips_gaps() {
        assert_eq!(index_at_point(50, 50, 100, 8, 3, 10), Some(0));
        assert_eq!(index_at_point(105, 50, 100, 8, 3, 10), None); // in the gap
        assert_eq!(index_at_point(120, 50, 100, 8, 3, 10), Some(1));
        assert_eq!(index_at_point(50, 50, 100, 8, 3, 0), None); // past the model
        assert_eq!(index_at_point(-1, 0, 100, 8, 3, 10), None);
    }

    #[test]
    fn left_right_wrap_around_the_whole_model() {
        assert_eq!(navigate(6, 3, 0, Direction::Left), 5);
        assert_eq!(navigate(6, 3, 5, Direction::Right), 0);
        assert_eq!(navigate(6, 3, 2, Direction::Right), 3);
        assert_eq!(navigate(6, 3, 3, Direction::Left), 2);
    }

    #[test]
    fn up_down_move_a_row() {
        // 3 columns, 8 items -> rows [0,1,2] [3,4,5] [6,7].
        assert_eq!(navigate(8, 3, 4, Direction::Up), 1);
        assert_eq!(navigate(8, 3, 1, Direction::Down), 4);
    }

    #[test]
    fn up_wraps_to_the_last_row_same_column_clamped() {
        // rows [0,1,2] [3,4,5] [6,7]; the last row has no column 2.
        assert_eq!(navigate(8, 3, 1, Direction::Up), 7); // column 1 exists there
        assert_eq!(navigate(8, 3, 2, Direction::Up), 7); // column 2 clamps to the row's end
    }

    #[test]
    fn down_wraps_to_the_top_row_same_column() {
        assert_eq!(navigate(8, 3, 7, Direction::Down), 1); // row 2 col 1 -> row 0 col 1
        assert_eq!(navigate(8, 3, 6, Direction::Down), 0);
    }

    #[test]
    fn navigation_is_a_no_op_on_an_empty_model() {
        assert_eq!(navigate(0, 3, 0, Direction::Right), 0);
    }
}
