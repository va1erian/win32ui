#![forbid(unsafe_code)]

//! Pure geometry and mnemonic parsing for the strip menu. No Win32 calls, so
//! the arithmetic is unit-tested without a window.

use crate::geometry::{Point, Rect};

/// `label` with its mnemonic `&` markers removed (`&&` becomes a literal `&`).
pub(crate) fn without_mnemonics(label: &str) -> String {
    let mut out = String::with_capacity(label.len());
    let mut chars = label.chars();
    while let Some(ch) = chars.next() {
        if ch == '&' {
            match chars.next() {
                Some('&') => out.push('&'),
                Some(next) => out.push(next),
                None => {}
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// The mnemonic character of `label` (the character after the first lone `&`),
/// lowercased so it compares against a key regardless of the label's case.
pub(crate) fn mnemonic(label: &str) -> Option<char> {
    let mut chars = label.chars();
    while let Some(ch) = chars.next() {
        if ch == '&' {
            match chars.next() {
                Some('&') => continue,
                Some(next) => return Some(next.to_ascii_lowercase()),
                None => return None,
            }
        }
    }
    None
}

/// Lays `widths` (device pixels) out left-to-right in one menu row starting at
/// `start_x`; each item is `height` tall with `padding` on both sides, and items
/// are separated by `gap`. The row's top is `y` pixels below the strip's top.
/// Returns one rectangle per item.
pub(crate) fn layout_items(
    widths: &[i32],
    start_x: i32,
    y: i32,
    height: i32,
    padding: i32,
    gap: i32,
) -> Vec<Rect> {
    let mut x = start_x;
    let mut rects = Vec::with_capacity(widths.len());
    for width in widths {
        let rect = Rect::new(x, y, x + width + padding * 2, y + height);
        rects.push(rect);
        x = rect.right + gap;
    }
    rects
}

/// The index of the item whose rectangle contains `point`, if any.
pub(crate) fn hit(items: &[Rect], point: Point) -> Option<usize> {
    items.iter().position(|rect| rect.contains(point))
}

/// The index of the first enabled item in `enabled`, if any.
pub(crate) fn first_enabled(enabled: &[bool]) -> Option<usize> {
    enabled.iter().position(|on| *on)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mnemonics_are_found_and_stripped() {
        assert_eq!(without_mnemonics("&File"), "File");
        assert_eq!(without_mnemonics("A && B"), "A & B");
        assert_eq!(without_mnemonics("Plain"), "Plain");
        assert_eq!(mnemonic("&File"), Some('f'));
        assert_eq!(mnemonic("E&xit"), Some('x'));
        assert_eq!(mnemonic("A && B"), None, "&& is a literal ampersand");
        assert_eq!(mnemonic("Plain"), None);
        assert_eq!(mnemonic("trailing&"), None);
    }

    #[test]
    fn items_are_laid_out_left_to_right() {
        let rects = layout_items(&[20, 30], 4, 0, 28, 10, 2);
        assert_eq!(rects[0], Rect::new(4, 0, 44, 28));
        assert_eq!(rects[1], Rect::new(46, 0, 96, 28));
    }

    #[test]
    fn hit_finds_the_item_under_a_point() {
        let rects = layout_items(&[20, 30], 0, 0, 28, 10, 2);
        assert_eq!(hit(&rects, Point::new(5, 14)), Some(0));
        assert_eq!(hit(&rects, Point::new(50, 14)), Some(1));
        assert_eq!(
            hit(&rects, Point::new(41, 14)),
            None,
            "the gap is not an item"
        );
        assert_eq!(hit(&rects, Point::new(5, 30)), None, "below the row");
    }

    #[test]
    fn first_enabled_skips_disabled_leading_items() {
        assert_eq!(first_enabled(&[false, true, true]), Some(1));
        assert_eq!(first_enabled(&[false, false]), None);
    }
}
