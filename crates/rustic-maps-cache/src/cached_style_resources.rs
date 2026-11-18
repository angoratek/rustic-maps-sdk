//! Disk-backed caching for the style resources a provider publishes.
//!
//! A provider-published style document names its own glyph ranges and sprite
//! sheet, and every install of the same family would otherwise fetch the same
//! bytes again. This wrapper answers a repeated fetch from the store under the
//! terms' thirty-day window, so a reopened application paints its base without
//! re-requesting the resources it already holds.
//!
//! The key names the provider and the resource address only. No credential
//! enters it — the SDK never sees one, the client appends it — so rotating a
//! key keeps the cached bytes valid, and two credentials on one provider share
//! them.
//!
//! A cache outage is not a reason to fail a request the network can serve: the
//! store is an accelerator, so a failing lookup or write degrades to the same
//! result as a miss.

use std::sync::Arc;

use geospatial_types::SDKError;
use rustic_maps_core::{BlockingWorkExecutor, CachedResponseStore, StyleResourceClient};

use crate::ResponseCategory;
use crate::cached_providers::run_cached;
use crate::request_fingerprint::RequestDescriptor;

/// Serves a provider's style-document, glyph-range and sprite-sheet fetches
/// from a response store before reaching the network.
pub struct CachedStyleResourceClient {
    /// The authenticated fetch the provider registered; only a cache miss
    /// reaches it.
    style_resources: Arc<dyn StyleResourceClient>,
    /// The store holding this provider's style resources.
    response_store: Arc<dyn CachedResponseStore<Vec<u8>>>,
    /// The provider key that namespaces every entry this wrapper writes.
    provider_key: String,
    /// How long a stored resource stays valid, in seconds.
    time_to_live_seconds: u64,
    /// The executor the store's blocking reads and writes run on.
    blocking_work_executor: Arc<dyn BlockingWorkExecutor>,
}

impl CachedStyleResourceClient {
    /// Builds the wrapper over a provider's authenticated fetch and the store
    /// its style resources are kept in.
    ///
    /// # Parameters
    /// - `style_resources` — the provider's authenticated style-resource
    ///   fetch, contacted only on a cache miss.
    /// - `response_store` — the store the fetched bytes are kept in.
    /// - `provider_key` — the provider the resources belong to; it namespaces
    ///   every entry so two providers never read each other's bytes.
    /// - `time_to_live_seconds` — how long a stored resource stays valid; the
    ///   provider terms' thirty-day window is
    ///   [`ResponseCategory::StyleResources::default_time_to_live_seconds`].
    /// - `blocking_work_executor` — the executor the store's blocking work
    ///   runs on.
    ///
    /// [`ResponseCategory::StyleResources::default_time_to_live_seconds`]: ResponseCategory::default_time_to_live_seconds
    pub fn new(
        style_resources: Arc<dyn StyleResourceClient>,
        response_store: Arc<dyn CachedResponseStore<Vec<u8>>>,
        provider_key: impl Into<String>,
        time_to_live_seconds: u64,
        blocking_work_executor: Arc<dyn BlockingWorkExecutor>,
    ) -> Self {
        Self {
            style_resources,
            response_store,
            provider_key: provider_key.into(),
            time_to_live_seconds,
            blocking_work_executor,
        }
    }
}

#[async_trait::async_trait]
impl StyleResourceClient for CachedStyleResourceClient {
    async fn fetch_style_resource(&self, url: &str) -> Result<Vec<u8>, SDKError> {
        let request_descriptor = RequestDescriptor::new(
            ResponseCategory::StyleResources,
            [
                ("provider_key", self.provider_key.as_str()),
                ("address", url),
            ],
        );
        run_cached(
            self.blocking_work_executor.as_ref(),
            &self.response_store,
            &request_descriptor,
            self.time_to_live_seconds,
            || async { self.style_resources.fetch_style_resource(url).await },
        )
        .await
    }

