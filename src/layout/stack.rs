#![forbid(unsafe_code)]

//! Stacks: weighted slots along one axis.

use super::Insets;
use crate::geometry::Rect;
use crate::window::dpi_scale;

/// The axis a [`Stack`] lays its slots along.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StackDirection {
    /// Slots run left to right.
    #[default]
    Horizontal,
    /// Slots run top to bottom.
    Vertical,
}

/// One slot in a [`Stack`], sized in 96-DPI design units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StackSlot {
    /// Exactly `px` units.
    Fixed(i32),
    /// Shares the leftover space proportionally to `weight`.
    Fill(u32),
    /// At least `px` units, shrinking proportionally only when the parent is
    /// too small to honour every slot.
    Min(i32),
}

/// Lays slots along one axis, with spacing between them and optional margins.
///
/// The slots tile the parent's content exactly: leftover pixels are shared by
/// `Fill` slots with largest-remainder rounding, so there are no one-pixel gaps
/// and no overlap.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stack {
    direction: StackDirection,
    spacing: i32,
    insets: Insets,
    slots: Vec<StackSlot>,
}

impl Stack {
    /// A stack that lays its slots left to right.
    pub const fn horizontal() -> Stack {
        Stack {
            direction: StackDirection::Horizontal,
            spacing: 0,
            insets: Insets::new(0, 0, 0, 0),
            slots: Vec::new(),
        }
    }

    /// A stack that lays its slots top to bottom.
    pub const fn vertical() -> Stack {
        Stack {
            direction: StackDirection::Vertical,
            spacing: 0,
            insets: Insets::new(0, 0, 0, 0),
            slots: Vec::new(),
        }
    }

    /// Gap between adjacent slots, in design units.
    pub const fn spacing(mut self, spacing: i32) -> Stack {
        self.spacing = spacing;
        self
    }

    /// Margins inside the parent, in design units.
    pub const fn margins(mut self, insets: Insets) -> Stack {
        self.insets = insets;
        self
    }

    /// Adds a fixed-size slot.
    pub fn fixed(mut self, px: i32) -> Stack {
        self.slots.push(StackSlot::Fixed(px));
        self
    }

    /// Adds a weighted fill slot.
    pub fn fill(mut self, weight: u32) -> Stack {
        self.slots.push(StackSlot::Fill(weight));
        self
    }

    /// Adds a minimum-size slot.
    pub fn min(mut self, px: i32) -> Stack {
        self.slots.push(StackSlot::Min(px));
        self
    }

    /// Adds an explicit slot.
    pub fn push(mut self, slot: StackSlot) -> Stack {
        self.slots.push(slot);
        self
    }

    /// Splits `rect` into one rect per slot, in order, scaling to `dpi`.
    pub fn split(&self, rect: Rect, dpi: u32) -> Vec<Rect> {
        let count = self.slots.len();
        if count == 0 {
            return Vec::new();
        }

        let inner = self.insets.apply(rect, dpi);
        let (main_start, main_len, cross_start, cross_len) = match self.direction {
            StackDirection::Horizontal => (inner.left, inner.width(), inner.top, inner.height()),
            StackDirection::Vertical => (inner.top, inner.height(), inner.left, inner.width()),
        };

        let gaps = count as i32 - 1;
        let spacing = if gaps > 0 {
            dpi_scale(self.spacing, dpi).clamp(0, main_len / gaps)
        } else {
            0
        };
        let content = (main_len - spacing * gaps).max(0);

        let mut rigid: Vec<(usize, i32)> = Vec::new();
        let mut fills: Vec<(usize, u32)> = Vec::new();
        for (index, slot) in self.slots.iter().enumerate() {
            match *slot {
                StackSlot::Fixed(px) => rigid.push((index, dpi_scale(px, dpi).max(0))),
                StackSlot::Min(px) => rigid.push((index, dpi_scale(px, dpi).max(0))),
                StackSlot::Fill(weight) => fills.push((index, weight)),
            }
        }

        let mut sizes = vec![0i32; count];
        let rigid_total: i32 = rigid.iter().map(|&(_, size)| size).sum();
        if rigid_total <= content {
            for &(index, size) in &rigid {
                sizes[index] = size;
            }
            let weights: Vec<i32> = fills.iter().map(|&(_, weight)| weight as i32).collect();
            for (&(index, _), size) in fills
                .iter()
                .zip(distribute(content - rigid_total, &weights))
            {
                sizes[index] = size;
            }
        } else {
            let weights: Vec<i32> = rigid.iter().map(|&(_, size)| size).collect();
            for (&(index, _), size) in rigid.iter().zip(distribute(content, &weights)) {
                sizes[index] = size;
            }
        }

        let mut rects = Vec::with_capacity(count);
        let mut cursor = main_start;
        for &size in &sizes {
            let end = cursor + size;
            rects.push(match self.direction {
                StackDirection::Horizontal => {
                    Rect::new(cursor, cross_start, end, cross_start + cross_len)
                }
                StackDirection::Vertical => {
                    Rect::new(cross_start, cursor, cross_start + cross_len, end)
                }
            });
            cursor = end + spacing;
        }
        rects
    }
}

