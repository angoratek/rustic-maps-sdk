//! Exact tile payload cache hit and miss accounting over one shared store.
//!
//! The installed raster-tile wrapper and the vector base's payload cache read
//! the same response store: the settings surface's tile cache is the one
//! database both bases fetch through. [`TileCacheStatisticsCounters`] is the
//! single counter pair every view of that store records into, and
//! [`CountingCachedResponseStore`] is the store view that records it — both
//! bases consult `lookup_response`, so both count through the same seam.
//!
//! Style resources (the style document, glyph ranges and sprite sheets) also
//! live in the same database but are not tile payloads: the style client keeps
//! the plain store view, so those lookups never pollute the tile counters.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use geospatial_types::SDKError;
use rustic_maps_core::CachedResponseStore;

/// The shared tile payload cache counters every view of one store records
/// into.
///
/// Clone the [`Arc`] the handle lives behind to share one counter pair between
/// the cached raster wrapper, the vector base's payload cache and the
/// developer-console readback. A newly installed tile cache starts a new
/// handle, so the counters reset with the cache they describe.
#[derive(Debug, Default)]
pub struct TileCacheStatisticsCounters {
    /// Payload lookups the store answered without a provider request.
    hit_count: AtomicU64,
    /// Payload lookups the store could not answer.
    miss_count: AtomicU64,
}

impl TileCacheStatisticsCounters {
    /// Builds a zeroed counter pair shared behind an [`Arc`].
    pub fn shared() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Reads the counters without mutating them.
    pub fn statistics(&self) -> TileCacheStatistics {
        TileCacheStatistics {
            hit_count: self.hit_count.load(Ordering::Relaxed),
            miss_count: self.miss_count.load(Ordering::Relaxed),
        }
    }

    /// Records one lookup the store answered.
    fn record_hit(&self) {
        self.hit_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Records one lookup the store could not answer.
    fn record_miss(&self) {
        self.miss_count.fetch_add(1, Ordering::Relaxed);
    }
}

/// A snapshot of tile payload cache lookups: the exact hits and misses the
/// cache answered since it was installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TileCacheStatistics {
    /// Payload lookups the store answered without a provider request.
    pub hit_count: u64,
    /// Payload lookups the store could not answer, including a failed store
    /// read that degraded to a provider request.
    pub miss_count: u64,
}

/// A response store view that records every lookup into a shared counter pair.
///
/// It delegates storage unchanged: only the lookup is accounted, because that
/// is the point where a payload either came from the cache or did not.
pub struct CountingCachedResponseStore<D> {
    /// The store whose lookups are counted.
    response_store: Arc<dyn CachedResponseStore<D>>,
    /// The counters every view of the same store shares.
    counters: Arc<TileCacheStatisticsCounters>,
}

impl<D> CountingCachedResponseStore<D> {
    /// Wraps a store so its lookups count into `counters`.
    pub fn new(
        response_store: Arc<dyn CachedResponseStore<D>>,
        counters: Arc<TileCacheStatisticsCounters>,
    ) -> Self {
        Self {
            response_store,
            counters,
        }
    }
}

impl<D> CachedResponseStore<D> for CountingCachedResponseStore<D> {
    fn lookup_response(&self, request_fingerprint: &str) -> Result<Option<Arc<D>>, SDKError> {
        let lookup_result = self.response_store.lookup_response(request_fingerprint);
        match &lookup_result {
            Ok(Some(_)) => self.counters.record_hit(),
            // A store read failure degrades to a miss in the cache-then-network
            // policy, so it is counted as one here rather than hidden.
            Ok(None) | Err(_) => self.counters.record_miss(),
        }
        lookup_result
    }

