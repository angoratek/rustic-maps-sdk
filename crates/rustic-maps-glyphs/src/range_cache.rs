//! A byte- and count-bounded least-recently-used cache for fetched glyph
//! ranges.
//!
//! Glyph ranges are immutable once fetched: a range response for one font
//! stack and one 256-codepoint block never changes. The cache therefore
//! stores decoded [`GlyphFontStack`] values under a
//! [`GlyphRangeKey`] that names the glyph source, the font stack and the
//! range, and evicts the least recently used entry whenever either the byte
//! ceiling or the entry-count ceiling would be exceeded. The ceiling is
//! honoured even when a single range is larger than the whole budget: the
//! range is not retained rather than silently overflowing the bound.

use crate::glyph::{GlyphFontStack, GlyphRange};
use std::collections::{HashMap, VecDeque};

/// The default byte ceiling: 4 MiB of retained glyph data.
pub const DEFAULT_MAXIMUM_CACHED_BYTE_COUNT: usize = 4 * 1024 * 1024;

/// The default entry ceiling: 64 cached ranges.
pub const DEFAULT_MAXIMUM_CACHED_RANGE_COUNT: usize = 64;

/// The bounds a [`GlyphRangeCache`] honours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlyphRangeCacheConfiguration {
    /// The most bytes the cache may retain, counted by
    /// [`GlyphFontStack::retained_byte_count`] plus the key's own bytes.
    pub maximum_cached_byte_count: usize,
    /// The most ranges the cache may retain.
    pub maximum_cached_range_count: usize,
}

impl Default for GlyphRangeCacheConfiguration {
    fn default() -> Self {
        Self {
            maximum_cached_byte_count: DEFAULT_MAXIMUM_CACHED_BYTE_COUNT,
            maximum_cached_range_count: DEFAULT_MAXIMUM_CACHED_RANGE_COUNT,
        }
    }
}

/// The identity of one cached glyph range.
///
/// `glyph_source` is the provider's namespace (its provider key, or the
/// glyph URL template itself); it keeps two providers that serve the same
/// font-stack name from sharing a range. `font_stack` is the font family list
/// the request names, and `range` is the 256-codepoint block.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GlyphRangeKey {
    /// The glyph source's stable namespace.
    pub glyph_source: String,
    /// The requested font stack's name.
    pub font_stack: String,
    /// The requested 256-codepoint block.
    pub range: GlyphRange,
}

impl GlyphRangeKey {
    /// Builds a range key.
    pub fn new(
        glyph_source: impl Into<String>,
        font_stack: impl Into<String>,
        range: GlyphRange,
    ) -> Self {
        Self {
            glyph_source: glyph_source.into(),
            font_stack: font_stack.into(),
            range,
        }
    }

    /// The bytes this key retains: its inline record plus its two owned
    /// strings.
    fn retained_byte_count(&self) -> usize {
        std::mem::size_of::<Self>() + self.glyph_source.len() + self.font_stack.len()
    }
}

/// One stored range and the byte count it contributes to the ceiling.
#[derive(Debug)]
struct CacheEntry {
    font_stack: GlyphFontStack,
    stored_byte_count: usize,
}

/// A least-recently-used cache of decoded glyph ranges under a byte ceiling
/// and an entry-count ceiling.
#[derive(Debug)]
pub struct GlyphRangeCache {
    configuration: GlyphRangeCacheConfiguration,
    entries: HashMap<GlyphRangeKey, CacheEntry>,
    /// Least recently used keys first, most recently used last.
    recency: VecDeque<GlyphRangeKey>,
    cached_byte_count: usize,
}

impl GlyphRangeCache {
    /// Builds an empty cache with the supplied bounds.
    pub fn new(configuration: GlyphRangeCacheConfiguration) -> Self {
        Self {
            configuration,
            entries: HashMap::new(),
            recency: VecDeque::new(),
            cached_byte_count: 0,
        }
    }

    /// The bounds this cache honours.
    pub fn configuration(&self) -> GlyphRangeCacheConfiguration {
        self.configuration
    }