/// Splits `total` pixels between `weights` so the parts sum to exactly `total`.
///
/// Uses cumulative rounding: each slot gets the difference between successive
/// floored boundaries, which keeps every part within one pixel of its ideal
/// share and makes the parts tile the whole.
fn distribute(total: i32, weights: &[i32]) -> Vec<i32> {
    let count = weights.len();
    let mut sizes = vec![0; count];
    if count == 0 || total <= 0 {
        return sizes;
    }

    let mut weights: Vec<i64> = weights
        .iter()
        .map(|&weight| i64::from(weight.max(0)))
        .collect();
    let mut sum: i64 = weights.iter().sum();
    if sum == 0 {
        weights = vec![1; count];
        sum = count as i64;
    }

    let total = i64::from(total);
    let mut accumulated = 0i64;
    let mut assigned = 0i64;
    for (size, weight) in sizes.iter_mut().zip(&weights) {
        accumulated += weight * total;
        let boundary = accumulated / sum;
        *size = (boundary - assigned) as i32;
        assigned = boundary;
    }
    sizes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(rect: Rect) -> i64 {
        i64::from(rect.width()) * i64::from(rect.height())
    }

    fn assert_tiles(rects: &[Rect], parent: Rect) {
        let total: i64 = rects.iter().map(|&rect| area(rect)).sum();
        assert_eq!(total, area(parent), "slots must tile the parent");
        for rect in rects {
            assert!(rect.width() >= 0 && rect.height() >= 0);
        }
    }

    #[test]
    fn stack_horizontal_fills_proportionally() {
        let rect = Rect::new(0, 0, 100, 40);
        let slots = Stack::horizontal()
            .fixed(10)
            .fill(1)
            .fill(2)
            .split(rect, 96);
        assert_eq!(slots.len(), 3);
        assert_eq!(slots[0], Rect::new(0, 0, 10, 40));
        assert_eq!(slots[1], Rect::new(10, 0, 40, 40));
        assert_eq!(slots[2], Rect::new(40, 0, 100, 40));
        assert_tiles(&slots, rect);
    }

    #[test]
    fn stack_rounding_has_no_gaps() {
        let rect = Rect::new(0, 0, 100, 10);
        let slots = Stack::horizontal().fill(1).fill(1).fill(1).split(rect, 96);
        assert_eq!(slots[0], Rect::new(0, 0, 33, 10));
        assert_eq!(slots[1], Rect::new(33, 0, 66, 10));
        assert_eq!(slots[2], Rect::new(66, 0, 100, 10));
        assert_tiles(&slots, rect);
    }

    #[test]
    fn stack_applies_spacing_and_margins() {
        let rect = Rect::new(0, 0, 100, 100);
        let slots = Stack::vertical()
            .margins(Insets::all(4))
            .spacing(2)
            .min(20)
            .fill(1)
            .split(rect, 96);
        assert_eq!(slots[0], Rect::new(4, 4, 96, 24));
        assert_eq!(slots[1], Rect::new(4, 26, 96, 96));
        assert_eq!(slots[1].bottom - slots[0].top, 92);
    }

    #[test]
    fn stack_scales_to_dpi() {
        let rect = Rect::new(0, 0, 200, 100);
        let slots = Stack::horizontal().fixed(10).fill(1).split(rect, 192);
        assert_eq!(slots[0], Rect::new(0, 0, 20, 100));
        assert_eq!(slots[1], Rect::new(20, 0, 200, 100));
    }

    #[test]
    fn stack_overflow_shrinks_rigid_slots() {
        let rect = Rect::new(0, 0, 50, 10);
        let slots = Stack::horizontal().fixed(100).fixed(50).split(rect, 96);
        assert_eq!(slots[0], Rect::new(0, 0, 33, 10));
        assert_eq!(slots[1], Rect::new(33, 0, 50, 10));
        assert_tiles(&slots, rect);
    }

    #[test]
    fn stack_clamps_spacing_in_a_tiny_rect() {
        let rect = Rect::new(0, 0, 10, 10);
        let slots = Stack::horizontal()
            .spacing(4)
            .fill(1)
            .fill(1)
            .fill(1)
            .fill(1)
            .split(rect, 96);
        for pair in slots.windows(2) {
            assert!(pair[0].right <= pair[1].left);
        }
        assert_eq!(slots[0].left, 0);
        assert_eq!(slots.last().unwrap().right, 10);
    }

    #[test]
    fn distribute_is_exact() {
        assert_eq!(distribute(10, &[1, 1, 1]), vec![3, 3, 4]);
        assert_eq!(distribute(0, &[1, 2]), vec![0, 0]);
        assert_eq!(distribute(7, &[0, 0]), vec![3, 4]);
    }
}
