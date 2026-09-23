#![forbid(unsafe_code)]

//! A bounded, per-thread cache of solid brushes and pens.
//!
//! [`Canvas`](super::Canvas) draws with solid colours constantly (fills,
//! outlines, separators, sort arrows). Creating and deleting an `HBRUSH`/`HPEN`
//! per call is wasteful and shows up in the process's GDI-object count under a
//! busy paint. This module keeps the most recently used objects alive, keyed by
//! colour (and width), and deletes the least recently used one once the bound is
//! reached, so the count stays flat instead of growing with the number of
//! distinct colours.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::Hash;

use windows::Win32::Graphics::Gdi::{HBRUSH, HPEN};

use crate::color::Color;

use super::{Brush, Pen};

/// The most brushes, and the most pens, kept alive per thread.
const CAPACITY: usize = 64;

/// A key for a solid pen: colour plus pixel width.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct PenKey {
    color: Color,
    width: i32,
}

/// A least-recently-used map with a hard capacity.
struct Lru<K, V> {
    entries: HashMap<K, Slot<V>>,
    capacity: usize,
    tick: u64,
}

struct Slot<V> {
    value: V,
    used: u64,
}

impl<K: Copy + Eq + Hash, V> Lru<K, V> {
    fn new(capacity: usize) -> Lru<K, V> {
        Lru {
            entries: HashMap::new(),
            capacity,
            tick: 0,
        }
    }

    /// Whether `key` is present, without changing its recency.
    fn contains(&self, key: K) -> bool {
        self.entries.contains_key(&key)
    }

    /// Returns the value for `key`, marking it most recently used.
    fn get(&mut self, key: K) -> Option<&V> {
        self.tick += 1;
        let used = self.tick;
        let slot = self.entries.get_mut(&key)?;
        slot.used = used;
        Some(&slot.value)
    }

    /// Inserts `value`, evicting and returning the least recently used entry if
    /// the map is already at capacity.
    fn insert(&mut self, key: K, value: V) -> Option<V> {
        self.tick += 1;
        let evicted = if self.entries.len() >= self.capacity && !self.entries.contains_key(&key) {
            self.evict()
        } else {
            None
        };
        self.entries.insert(
            key,
            Slot {
                value,
                used: self.tick,
            },
        );
        evicted
    }

    /// Removes the least recently used entry and returns its value.
    fn evict(&mut self) -> Option<V> {
        let key = self
            .entries
            .iter()
            .min_by_key(|(_, slot)| slot.used)
            .map(|(key, _)| *key)?;
        self.entries.remove(&key).map(|slot| slot.value)
    }

    /// The number of live entries.
    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

/// The current thread's live solid brushes and pens. Evicted (and dropped)
/// values release their GDI object through the RAII handles.
struct SolidCache {
    brushes: Lru<Color, Brush>,
    pens: Lru<PenKey, Pen>,
}

impl SolidCache {
    fn new() -> SolidCache {
        SolidCache {
            brushes: Lru::new(CAPACITY),
            pens: Lru::new(CAPACITY),
        }
    }

    fn brush(&mut self, color: Color) -> Option<HBRUSH> {
        if !self.brushes.contains(color) {
            self.brushes.insert(color, Brush::solid(color).ok()?);
        }
        self.brushes.get(color).map(Brush::raw)
    }

    fn pen(&mut self, color: Color, width: i32) -> Option<HPEN> {
        let key = PenKey { color, width };
        if !self.pens.contains(key) {
            self.pens.insert(key, Pen::new(color, width).ok()?);
        }
        self.pens.get(key).map(Pen::raw)
    }
}

thread_local! {
    /// Win32 GDI objects are thread-affine, so a per-thread cache needs no
    /// locking.
    static SOLID: RefCell<SolidCache> = RefCell::new(SolidCache::new());
}

/// Returns a cached solid brush of `color`, creating it on first use.
pub(crate) fn solid_brush(color: Color) -> Option<HBRUSH> {
    SOLID.with(|cache| cache.borrow_mut().brush(color))
}

/// Returns a cached cosmetic pen of `color` and `width`, creating it on first
/// use.
pub(crate) fn pen(color: Color, width: i32) -> Option<HPEN> {
    SOLID.with(|cache| cache.borrow_mut().pen(color, width))
}

#[cfg(test)]
mod tests {
    use super::{CAPACITY, Lru};

    #[test]
    fn stays_bounded() {
        let mut lru: Lru<u32, u32> = Lru::new(CAPACITY);
        for key in 0..(CAPACITY as u32 * 4) {
            lru.insert(key, key);
        }
        assert_eq!(lru.len(), CAPACITY);
        // The oldest keys were evicted; the newest survived.
        assert!(lru.get(0).is_none());
        assert_eq!(
            lru.get(CAPACITY as u32 * 4 - 1),
            Some(&(CAPACITY as u32 * 4 - 1))
        );
    }

    #[test]
    fn evicts_least_recently_used() {
        let mut lru: Lru<u32, u32> = Lru::new(2);
        lru.insert(1, 1);
        lru.insert(2, 2);
        assert_eq!(lru.get(1), Some(&1)); // 1 is now more recent than 2
        lru.insert(3, 3); // evicts 2
        assert_eq!(lru.get(1), Some(&1));
        assert!(lru.get(2).is_none());
        assert_eq!(lru.get(3), Some(&3));
    }
}