    /// Returns the cached range and marks it as most recently used.
    pub fn get(&mut self, key: &GlyphRangeKey) -> Option<&GlyphFontStack> {
        if !self.entries.contains_key(key) {
            return None;
        }
        self.mark_most_recently_used(key);
        self.entries.get(key).map(|entry| &entry.font_stack)
    }

    /// Stores a range, replacing an existing entry for the same key, then
    /// evicts least recently used entries until both ceilings hold.
    ///
    /// A range whose own bytes exceed the byte ceiling is stored and then
    /// immediately evicted, so the cache never exceeds the configured bound.
    pub fn insert(&mut self, key: GlyphRangeKey, font_stack: GlyphFontStack) {
        let stored_byte_count = key.retained_byte_count() + font_stack.retained_byte_count();
        if let Some(previous_entry) = self.entries.remove(&key) {
            self.cached_byte_count -= previous_entry.stored_byte_count;
        }
        self.recency.retain(|existing_key| existing_key != &key);
        self.entries.insert(
            key.clone(),
            CacheEntry {
                font_stack,
                stored_byte_count,
            },
        );
        self.recency.push_back(key);
        self.cached_byte_count += stored_byte_count;
        self.evict_to_limits();
    }

    /// Removes one cached range, answering it when it was present.
    pub fn remove(&mut self, key: &GlyphRangeKey) -> Option<GlyphFontStack> {
        let entry = self.entries.remove(key)?;
        self.cached_byte_count -= entry.stored_byte_count;
        self.recency.retain(|existing_key| existing_key != key);
        Some(entry.font_stack)
    }

    /// Removes every cached range.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.recency.clear();
        self.cached_byte_count = 0;
    }

    /// The number of cached ranges.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache holds no ranges.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The bytes the cache currently retains, including keys.
    pub fn cached_byte_count(&self) -> usize {
        self.cached_byte_count
    }

    /// Moves an existing key to the most-recently-used end of the recency
    /// queue.
    fn mark_most_recently_used(&mut self, key: &GlyphRangeKey) {
        self.recency.retain(|existing_key| existing_key != key);
        self.recency.push_back(key.clone());
    }

    /// Evicts least recently used entries until both ceilings hold.
    fn evict_to_limits(&mut self) {
        while self.entries.len() > self.configuration.maximum_cached_range_count
            || self.cached_byte_count > self.configuration.maximum_cached_byte_count
        {
            let Some(oldest_key) = self.recency.pop_front() else {
                break;
            };
            if let Some(oldest_entry) = self.entries.remove(&oldest_key) {
                self.cached_byte_count -= oldest_entry.stored_byte_count;
            }
        }
    }
}

