#![forbid(unsafe_code)]

//! Pure geometry for the material top bar: turning a row of item spans into
//! rectangles and hit-testing a point against them. No Win32 calls, so the
//! arithmetic is unit-tested without a window.

use crate::geometry::{Point, Rect};

/// How much room an item claims in the row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Span {
    /// A fixed width in device-independent pixels.
    Fixed(f32),
    /// Absorbs an equal share of the row's leftover width.
    Fill,
}

/// Lays `spans` out left to right across `left..right` (device-independent
/// pixels). Fixed spans take their width; fill spans split the leftover equally
/// (zero when it is negative, so a crowded row never overflows). Returns one
/// `(left, width)` per span, in order.
pub(crate) fn distribute(spans: &[Span], left: f32, right: f32) -> Vec<(f32, f32)> {
    let fixed: f32 = spans
        .iter()
        .map(|span| match span {
            Span::Fixed(width) => *width,
            Span::Fill => 0.0,
        })
        .sum();
    let fills = spans
        .iter()
        .filter(|span| matches!(span, Span::Fill))
        .count();
    let share = if fills == 0 {
        0.0
    } else {
        ((right - left - fixed) / fills as f32).max(0.0)
    };
    let mut x = left;
    spans
        .iter()
        .map(|span| {
            let width = match span {
                Span::Fixed(width) => *width,
                Span::Fill => share,
            };
            let placed = (x, width);
            x += width;
            placed
        })
        .collect()
}

/// The index of the item whose rectangle contains `point`, if any.
pub(crate) fn hit(rects: &[Rect], point: Point) -> Option<usize> {
    rects.iter().position(|rect| rect.contains(point))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_items_are_placed_left_to_right() {
        let placed = distribute(&[Span::Fixed(20.0), Span::Fixed(30.0)], 0.0, 200.0);
        assert_eq!(placed, vec![(0.0, 20.0), (20.0, 30.0)]);
    }

    #[test]
    fn a_fill_span_pushes_the_rest_to_the_right() {
        let placed = distribute(
            &[Span::Fixed(20.0), Span::Fill, Span::Fixed(30.0)],
            0.0,
            200.0,
        );
        assert_eq!(placed[0], (0.0, 20.0));
        assert_eq!(placed[1], (20.0, 150.0));
        assert_eq!(placed[2], (170.0, 30.0));
    }

    #[test]
    fn multiple_fills_share_the_leftover_equally() {
        let placed = distribute(
            &[Span::Fixed(10.0), Span::Fill, Span::Fill, Span::Fixed(10.0)],
            0.0,
            100.0,
        );
        assert_eq!(placed[1].1, 40.0);
        assert_eq!(placed[2].1, 40.0);
        assert_eq!(placed[2].0, 50.0);
    }

    #[test]
    fn a_crowded_row_gives_fills_zero_width() {
        let placed = distribute(&[Span::Fixed(80.0), Span::Fill], 0.0, 50.0);
        assert_eq!(placed[1], (80.0, 0.0));
    }

    #[test]
    fn hit_finds_the_item_under_a_point() {
        let rects = vec![Rect::new(0, 0, 20, 10), Rect::new(20, 0, 60, 10)];
        assert_eq!(hit(&rects, Point::new(5, 5)), Some(0));
        assert_eq!(hit(&rects, Point::new(40, 5)), Some(1));
        assert_eq!(hit(&rects, Point::new(80, 5)), None);
    }
}
