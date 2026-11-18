//! Least-recently-used pool of encoded tile payloads awaiting GPU upload.
//!
//! The pool bounds the memory the render loop pins between frames. It stores
//! the encoded image bytes the [`crate::tile_scheduler::TileScheduler`] fetched;
//! the renderer reads an entry, uploads it to device memory, and lets the
//! recency order decide which payloads leave first.

use crate::tile_key::TileKey;
use std::collections::HashMap;

struct PoolEntry {
    encoded_image_bytes: Vec<u8>,
    last_accessed_recency: u64,
    /// Monotonic stamp distinguishing this payload's contents from every
    /// payload ever stored under the same tile key. The GPU texture cache
    /// (a native-only module behind the same feature gate as the headless
    /// renderer) compares this stamp against its own
    /// entries to detect payloads whose bytes were replaced or reinserted —
    /// those invalidate any uploaded texture derived from the old bytes.
    payload_revision: u64,
}

/// Byte-bounded LRU pool of tile payloads.
pub struct TileTexturePool {
    maximum_stored_bytes: u64,
    stored_bytes: u64,
    recency_counter: u64,
    /// Monotonic source of payload revisions: every insert or replace stamp
    /// the next value, so a payload's revision is unique among all payloads
    /// the pool ever held.
    revision_counter: u64,
    entries: HashMap<TileKey, PoolEntry>,
}

impl TileTexturePool {
    /// Creates an empty pool bounded by `maximum_stored_bytes`.
    pub fn new(maximum_stored_bytes: u64) -> Self {
        Self {
            maximum_stored_bytes,
            stored_bytes: 0,
            recency_counter: 0,
            revision_counter: 0,
            entries: HashMap::new(),
        }
    }

    /// Inserts or replaces a payload, evicting least-recently-used entries
    /// until the budget is honored. A single payload larger than the budget is
    /// still kept — dropping the only visible tile would flash the map rather
    /// than save the memory.
    pub fn insert(&mut self, tile_key: TileKey, encoded_image_bytes: Vec<u8>) {
        let payload_bytes = encoded_image_bytes.len() as u64;
        self.revision_counter += 1;
        if let Some(previous_entry) = self.entries.insert(
            tile_key,
            PoolEntry {
                encoded_image_bytes,
                last_accessed_recency: 0,
                payload_revision: self.revision_counter,
            },
        ) {
            self.stored_bytes -= previous_entry.encoded_image_bytes.len() as u64;
        }
        self.stored_bytes += payload_bytes;
        self.touch(&tile_key);
        while self.stored_bytes > self.maximum_stored_bytes && self.entries.len() > 1 {
            let eviction_candidate = self
                .entries
                .iter()
                .min_by_key(|(_, pool_entry)| pool_entry.last_accessed_recency)
                .map(|(tile_key, _)| *tile_key);
            match eviction_candidate {
                Some(candidate) => {
                    if let Some(evicted_entry) = self.entries.remove(&candidate) {
                        self.stored_bytes -= evicted_entry.encoded_image_bytes.len() as u64;
                    }
                }
                None => break,
            }
        }
    }

    /// Borrows the stored payload, refreshing its recency.
    pub fn lookup(&mut self, tile_key: &TileKey) -> Option<&[u8]> {
        if !self.entries.contains_key(tile_key) {
            return None;
        }
        self.touch(tile_key);
        self.entries
            .get(tile_key)
            .map(|pool_entry| pool_entry.encoded_image_bytes.as_slice())
    }

    /// Refreshes the payload's recency without returning its bytes; reports
    /// whether the payload is pooled. Consumers reusing derived state (an
    /// already-uploaded texture read from the GPU cache) count as a use of
    /// the payload too — without this, LRU eviction would target exactly the
    /// steady-camera tiles whose cache hits let their lookups lapse.
    pub fn refresh_recency(&mut self, tile_key: &TileKey) -> bool {
        let is_pooled = self.entries.contains_key(tile_key);
        if is_pooled {
            self.touch(tile_key);
        }
        is_pooled
    }

    /// Reports whether the payload is pooled without touching recency.
    pub fn contains(&self, tile_key: &TileKey) -> bool {
        self.entries.contains_key(tile_key)
    }

    /// The revision stamp of the pooled payload, without touching recency, or
    /// `None` when the payload is not pooled. A different stamp means the
    /// bytes under this tile key changed — consumers holding derived state
    /// (an uploaded GPU texture) must regenerate it.
    pub fn payload_revision(&self, tile_key: &TileKey) -> Option<u64> {
        self.entries
            .get(tile_key)
            .map(|pool_entry| pool_entry.payload_revision)
    }

    /// Removes one payload from the pool, releasing its bytes; reports whether
    /// it was pooled. The renderer evicts payloads that fail to decode, so the
    /// scheduler's next fetch replaces the corrupt bytes instead of the pool
    /// poisoning every later frame with them.
    pub fn evict(&mut self, tile_key: &TileKey) -> bool {
        match self.entries.remove(tile_key) {
            Some(pool_entry) => {
                self.stored_bytes -= pool_entry.encoded_image_bytes.len() as u64;
                true
            }
            None => false,
        }
    }

