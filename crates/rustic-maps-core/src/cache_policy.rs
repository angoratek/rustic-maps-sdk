//! Cache-then-network orchestration around any provider call.
//!
//! The caller supplies a [`CachedResponseStore`] for persistence and a future
//! that performs the network call. Values travel as typed records end to end,
//! so the orchestration path never handles untyped JSON.

use std::future::Future;
use std::sync::Arc;

use geospatial_types::SDKError;

use crate::blocking_work::{BlockingWorkExecutor, run_blocking};

/// Where a cache-then-network execution took its value from.
///
/// Callers use this for freshness accounting, such as a staleness indicator in
/// a user interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheThenNetworkOutcome {
    /// The value came from the cache and no network call happened.
    CacheHit,
    /// The cache missed (or the entry was expired) and the network served.
    NetworkFetch,
}

/// Storage and retrieval of deserialized provider responses, keyed by request
/// fingerprint.
///
/// `D` is the typed record the provider returns, for example
/// `Vec<PlaceResult>`. Implementations must be safe to share across threads.
pub trait CachedResponseStore<D>: Send + Sync {
    /// Looks up a cached value for a request fingerprint.
    ///
    /// # Parameters
    /// - `request_fingerprint` — the stable key identifying the request.
    ///
    /// # Returns
    /// The stored value, or `None` when no entry exists or the entry has
    /// expired.
    ///
    /// # Errors
    /// Returns [`SDKError::CacheOperationFailure`] when the store cannot be
    /// read.
    fn lookup_response(&self, request_fingerprint: &str) -> Result<Option<Arc<D>>, SDKError>;

    /// Stores a value under a request fingerprint.
    ///
    /// # Parameters
    /// - `request_fingerprint` — the stable key identifying the request.
    /// - `response_value` — the value to store.
    /// - `time_to_live_seconds` — how long the entry stays valid.
    ///
    /// # Errors
    /// Returns [`SDKError::CacheOperationFailure`] when the store cannot be
    /// written.
    fn store_response(
        &self,
        request_fingerprint: &str,
        response_value: Arc<D>,
        time_to_live_seconds: u64,
    ) -> Result<(), SDKError>;
}

/// Executes the cache-then-network policy.
///
/// The store is consulted first. On a hit, the cached value is returned. On a
/// miss, `network_fetch` is awaited once, the result is stored for
/// `time_to_live_seconds`, and the fresh value is returned.
///
/// The store's synchronous lookups and writes run through
/// `blocking_work_executor`, so a store that blocks (SQLite does) never blocks
/// the async reactor the network fetch runs on.
///
/// # Parameters
/// - `blocking_work_executor` — where the store's synchronous calls run.
/// - `response_store` — the store consulted before and written after the
///   network call.
/// - `request_fingerprint` — the stable key identifying the request.
/// - `time_to_live_seconds` — how long the stored value stays valid.
/// - `network_fetch` — called once, and only on a cache miss.
///
/// # Returns
/// The value and [`CacheThenNetworkOutcome::CacheHit`] or
/// [`CacheThenNetworkOutcome::NetworkFetch`], saying where it came from.
///
/// # Errors
/// Returns the network fetch's [`SDKError`] when the fetch fails. A cache
/// store is an accelerator, never a gate: a failed lookup degrades to a miss
/// and a failed write degrades to cache-less operation, so neither fails a
/// healthy request.
///
/// # Async
/// The returned future is safe to poll on any async runtime. The network
/// closure runs after the lookup and before the write.
pub async fn execute_cache_then_network<D, FetchFuture>(
    blocking_work_executor: &dyn BlockingWorkExecutor,
    response_store: &Arc<dyn CachedResponseStore<D>>,
    request_fingerprint: &str,
    time_to_live_seconds: u64,
    network_fetch: impl FnOnce() -> FetchFuture,
) -> Result<(Arc<D>, CacheThenNetworkOutcome), SDKError>
where
    D: Send + Sync + 'static,
    FetchFuture: Future<Output = Result<D, SDKError>>,
{
    // A cache outage is not a reason to fail a request the network can serve:
    // the store is an accelerator, so a lookup failure degrades to a miss in
    // the same way a write failure degrades to cache-less operation.
    let lookup_store = Arc::clone(response_store);
    let lookup_fingerprint = request_fingerprint.to_string();
    let lookup_result = run_blocking(blocking_work_executor, move || {
        lookup_store.lookup_response(&lookup_fingerprint)
    })
    .await;
    let cached_value = lookup_result.unwrap_or(None);
    if let Some(cached_value) = cached_value {
        return Ok((cached_value, CacheThenNetworkOutcome::CacheHit));
    }

    let fresh_value = network_fetch().await?;
    let shared_value = Arc::new(fresh_value);

    // Storage is best-effort: a failing store must not lose the network result.
    let store = Arc::clone(response_store);
    let store_fingerprint = request_fingerprint.to_string();
    let stored_value = Arc::clone(&shared_value);
    let _ = run_blocking(blocking_work_executor, move || {
        store.store_response(&store_fingerprint, stored_value, time_to_live_seconds)
    })
    .await;

    Ok((shared_value, CacheThenNetworkOutcome::NetworkFetch))
}
#[cfg(test)]
mod tests {
    use super::{CacheThenNetworkOutcome, CachedResponseStore, execute_cache_then_network};
    use crate::blocking_work::{BlockingWorkExecutor, InlineBlockingWorkExecutor};
    use geospatial_types::GeocodeQuality;
    use geospatial_types::GeographicCoordinate;
    use geospatial_types::PlaceResult;
    use geospatial_types::SDKError;
    use std::any::Any;
    use std::collections::HashMap;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Fixture store recording every network fetch through the closure.
    struct FixtureStore {
        lookups: Mutex<HashMap<String, Arc<Vec<PlaceResult>>>>,
        writes_rejected: bool,
    }