impl Default for GlyphRangeCache {
    fn default() -> Self {
        Self::new(GlyphRangeCacheConfiguration::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glyph::Glyph;

    /// A font stack carrying one glyph with the requested bitmap byte count.
    fn sample_font_stack(bitmap_byte_count: usize) -> GlyphFontStack {
        GlyphFontStack {
            name: "Noto Sans Regular".to_string(),
            range: "0-255".to_string(),
            glyphs: vec![Glyph {
                codepoint: 0x41,
                bitmap: vec![0x7f; bitmap_byte_count],
                width: 0,
                height: 0,
                left: 0,
                top: 0,
                advance: 0,
            }],
        }
    }

    /// A key for the sample font stack in the given range.
    fn sample_key(range: GlyphRange) -> GlyphRangeKey {
        GlyphRangeKey::new("test-provider", "Noto Sans Regular", range)
    }

    /// The byte cost of the sample key and stack pair.
    fn sample_cost(range: GlyphRange) -> usize {
        sample_key(range).retained_byte_count() + sample_font_stack(64).retained_byte_count()
    }

    #[test]
    fn returns_a_cached_range_and_tracks_recent_use() {
        let mut cache = GlyphRangeCache::new(GlyphRangeCacheConfiguration {
            maximum_cached_byte_count: usize::MAX,
            maximum_cached_range_count: 2,
        });
        let first_range = GlyphRange::containing(0);
        let second_range = GlyphRange::containing(0x100);
        let third_range = GlyphRange::containing(0x200);

        cache.insert(sample_key(first_range), sample_font_stack(16));
        cache.insert(sample_key(second_range), sample_font_stack(16));
        assert_eq!(cache.len(), 2);

        // Touching the first range makes the second one the eviction victim.
        assert!(cache.get(&sample_key(first_range)).is_some());
        cache.insert(sample_key(third_range), sample_font_stack(16));

        assert_eq!(cache.len(), 2);
        assert!(cache.get(&sample_key(first_range)).is_some());
        assert!(cache.get(&sample_key(second_range)).is_none());
        assert!(cache.get(&sample_key(third_range)).is_some());
    }

    #[test]
    fn respects_the_byte_ceiling_and_evicts_the_oldest_range() {
        let first_range = GlyphRange::containing(0);
        let second_range = GlyphRange::containing(0x100);
        let third_range = GlyphRange::containing(0x200);
        let maximum_cached_byte_count = sample_cost(first_range) * 2;
        let mut cache = GlyphRangeCache::new(GlyphRangeCacheConfiguration {
            maximum_cached_byte_count,
            maximum_cached_range_count: usize::MAX,
        });

        cache.insert(sample_key(first_range), sample_font_stack(64));
        cache.insert(sample_key(second_range), sample_font_stack(64));
        cache.insert(sample_key(third_range), sample_font_stack(64));

        assert!(cache.cached_byte_count() <= maximum_cached_byte_count);
        assert_eq!(cache.len(), 2);
        assert!(cache.get(&sample_key(first_range)).is_none());
        assert!(cache.get(&sample_key(second_range)).is_some());
        assert!(cache.get(&sample_key(third_range)).is_some());
    }

    #[test]
    fn respects_the_range_count_ceiling() {
        let mut cache = GlyphRangeCache::new(GlyphRangeCacheConfiguration {
            maximum_cached_byte_count: usize::MAX,
            maximum_cached_range_count: 2,
        });
        let ranges = [
            GlyphRange::containing(0),
            GlyphRange::containing(0x100),
            GlyphRange::containing(0x200),
        ];
        for range in ranges {
            cache.insert(sample_key(range), sample_font_stack(8));
        }

        assert_eq!(cache.len(), 2);
        assert!(cache.get(&sample_key(ranges[0])).is_none());
        assert!(cache.get(&sample_key(ranges[1])).is_some());
        assert!(cache.get(&sample_key(ranges[2])).is_some());
    }

    #[test]
    fn a_range_larger_than_the_byte_ceiling_is_not_retained() {
        let range = GlyphRange::containing(0);
        let mut cache = GlyphRangeCache::new(GlyphRangeCacheConfiguration {
            maximum_cached_byte_count: sample_cost(range) - 1,
            maximum_cached_range_count: usize::MAX,
        });

        cache.insert(sample_key(range), sample_font_stack(64));

        assert!(cache.is_empty());
        assert_eq!(cache.cached_byte_count(), 0);
    }

    #[test]
    fn reinserting_the_same_key_replaces_without_double_counting() {
        let range = GlyphRange::containing(0);
        let mut cache = GlyphRangeCache::default();

        cache.insert(sample_key(range), sample_font_stack(16));
        cache.insert(sample_key(range), sample_font_stack(32));

        assert_eq!(cache.len(), 1);
        let expected =
            sample_key(range).retained_byte_count() + sample_font_stack(32).retained_byte_count();
        assert_eq!(cache.cached_byte_count(), expected);
    }

    #[test]
    fn remove_and_clear_release_the_counted_bytes() {
        let first_range = GlyphRange::containing(0);
        let second_range = GlyphRange::containing(0x100);
        let mut cache = GlyphRangeCache::default();
        cache.insert(sample_key(first_range), sample_font_stack(16));
        cache.insert(sample_key(second_range), sample_font_stack(16));

        assert!(cache.remove(&sample_key(first_range)).is_some());
        assert!(cache.remove(&sample_key(first_range)).is_none());
        assert_eq!(cache.len(), 1);

        cache.clear();
        assert!(cache.is_empty());
        assert_eq!(cache.cached_byte_count(), 0);
    }
}