    /// Total bytes currently pooled.
    pub fn stored_bytes(&self) -> u64 {
        self.stored_bytes
    }

    /// Number of pooled payloads.
    pub fn stored_payload_count(&self) -> usize {
        self.entries.len()
    }

    fn touch(&mut self, tile_key: &TileKey) {
        self.recency_counter += 1;
        if let Some(pool_entry) = self.entries.get_mut(tile_key) {
            pool_entry.last_accessed_recency = self.recency_counter;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TileTexturePool;
    use crate::tile_key::TileKey;

    fn tile_key(tile_x: u32) -> TileKey {
        TileKey::new(tile_x, 0, 10).unwrap()
    }

    #[test]
    fn evicts_the_least_recently_used_payload() {
        let mut payload_pool = TileTexturePool::new(2500);
        let first = tile_key(1);
        let second = tile_key(2);
        let third = tile_key(3);
        payload_pool.insert(first, vec![7; 1000]);
        payload_pool.insert(second, vec![7; 1000]);
        // Refresh the first payload so the second becomes the eviction victim.
        assert!(payload_pool.lookup(&first).is_some());
        payload_pool.insert(third, vec![7; 1000]);
        assert!(payload_pool.contains(&first));
        assert!(!payload_pool.contains(&second));
        assert!(payload_pool.contains(&third));
        assert_eq!(payload_pool.stored_bytes(), 2000);
    }

    #[test]
    fn replacing_a_payload_releases_its_previous_bytes() {
        let mut payload_pool = TileTexturePool::new(5000);
        let key = tile_key(1);
        payload_pool.insert(key, vec![7; 4000]);
        payload_pool.insert(key, vec![7; 1000]);
        assert_eq!(payload_pool.stored_bytes(), 1000);
        assert_eq!(payload_pool.stored_payload_count(), 1);
    }

    #[test]
    fn an_oversized_payload_is_kept_alone() {
        let mut payload_pool = TileTexturePool::new(1000);
        let oversize = tile_key(1);
        let small = tile_key(2);
        payload_pool.insert(oversize, vec![7; 4000]);
        assert!(payload_pool.contains(&oversize));
        payload_pool.insert(small, vec![7; 10]);
        assert!(!payload_pool.contains(&oversize));
        assert!(payload_pool.contains(&small));
        assert_eq!(payload_pool.stored_bytes(), 10);
    }

    #[test]
    fn lookups_return_the_stored_bytes() {
        let mut payload_pool = TileTexturePool::new(1000);
        let key = tile_key(5);
        payload_pool.insert(key, vec![1, 2, 3]);
        assert_eq!(payload_pool.lookup(&key), Some(&[1u8, 2, 3][..]));
        assert!(payload_pool.lookup(&tile_key(6)).is_none());
    }

    #[test]
    fn evicting_a_payload_releases_its_bytes() {
        let mut payload_pool = TileTexturePool::new(10_000);
        let pooled_key = tile_key(1);
        let absent_key = tile_key(2);
        payload_pool.insert(pooled_key, vec![7; 1000]);
        assert!(payload_pool.contains(&pooled_key));
        assert!(payload_pool.evict(&pooled_key));
        assert!(!payload_pool.contains(&pooled_key));
        assert_eq!(payload_pool.stored_bytes(), 0);
        assert_eq!(payload_pool.stored_payload_count(), 0);
        // Evicting an absent payload is a no-op.
        assert!(!payload_pool.evict(&absent_key));
        // A reinserted payload is fresh; the evicted bytes are gone.
        payload_pool.insert(pooled_key, vec![7; 10]);
        assert_eq!(payload_pool.stored_bytes(), 10);
    }

    #[test]
    fn every_insert_stamps_a_fresh_payload_revision() {
        let mut payload_pool = TileTexturePool::new(10_000);
        let replaced_key = tile_key(1);
        let absent_key = tile_key(2);
        assert_eq!(
            payload_pool.payload_revision(&absent_key),
            None,
            "an absent payload has no revision"
        );
        payload_pool.insert(replaced_key, vec![7; 10]);
        let first_revision = payload_pool.payload_revision(&replaced_key);
        payload_pool.insert(replaced_key, vec![9; 10]);
        let second_revision = payload_pool.payload_revision(&replaced_key);
        assert_ne!(
            first_revision, second_revision,
            "replacing a payload must invalidate consumers of the old bytes"
        );
        payload_pool.insert(tile_key(3), vec![7; 10]);
        assert_ne!(
            second_revision,
            payload_pool.payload_revision(&tile_key(3)),
            "distinct payloads never share a revision"
        );
        assert!(payload_pool.contains(&replaced_key));
        payload_pool.evict(&replaced_key);
        assert_eq!(payload_pool.payload_revision(&replaced_key), None);
    }
}