    impl FixtureStore {
        fn empty(writes_rejected: bool) -> Self {
            Self {
                lookups: Mutex::new(HashMap::new()),
                writes_rejected,
            }
        }
    }

    /// A store whose every lookup fails, proving a cache outage degrades to a
    /// network fetch rather than failing the request.
    struct FailingLookupStore;

    /// Executor recording every job it is handed, proving the store's
    /// synchronous calls route through the seam instead of running inline.
    #[derive(Default)]
    struct RecordingExecutor {
        run_count: AtomicUsize,
    }

    impl BlockingWorkExecutor for RecordingExecutor {
        fn run_blocking(
            &self,
            work: Box<dyn FnOnce() -> Box<dyn Any + Send> + Send + 'static>,
        ) -> Pin<Box<dyn Future<Output = Box<dyn Any + Send>> + Send + 'static>> {
            self.run_count.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move { work() })
        }
    }

    impl CachedResponseStore<Vec<PlaceResult>> for FailingLookupStore {
        fn lookup_response(
            &self,
            _request_fingerprint: &str,
        ) -> Result<Option<Arc<Vec<PlaceResult>>>, SDKError> {
            Err(SDKError::CacheOperationFailure(
                geospatial_types::CacheOperationFailureReason::StoreUnavailable,
            ))
        }

        fn store_response(
            &self,
            _request_fingerprint: &str,
            _response_value: Arc<Vec<PlaceResult>>,
            _time_to_live_seconds: u64,
        ) -> Result<(), SDKError> {
            Ok(())
        }
    }

    impl CachedResponseStore<Vec<PlaceResult>> for FixtureStore {
        fn lookup_response(
            &self,
            request_fingerprint: &str,
        ) -> Result<Option<Arc<Vec<PlaceResult>>>, SDKError> {
            Ok(self
                .lookups
                .lock()
                .unwrap()
                .get(request_fingerprint)
                .cloned())
        }

        fn store_response(
            &self,
            request_fingerprint: &str,
            response_value: Arc<Vec<PlaceResult>>,
            _time_to_live_seconds: u64,
        ) -> Result<(), SDKError> {
            if self.writes_rejected {
                return Err(SDKError::CacheOperationFailure(
                    geospatial_types::CacheOperationFailureReason::WriteRejected,
                ));
            }
            self.lookups
                .lock()
                .unwrap()
                .insert(request_fingerprint.to_string(), response_value);
            Ok(())
        }
    }

    fn canned_response() -> Vec<PlaceResult> {
        vec![PlaceResult {
            structured_address_text: "1301 L St, Sacramento".to_string(),
            coordinate: GeographicCoordinate::from_degrees(38.57, -121.49).unwrap(),
            geocode_quality: GeocodeQuality::StreetAddress,
        }]
    }

    fn fixture_store(writes_rejected: bool) -> Arc<dyn CachedResponseStore<Vec<PlaceResult>>> {
        Arc::new(FixtureStore::empty(writes_rejected))
    }

    #[tokio::test]
    async fn network_fetch_runs_once_then_cache_hits() {
        let store = fixture_store(false);
        let executor = InlineBlockingWorkExecutor;
        let mut network_call_count = 0;

        let (first_value, first_outcome) =
            execute_cache_then_network(&executor, &store, "fingerprint-a", 3600, || async {
                network_call_count += 1;
                Ok::<_, SDKError>(canned_response())
            })
            .await
            .unwrap();
        assert_eq!(first_outcome, CacheThenNetworkOutcome::NetworkFetch);
        assert_eq!(first_value.len(), 1);

        let (second_value, second_outcome) =
            execute_cache_then_network(&executor, &store, "fingerprint-a", 3600, || async {
                network_call_count += 1;
                Ok::<_, SDKError>(canned_response())
            })
            .await
            .unwrap();
        assert_eq!(second_outcome, CacheThenNetworkOutcome::CacheHit);
        assert_eq!(second_value, first_value);
        assert_eq!(network_call_count, 1);
    }

    #[tokio::test]
    async fn failed_cache_lookup_degrades_to_a_network_fetch() {
        let store: Arc<dyn CachedResponseStore<Vec<PlaceResult>>> = Arc::new(FailingLookupStore);
        let executor = InlineBlockingWorkExecutor;
        let (value, outcome) =
            execute_cache_then_network(&executor, &store, "fingerprint-c", 3600, || async {
                Ok::<_, SDKError>(canned_response())
            })
            .await
            .expect("a cache outage does not fail a healthy request");
        assert_eq!(outcome, CacheThenNetworkOutcome::NetworkFetch);
        assert_eq!(value.len(), 1);
    }

    #[tokio::test]
    async fn failed_cache_write_still_returns_fresh_value() {
        let store = fixture_store(true);
        let executor = InlineBlockingWorkExecutor;
        let (value, outcome) =
            execute_cache_then_network(&executor, &store, "fingerprint-b", 3600, || async {
                Ok::<_, SDKError>(canned_response())
            })
            .await
            .unwrap();
        assert_eq!(outcome, CacheThenNetworkOutcome::NetworkFetch);
        assert_eq!(value.len(), 1);
    }

    #[tokio::test]
    async fn network_failure_propagates() {
        let store = fixture_store(false);
        let executor = InlineBlockingWorkExecutor;
        let failure =
            execute_cache_then_network(&executor, &store, "fingerprint-c", 3600, || async {
                Err::<Vec<PlaceResult>, SDKError>(SDKError::NetworkRequestFailed("offline".into()))
            })
            .await;
        assert!(matches!(failure, Err(SDKError::NetworkRequestFailed(_))));
    }

    #[tokio::test]
    async fn store_lookups_and_writes_run_through_the_executor_seam() {
        let store = fixture_store(false);
        let executor = RecordingExecutor::default();
        // A miss performs one lookup and one write; a hit performs one lookup.
        execute_cache_then_network(&executor, &store, "fingerprint-seam", 3600, || async {
            Ok::<_, SDKError>(canned_response())
        })
        .await
        .unwrap();
        execute_cache_then_network(&executor, &store, "fingerprint-seam", 3600, || async {
            Ok::<_, SDKError>(canned_response())
        })
        .await
        .unwrap();
        assert_eq!(
            executor.run_count.load(Ordering::SeqCst),
            3,
            "every synchronous store call must travel through the blocking-work seam"
        );
    }
}
