//! A bounded least-recently-used cache of measured widths.

use std::collections::HashMap;

/// Widths keyed by string, evicting the least recently used half in one sweep
/// when full, so a miss costs O(1) amortised and a hit allocates nothing.
pub(super) struct WidthCache {
    entries: HashMap<Box<str>, Entry>,
    clock: u64,
    capacity: usize,
}

struct Entry {
    width: f32,
    last_used: u64,
}

impl WidthCache {
    pub(super) fn new(capacity: usize) -> WidthCache {
        WidthCache {
            entries: HashMap::with_capacity(capacity),
            clock: 0,
            capacity,
        }
    }

    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(super) fn get(&mut self, text: &str) -> Option<f32> {
        self.clock += 1;
        let entry = self.entries.get_mut(text)?;
        entry.last_used = self.clock;
        Some(entry.width)
    }

    pub(super) fn insert(&mut self, text: &str, width: f32) {
        if self.entries.len() >= self.capacity {
            self.evict_older_half();
        }
        self.clock += 1;
        self.entries.insert(
            text.into(),
            Entry {
                width,
                last_used: self.clock,
            },
        );
    }

    fn evict_older_half(&mut self) {
        let mut stamps: Vec<u64> = self.entries.values().map(|e| e.last_used).collect();
        let middle = stamps.len() / 2;
        let (_, &mut cutoff, _) = stamps.select_nth_unstable(middle);
        self.entries.retain(|_, entry| entry.last_used >= cutoff);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hits_return_the_stored_width() {
        let mut cache = WidthCache::new(4);
        assert_eq!(cache.get("a"), None);
        cache.insert("a", 1.5);
        assert_eq!(cache.get("a"), Some(1.5));
    }

    #[test]
    fn stays_within_capacity() {
        let mut cache = WidthCache::new(8);
        for n in 0..100 {
            cache.insert(&n.to_string(), n as f32);
            assert!(cache.len() <= 8);
        }
    }

    #[test]
    fn evicts_the_least_recently_used() {
        let mut cache = WidthCache::new(4);
        for key in ["a", "b", "c", "d"] {
            cache.insert(key, 1.0);
        }
        // Touch "a" so it is the newest of the old entries.
        cache.get("a");
        cache.insert("e", 1.0);
        assert!(cache.get("a").is_some(), "the recently used entry survives");
        assert!(cache.get("b").is_none(), "the oldest entry is evicted");
        assert!(cache.get("e").is_some());
    }
}