    fn resolve_declared_style_resource_address(&self, declared_address: &str) -> Option<String> {
        self.style_resources
            .resolve_declared_style_resource_address(declared_address)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use rustic_maps_core::InlineBlockingWorkExecutor;

    /// The bytes every case fetches.
    const STYLE_DOCUMENT_BYTES: &[u8] = br#"{"version":8}"#;

    /// An authenticated fetch double that records every address it serves.
    struct RecordingStyleResourceClient {
        requested_urls: Mutex<Vec<String>>,
    }

    impl RecordingStyleResourceClient {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                requested_urls: Mutex::new(Vec::new()),
            })
        }

        fn requested_urls(&self) -> Vec<String> {
            self.requested_urls.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl StyleResourceClient for RecordingStyleResourceClient {
        async fn fetch_style_resource(&self, url: &str) -> Result<Vec<u8>, SDKError> {
            self.requested_urls.lock().unwrap().push(url.to_string());
            Ok(STYLE_DOCUMENT_BYTES.to_vec())
        }
    }

    /// A response store holding bytes with a per-entry expiry.
    #[derive(Default)]
    struct InMemoryResponseStore {
        entries: Mutex<HashMap<String, (Vec<u8>, Instant)>>,
    }

    impl CachedResponseStore<Vec<u8>> for InMemoryResponseStore {
        fn lookup_response(
            &self,
            request_fingerprint: &str,
        ) -> Result<Option<Arc<Vec<u8>>>, SDKError> {
            let entries = self.entries.lock().unwrap();
            let Some((payload, expires_at)) = entries.get(request_fingerprint) else {
                return Ok(None);
            };
            if *expires_at <= Instant::now() {
                return Ok(None);
            }
            Ok(Some(Arc::new(payload.clone())))
        }

        fn store_response(
            &self,
            request_fingerprint: &str,
            response_value: Arc<Vec<u8>>,
            time_to_live_seconds: u64,
        ) -> Result<(), SDKError> {
            self.entries.lock().unwrap().insert(
                request_fingerprint.to_string(),
                (
                    response_value.as_ref().clone(),
                    Instant::now() + Duration::from_secs(time_to_live_seconds),
                ),
            );
            Ok(())
        }
    }

    /// A store that fails every operation, standing in for an unavailable
    /// disk.
    struct FailingResponseStore;

    impl CachedResponseStore<Vec<u8>> for FailingResponseStore {
        fn lookup_response(
            &self,
            _request_fingerprint: &str,
        ) -> Result<Option<Arc<Vec<u8>>>, SDKError> {
            Err(SDKError::CacheOperationFailure(
                geospatial_types::CacheOperationFailureReason::StoreUnavailable,
            ))
        }

        fn store_response(
            &self,
            _request_fingerprint: &str,
            _response_value: Arc<Vec<u8>>,
            _time_to_live_seconds: u64,
        ) -> Result<(), SDKError> {
            Err(SDKError::CacheOperationFailure(
                geospatial_types::CacheOperationFailureReason::StoreUnavailable,
            ))
        }
    }

    const RESOURCE_URL: &str = "https://styles.example.test/published/style.json";

    fn cached_client(
        style_resources: Arc<RecordingStyleResourceClient>,
        response_store: Arc<dyn CachedResponseStore<Vec<u8>>>,
    ) -> CachedStyleResourceClient {
        CachedStyleResourceClient::new(
            style_resources as Arc<dyn StyleResourceClient>,
            response_store,
            "example-vendor",
            ResponseCategory::StyleResources.default_time_to_live_seconds(),
            Arc::new(InlineBlockingWorkExecutor),
        )
    }

    #[tokio::test]
    async fn a_second_fetch_of_the_same_resource_costs_no_request() {
        let style_resources = RecordingStyleResourceClient::new();
        let stored_store = Arc::new(InMemoryResponseStore::default());
        let client = cached_client(Arc::clone(&style_resources), stored_store);

        let first_bytes = client
            .fetch_style_resource(RESOURCE_URL)
            .await
            .expect("the first fetch reaches the provider");
        let second_bytes = client
            .fetch_style_resource(RESOURCE_URL)
            .await
            .expect("the second fetch is served from the store");

        assert_eq!(first_bytes, STYLE_DOCUMENT_BYTES);
        assert_eq!(second_bytes, STYLE_DOCUMENT_BYTES);
        assert_eq!(
            style_resources.requested_urls(),
            vec![RESOURCE_URL.to_string()],
            "only the first fetch reaches the provider"
        );
    }

    #[tokio::test]
    async fn two_providers_never_share_a_stored_style_resource() {
        let style_resources = RecordingStyleResourceClient::new();
        let stored_store = Arc::new(InMemoryResponseStore::default());
        let first_client = cached_client(Arc::clone(&style_resources), stored_store.clone());
        let second_client = CachedStyleResourceClient::new(
            Arc::clone(&style_resources) as Arc<dyn StyleResourceClient>,
            stored_store,
            "another-vendor",
            ResponseCategory::StyleResources.default_time_to_live_seconds(),
            Arc::new(InlineBlockingWorkExecutor),
        );

        first_client
            .fetch_style_resource(RESOURCE_URL)
            .await
            .expect("the first provider's fetch reaches the provider");
        second_client
            .fetch_style_resource(RESOURCE_URL)
            .await
            .expect("the second provider's fetch reaches the provider too");

        assert_eq!(
            style_resources.requested_urls().len(),
            2,
            "the provider key namespaces the entry, so both are fetched"
        );
    }

    #[tokio::test]
    async fn an_expired_entry_is_fetched_again() {
        let style_resources = RecordingStyleResourceClient::new();
        let stored_store = Arc::new(InMemoryResponseStore::default());
        let client = CachedStyleResourceClient::new(
            Arc::clone(&style_resources) as Arc<dyn StyleResourceClient>,
            stored_store,
            "example-vendor",
            0,
            Arc::new(InlineBlockingWorkExecutor),
        );

        client
            .fetch_style_resource(RESOURCE_URL)
            .await
            .expect("the first fetch reaches the provider");
        client
            .fetch_style_resource(RESOURCE_URL)
            .await
            .expect("an entry with no lifetime is fetched again");

        assert_eq!(style_resources.requested_urls().len(), 2);
    }

    #[tokio::test]
    async fn an_unavailable_store_still_serves_the_network_result() {
        let style_resources = RecordingStyleResourceClient::new();
        let client = cached_client(Arc::clone(&style_resources), Arc::new(FailingResponseStore));

        let fetched_bytes = client
            .fetch_style_resource(RESOURCE_URL)
            .await
            .expect("a cache outage does not fail the fetch");

        assert_eq!(fetched_bytes, STYLE_DOCUMENT_BYTES);
        assert_eq!(style_resources.requested_urls().len(), 1);
    }
}