    fn store_response(
        &self,
        request_fingerprint: &str,
        response_value: Arc<D>,
        time_to_live_seconds: u64,
    ) -> Result<(), SDKError> {
        self.response_store.store_response(
            request_fingerprint,
            response_value,
            time_to_live_seconds,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::Mutex;

    use geospatial_types::{CacheOperationFailureReason, SDKError};
    use rustic_maps_core::CachedResponseStore;

    use super::{CountingCachedResponseStore, TileCacheStatisticsCounters};

    /// An in-memory store that can be told to fail its lookups.
    #[derive(Default)]
    struct RecordingResponseStore {
        stored_by_fingerprint: Mutex<HashMap<String, Arc<String>>>,
        fails_lookups: Mutex<bool>,
    }

    impl RecordingResponseStore {
        fn failing_lookups() -> Arc<Self> {
            let store = Self::default();
            *store.fails_lookups.lock().unwrap() = true;
            Arc::new(store)
        }
    }

    impl CachedResponseStore<String> for RecordingResponseStore {
        fn lookup_response(
            &self,
            request_fingerprint: &str,
        ) -> Result<Option<Arc<String>>, SDKError> {
            if *self.fails_lookups.lock().unwrap() {
                return Err(SDKError::CacheOperationFailure(
                    CacheOperationFailureReason::RecordCorrupted,
                ));
            }
            Ok(self
                .stored_by_fingerprint
                .lock()
                .unwrap()
                .get(request_fingerprint)
                .cloned())
        }

        fn store_response(
            &self,
            request_fingerprint: &str,
            response_value: Arc<String>,
            _time_to_live_seconds: u64,
        ) -> Result<(), SDKError> {
            self.stored_by_fingerprint
                .lock()
                .unwrap()
                .insert(request_fingerprint.to_string(), response_value);
            Ok(())
        }
    }

    #[test]
    fn a_zeroed_counter_pair_reports_zero_hits_and_zero_misses() {
        let counters = TileCacheStatisticsCounters::shared();
        let statistics = counters.statistics();
        assert_eq!(statistics.hit_count, 0);
        assert_eq!(statistics.miss_count, 0);
    }

    #[test]
    fn a_stored_payload_counts_one_miss_then_one_hit() {
        let counters = TileCacheStatisticsCounters::shared();
        let store = Arc::new(RecordingResponseStore::default());
        let counting_store = CountingCachedResponseStore::new(store.clone(), Arc::clone(&counters));

        assert!(
            counting_store
                .lookup_response("tile-address")
                .unwrap()
                .is_none(),
            "an empty store answers no payload",
        );
        counting_store
            .store_response("tile-address", Arc::new("payload".to_string()), 60)
            .unwrap();
        assert!(
            counting_store
                .lookup_response("tile-address")
                .unwrap()
                .is_some(),
            "the stored payload answers the second lookup",
        );

        let statistics = counters.statistics();
        assert_eq!(statistics.miss_count, 1);
        assert_eq!(statistics.hit_count, 1);
    }

    #[test]
    fn a_failed_store_read_counts_as_a_miss() {
        let counters = TileCacheStatisticsCounters::shared();
        let counting_store = CountingCachedResponseStore::new(
            RecordingResponseStore::failing_lookups(),
            Arc::clone(&counters),
        );

        assert!(
            counting_store.lookup_response("tile-address").is_err(),
            "the store failure surfaces to the caller",
        );

        let statistics = counters.statistics();
        assert_eq!(
            statistics.miss_count, 1,
            "a failed read did not answer from the cache, so it is a miss",
        );
        assert_eq!(statistics.hit_count, 0);
    }

    #[test]
    fn every_view_of_one_counter_pair_shares_the_same_counts() {
        let counters = TileCacheStatisticsCounters::shared();
        let raster_store = CountingCachedResponseStore::new(
            Arc::new(RecordingResponseStore::default()),
            Arc::clone(&counters),
        );
        let vector_store = CountingCachedResponseStore::new(
            Arc::new(RecordingResponseStore::default()),
            Arc::clone(&counters),
        );

        raster_store.lookup_response("raster-tile").unwrap();
        vector_store.lookup_response("vector-tile").unwrap();
        vector_store.lookup_response("vector-tile").unwrap();

        let statistics = counters.statistics();
        assert_eq!(
            statistics.miss_count, 3,
            "both bases' misses land in the one cache counter pair",
        );
        assert_eq!(statistics.hit_count, 0);
    }

    #[test]
    fn a_fresh_counter_pair_resets_with_its_cache() {
        let first_counters = TileCacheStatisticsCounters::shared();
        let first_store = CountingCachedResponseStore::new(
            Arc::new(RecordingResponseStore::default()),
            Arc::clone(&first_counters),
        );
        first_store.lookup_response("tile-address").unwrap();
        assert_eq!(first_counters.statistics().miss_count, 1);

        // A newly installed cache builds a new pair; the previous cache's
        // counts do not carry over.
        let second_counters = TileCacheStatisticsCounters::shared();
        assert_eq!(second_counters.statistics().hit_count, 0);
        assert_eq!(second_counters.statistics().miss_count, 0);
    }
}
