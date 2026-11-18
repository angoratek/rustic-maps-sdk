//! Cache-then-network wrappers around the provider contracts.
//!
//! Each wrapper implements a [`geospatial_traits`] contract by delegating
//! through [`rustic_maps_core::execute_cache_then_network`] with a fingerprint built
//! from the request arguments. The wrapped provider is only contacted on a
//! cache miss, so network and cache-hit behavior are indistinguishable to
//! callers.

use crate::ResponseCategory;
use crate::request_fingerprint::{RequestDescriptor, coordinate_to_fingerprint_text};
use geospatial_traits::{
    DistanceMatrixProvider, GeocodingProvider, RasterTileProvider, RoutingProvider,
    SearchPlaceProvider, TrafficFlowProvider, TrafficIncidentProvider, VectorTileProvider,
    VectorTileSourceDescriptor,
};
use geospatial_types::route::{
    DistanceMatrix, RouteCapabilities, RouteData, RouteOptions, TransportationMode,
};
use geospatial_types::{
    BoundingBox, CacheOperationFailureReason, GeographicCoordinate, PlaceResult, RasterImageFormat,
    RasterTileData, RasterTileResolution, SDKError, TrafficFlowData, TrafficIncident,
    VectorTileData,
};
use rustic_maps_core::{
    BlockingWorkExecutor, CacheThenNetworkOutcome, CachedResponseStore, InlineBlockingWorkExecutor,
    execute_cache_then_network,
};
use std::future::Future;
use std::sync::Arc;

/// Runs one cache-then-network call and hands back an owned value.
pub(crate) async fn run_cached<D, F, FutureOutput>(
    blocking_work_executor: &dyn BlockingWorkExecutor,
    response_store: &Arc<dyn CachedResponseStore<D>>,
    request_descriptor: &RequestDescriptor,
    time_to_live_seconds: u64,
    network_fetch: F,
) -> Result<D, SDKError>
where
    D: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + Clone + 'static,
    F: FnOnce() -> FutureOutput,
    FutureOutput: Future<Output = Result<D, SDKError>>,
{
    let (response_value, _outcome) = execute_cache_then_network(
        blocking_work_executor,
        response_store,
        &request_descriptor.to_fingerprint(),
        time_to_live_seconds,
        network_fetch,
    )
    .await?;
    Ok(response_value.as_ref().clone())
}

fn options_to_fingerprint_text(route_options: &RouteOptions) -> Result<String, SDKError> {
    serde_json::to_string(route_options)
        .map_err(|_| SDKError::CacheOperationFailure(CacheOperationFailureReason::WriteRejected))
}

fn bounding_box_to_fingerprint_text(bounding_box: &BoundingBox) -> String {
    format!(
        "{}|{}",
        coordinate_to_fingerprint_text(&bounding_box.south_west),
        coordinate_to_fingerprint_text(&bounding_box.north_east)
    )
}

/// The cache handle both tile wrappers share: the store, the namespace, the
/// retention and the blocking executor.
///
/// A handle cannot be built without a `tile_source_namespace`, so neither
/// cached tile wrapper can store a payload under a provider-free key.
pub struct TileResponseCacheHandle<D> {
    response_store: Arc<dyn CachedResponseStore<D>>,
    tile_source_namespace: String,
    time_to_live_seconds: u64,
    blocking_work_executor: Arc<dyn BlockingWorkExecutor>,
}

impl<D> TileResponseCacheHandle<D>
where
    D: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + Clone + 'static,
{
    /// Builds a handle over a store, namespaced to the producing source.
    ///
    /// `tile_source_namespace` is the catalog provider key for a catalog
    /// provider, or a stable caller-supplied key for an application provider.
    /// It never carries a credential or a raw URL. `time_to_live_seconds` is
    /// the category default the owning wrapper applies.
    pub fn new(
        response_store: Arc<dyn CachedResponseStore<D>>,
        tile_source_namespace: impl Into<String>,
        time_to_live_seconds: u64,
    ) -> Self {
        Self {
            response_store,
            tile_source_namespace: tile_source_namespace.into(),
            time_to_live_seconds,
            blocking_work_executor: Arc::new(InlineBlockingWorkExecutor),
        }
    }

    /// The namespace this handle's entries carry.
    pub fn tile_source_namespace(&self) -> &str {
        &self.tile_source_namespace
    }

    /// Overrides the retention of the entries this handle stores.
    pub fn with_time_to_live_seconds(mut self, time_to_live_seconds: u64) -> Self {
        self.time_to_live_seconds = time_to_live_seconds;
        self
    }

    /// Runs the store's synchronous lookups and writes through `executor`.
    pub fn with_blocking_work_executor(
        mut self,
        blocking_work_executor: Arc<dyn BlockingWorkExecutor>,
    ) -> Self {
        self.blocking_work_executor = blocking_work_executor;
        self
    }

    /// Answers one request from the store, fetching and storing it on a miss.
    async fn lookup_or_fetch<F, FutureOutput>(
        &self,
        request_fingerprint: &str,
        network_fetch: F,
    ) -> Result<(D, CacheThenNetworkOutcome), SDKError>
    where
        F: FnOnce() -> FutureOutput,
        FutureOutput: Future<Output = Result<D, SDKError>>,
    {
        let (response_value, fetch_outcome) = execute_cache_then_network(
            self.blocking_work_executor.as_ref(),
            &self.response_store,
            request_fingerprint,
            self.time_to_live_seconds,
            network_fetch,
        )
        .await?;
        Ok((response_value.as_ref().clone(), fetch_outcome))
    }
}

/// Builds the cacheable descriptor of one raster tile request, covering the
/// producing source, the address, the encoding and the resolution.
///
/// `tile_source_namespace` names the provider or source that produced the
/// tile, so two providers serving the same address never share an entry. It is
/// the catalog provider key for a catalog provider, and a caller-supplied
/// stable key for an application provider; it never carries a credential.
/// Public so the tile-pack manager (`tile_packs`) fingerprints its planned
/// members with exactly the same scheme the cached wrapper stores under — the
/// pack membership and the cache entries must key identically for the two to
/// coexist in one store.
pub fn tile_request_descriptor(
    tile_source_namespace: &str,
    tile_x: u32,
    tile_y: u32,
    zoom_level: u8,
    image_format: RasterImageFormat,
    tile_resolution: RasterTileResolution,
) -> RequestDescriptor {
    RequestDescriptor::new(
        ResponseCategory::RasterTiles,
        [
            ("tile_source_namespace", tile_source_namespace.to_string()),
            ("zoom_level", zoom_level.to_string()),
            ("tile_x", tile_x.to_string()),
            ("tile_y", tile_y.to_string()),
            ("image_format", format!("{image_format:?}")),
            ("tile_resolution", format!("{tile_resolution:?}")),
        ],
    )
}

/// Cached geocoding: forward and reverse lookups.
pub struct CachedGeocodingProvider {
    geocoding_provider: Arc<dyn GeocodingProvider>,
    response_store: Arc<dyn CachedResponseStore<Vec<PlaceResult>>>,
    time_to_live_seconds: u64,
    blocking_work_executor: Arc<dyn BlockingWorkExecutor>,
}

impl CachedGeocodingProvider {
    /// Wraps a provider, caching under the geocoding category default TTL.
    pub fn new(
        geocoding_provider: Arc<dyn GeocodingProvider>,
        response_store: Arc<dyn CachedResponseStore<Vec<PlaceResult>>>,
    ) -> Self {
        Self {
            geocoding_provider,
            response_store,
            time_to_live_seconds: ResponseCategory::Geocoding.default_time_to_live_seconds(),
            blocking_work_executor: Arc::new(InlineBlockingWorkExecutor),
        }
    }

    /// Overrides the TTL for both directions of this wrapper.
    pub fn with_time_to_live_seconds(mut self, time_to_live_seconds: u64) -> Self {
        self.time_to_live_seconds = time_to_live_seconds;
        self
    }

    /// Runs the store's synchronous lookups and writes through `executor`,
    /// keeping them off the async reactor that drives provider futures.
    pub fn with_blocking_work_executor(
        mut self,
        blocking_work_executor: Arc<dyn BlockingWorkExecutor>,
    ) -> Self {
        self.blocking_work_executor = blocking_work_executor;
        self
    }
}

#[async_trait::async_trait]
impl GeocodingProvider for CachedGeocodingProvider {
    async fn forward_geocode(&self, address_text: &str) -> Result<Vec<PlaceResult>, SDKError> {
        let descriptor = RequestDescriptor::new(
            ResponseCategory::Geocoding,
            [
                ("operation", "forward_geocode"),
                ("address_text", address_text),
            ],
        );
        let geocoding_provider = self.geocoding_provider.clone();
        let address_text = address_text.to_string();
        run_cached(
            self.blocking_work_executor.as_ref(),
            &self.response_store,
            &descriptor,
            self.time_to_live_seconds,
            move || async move { geocoding_provider.forward_geocode(&address_text).await },
        )
        .await
    }

    async fn reverse_geocode(
        &self,
        coordinate: &GeographicCoordinate,
    ) -> Result<Vec<PlaceResult>, SDKError> {
        let descriptor = RequestDescriptor::new(
            ResponseCategory::Geocoding,
            [
                ("operation", "reverse_geocode"),
                (
                    "coordinate",
                    coordinate_to_fingerprint_text(coordinate).as_str(),
                ),
            ],
        );
        let geocoding_provider = self.geocoding_provider.clone();
        let coordinate = *coordinate;
        run_cached(
            self.blocking_work_executor.as_ref(),
            &self.response_store,
            &descriptor,
            self.time_to_live_seconds,
            move || async move { geocoding_provider.reverse_geocode(&coordinate).await },
        )
        .await
    }
}

/// Cached route calculation.
pub struct CachedRoutingProvider {
    routing_provider: Arc<dyn RoutingProvider>,
    response_store: Arc<dyn CachedResponseStore<RouteData>>,
    time_to_live_seconds: u64,
    blocking_work_executor: Arc<dyn BlockingWorkExecutor>,
}

impl CachedRoutingProvider {
    /// Wraps a provider, caching under the routing category default TTL.
    pub fn new(
        routing_provider: Arc<dyn RoutingProvider>,
        response_store: Arc<dyn CachedResponseStore<RouteData>>,
    ) -> Self {
        Self {
            routing_provider,
            response_store,
            time_to_live_seconds: ResponseCategory::Routing.default_time_to_live_seconds(),
            blocking_work_executor: Arc::new(InlineBlockingWorkExecutor),
        }
    }

    /// Overrides the TTL of this wrapper.
    pub fn with_time_to_live_seconds(mut self, time_to_live_seconds: u64) -> Self {
        self.time_to_live_seconds = time_to_live_seconds;
        self
    }

    /// Runs the store's synchronous lookups and writes through `executor`,
    /// keeping them off the async reactor that drives provider futures.
    pub fn with_blocking_work_executor(
        mut self,
        blocking_work_executor: Arc<dyn BlockingWorkExecutor>,
    ) -> Self {
        self.blocking_work_executor = blocking_work_executor;
        self
    }
}

#[async_trait::async_trait]
impl RoutingProvider for CachedRoutingProvider {
    /// The cache never changes what the wrapped provider can honour.
    fn route_capabilities(&self) -> RouteCapabilities {
        self.routing_provider.route_capabilities()
    }

    async fn calculate_route(
        &self,
        waypoints: &[GeographicCoordinate],
        route_options: &RouteOptions,
    ) -> Result<RouteData, SDKError> {
        let route_options_text = options_to_fingerprint_text(route_options)?;
        let descriptor = RequestDescriptor::new(
            ResponseCategory::Routing,
            [
                (
                    "waypoints",
                    waypoints
                        .iter()
                        .map(coordinate_to_fingerprint_text)
                        .collect::<Vec<_>>()
                        .join(";"),
                ),
                ("route_options", route_options_text),
            ],
        );
        let routing_provider = self.routing_provider.clone();
        let waypoints = waypoints.to_vec();
        let route_options = route_options.clone();
        run_cached(
            self.blocking_work_executor.as_ref(),
            &self.response_store,
            &descriptor,
            self.time_to_live_seconds,
            move || async move {
                routing_provider
                    .calculate_route(&waypoints, &route_options)
                    .await
            },
        )
        .await
    }
}

/// Cached origin-destination travel-time matrices.
pub struct CachedDistanceMatrixProvider {
    distance_matrix_provider: Arc<dyn DistanceMatrixProvider>,
    response_store: Arc<dyn CachedResponseStore<DistanceMatrix>>,
    time_to_live_seconds: u64,
    blocking_work_executor: Arc<dyn BlockingWorkExecutor>,
}

impl CachedDistanceMatrixProvider {
    /// Wraps a provider, caching under the matrix category default TTL.
    pub fn new(
        distance_matrix_provider: Arc<dyn DistanceMatrixProvider>,
        response_store: Arc<dyn CachedResponseStore<DistanceMatrix>>,
    ) -> Self {
        Self {
            distance_matrix_provider,
            response_store,
            time_to_live_seconds: ResponseCategory::DistanceMatrix.default_time_to_live_seconds(),
            blocking_work_executor: Arc::new(InlineBlockingWorkExecutor),
        }
    }

    /// Overrides the TTL of this wrapper.
    pub fn with_time_to_live_seconds(mut self, time_to_live_seconds: u64) -> Self {
        self.time_to_live_seconds = time_to_live_seconds;
        self
    }

    /// Runs the store's synchronous lookups and writes through `executor`,
    /// keeping them off the async reactor that drives provider futures.
    pub fn with_blocking_work_executor(
        mut self,
        blocking_work_executor: Arc<dyn BlockingWorkExecutor>,
    ) -> Self {
        self.blocking_work_executor = blocking_work_executor;
        self
    }
}

#[async_trait::async_trait]
impl DistanceMatrixProvider for CachedDistanceMatrixProvider {
    async fn calculate_distance_matrix(
        &self,
        origins: &[GeographicCoordinate],
        destinations: &[GeographicCoordinate],
        transportation_mode: TransportationMode,
    ) -> Result<DistanceMatrix, SDKError> {
        let descriptor = RequestDescriptor::new(
            ResponseCategory::DistanceMatrix,
            [
                (
                    "origins",
                    origins
                        .iter()
                        .map(coordinate_to_fingerprint_text)
                        .collect::<Vec<_>>()
                        .join(";"),
                ),
                (
                    "destinations",
                    destinations
                        .iter()
                        .map(coordinate_to_fingerprint_text)
                        .collect::<Vec<_>>()
                        .join(";"),
                ),
                ("transportation_mode", format!("{transportation_mode:?}")),
            ],
        );
        let distance_matrix_provider = self.distance_matrix_provider.clone();
        let origins = origins.to_vec();
        let destinations = destinations.to_vec();
        run_cached(
            self.blocking_work_executor.as_ref(),
            &self.response_store,
            &descriptor,
            self.time_to_live_seconds,
            move || async move {
                distance_matrix_provider
                    .calculate_distance_matrix(&origins, &destinations, transportation_mode)
                    .await
            },
        )
        .await
    }
}

/// Cached search: autocomplete and full queries share the category but
/// differ by their `operation` fingerprint key.
pub struct CachedSearchPlaceProvider {
    search_place_provider: Arc<dyn SearchPlaceProvider>,
    response_store: Arc<dyn CachedResponseStore<Vec<PlaceResult>>>,
    time_to_live_seconds: u64,
    blocking_work_executor: Arc<dyn BlockingWorkExecutor>,
}

impl CachedSearchPlaceProvider {
    /// Wraps a provider, caching under the search category default TTL.
    pub fn new(
        search_place_provider: Arc<dyn SearchPlaceProvider>,
        response_store: Arc<dyn CachedResponseStore<Vec<PlaceResult>>>,
    ) -> Self {
        Self {
            search_place_provider,
            response_store,
            time_to_live_seconds: ResponseCategory::SearchPrediction.default_time_to_live_seconds(),
            blocking_work_executor: Arc::new(InlineBlockingWorkExecutor),
        }
    }

    /// Overrides the TTL of this wrapper.
    pub fn with_time_to_live_seconds(mut self, time_to_live_seconds: u64) -> Self {
        self.time_to_live_seconds = time_to_live_seconds;
        self
    }

    /// Runs the store's synchronous lookups and writes through `executor`,
    /// keeping them off the async reactor that drives provider futures.
    pub fn with_blocking_work_executor(
        mut self,
        blocking_work_executor: Arc<dyn BlockingWorkExecutor>,
    ) -> Self {
        self.blocking_work_executor = blocking_work_executor;
        self
    }
}

#[async_trait::async_trait]
impl SearchPlaceProvider for CachedSearchPlaceProvider {
    async fn search_autocomplete(
        &self,
        query_text: &str,
        search_boundary: Option<BoundingBox>,
    ) -> Result<Vec<PlaceResult>, SDKError> {
        let descriptor = RequestDescriptor::new(
            ResponseCategory::SearchPrediction,
            [
                ("operation", "search_autocomplete"),
                ("query_text", query_text),
                (
                    "search_boundary",
                    search_boundary
                        .as_ref()
                        .map(bounding_box_to_fingerprint_text)
                        .unwrap_or_default()
                        .as_str(),
                ),
            ],
        );
        let search_place_provider = self.search_place_provider.clone();
        let query_text = query_text.to_string();
        run_cached(
            self.blocking_work_executor.as_ref(),
            &self.response_store,
            &descriptor,
            self.time_to_live_seconds,
            move || async move {
                search_place_provider
                    .search_autocomplete(&query_text, search_boundary)
                    .await
            },
        )
        .await
    }

    async fn search_places(
        &self,
        query_text: &str,
        search_boundary: Option<BoundingBox>,
    ) -> Result<Vec<PlaceResult>, SDKError> {
        let descriptor = RequestDescriptor::new(
            ResponseCategory::SearchPrediction,
            [
                ("operation", "search_places"),
                ("query_text", query_text),
                (
                    "search_boundary",
                    search_boundary
                        .as_ref()
                        .map(bounding_box_to_fingerprint_text)
                        .unwrap_or_default()
                        .as_str(),
                ),
            ],
        );
        let search_place_provider = self.search_place_provider.clone();
        let query_text = query_text.to_string();
        run_cached(
            self.blocking_work_executor.as_ref(),
            &self.response_store,
            &descriptor,
            self.time_to_live_seconds,
            move || async move {
                search_place_provider
                    .search_places(&query_text, search_boundary)
                    .await
            },
        )
        .await
    }
}

/// Cached traffic disruption reports.
pub struct CachedTrafficIncidentProvider {
    traffic_incident_provider: Arc<dyn TrafficIncidentProvider>,
    response_store: Arc<dyn CachedResponseStore<Vec<TrafficIncident>>>,
    time_to_live_seconds: u64,
    blocking_work_executor: Arc<dyn BlockingWorkExecutor>,
}

impl CachedTrafficIncidentProvider {
    /// Wraps a provider, caching under the incidents category default TTL.
    pub fn new(
        traffic_incident_provider: Arc<dyn TrafficIncidentProvider>,
        response_store: Arc<dyn CachedResponseStore<Vec<TrafficIncident>>>,
    ) -> Self {
        Self {
            traffic_incident_provider,
            response_store,
            time_to_live_seconds: ResponseCategory::TrafficIncidents.default_time_to_live_seconds(),
            blocking_work_executor: Arc::new(InlineBlockingWorkExecutor),
        }
    }

    /// Overrides the TTL of this wrapper.
    pub fn with_time_to_live_seconds(mut self, time_to_live_seconds: u64) -> Self {
        self.time_to_live_seconds = time_to_live_seconds;
        self
    }

    /// Runs the store's synchronous lookups and writes through `executor`,
    /// keeping them off the async reactor that drives provider futures.
    pub fn with_blocking_work_executor(
        mut self,
        blocking_work_executor: Arc<dyn BlockingWorkExecutor>,
    ) -> Self {
        self.blocking_work_executor = blocking_work_executor;
        self
    }
}

#[async_trait::async_trait]
impl TrafficIncidentProvider for CachedTrafficIncidentProvider {
    async fn fetch_traffic_incidents(
        &self,
        bounding_box: &BoundingBox,
    ) -> Result<Vec<TrafficIncident>, SDKError> {
        let descriptor = RequestDescriptor::new(
            ResponseCategory::TrafficIncidents,
            [(
                "bounding_box",
                bounding_box_to_fingerprint_text(bounding_box),
            )],
        );
        let traffic_incident_provider = self.traffic_incident_provider.clone();
        let bounding_box = *bounding_box;
        run_cached(
            self.blocking_work_executor.as_ref(),
            &self.response_store,
            &descriptor,
            self.time_to_live_seconds,
            move || async move {
                traffic_incident_provider
                    .fetch_traffic_incidents(&bounding_box)
                    .await
            },
        )
        .await
    }
}

/// Cached traffic flow observations.
pub struct CachedTrafficFlowProvider {
    traffic_flow_provider: Arc<dyn TrafficFlowProvider>,
    response_store: Arc<dyn CachedResponseStore<Vec<TrafficFlowData>>>,
    time_to_live_seconds: u64,
    blocking_work_executor: Arc<dyn BlockingWorkExecutor>,
}

impl CachedTrafficFlowProvider {
    /// Wraps a provider, caching under the flow category default TTL.
    pub fn new(
        traffic_flow_provider: Arc<dyn TrafficFlowProvider>,
        response_store: Arc<dyn CachedResponseStore<Vec<TrafficFlowData>>>,
    ) -> Self {
        Self {
            traffic_flow_provider,
            response_store,
            time_to_live_seconds: ResponseCategory::TrafficFlow.default_time_to_live_seconds(),
            blocking_work_executor: Arc::new(InlineBlockingWorkExecutor),
        }
    }

    /// Overrides the TTL of this wrapper.
    pub fn with_time_to_live_seconds(mut self, time_to_live_seconds: u64) -> Self {
        self.time_to_live_seconds = time_to_live_seconds;
        self
    }

    /// Runs the store's synchronous lookups and writes through `executor`,
    /// keeping them off the async reactor that drives provider futures.
    pub fn with_blocking_work_executor(
        mut self,
        blocking_work_executor: Arc<dyn BlockingWorkExecutor>,
    ) -> Self {
        self.blocking_work_executor = blocking_work_executor;
        self
    }
}

#[async_trait::async_trait]
impl TrafficFlowProvider for CachedTrafficFlowProvider {
    async fn fetch_traffic_flow(
        &self,
        bounding_box: &BoundingBox,
    ) -> Result<Vec<TrafficFlowData>, SDKError> {
        let descriptor = RequestDescriptor::new(
            ResponseCategory::TrafficFlow,
            [(
                "bounding_box",
                bounding_box_to_fingerprint_text(bounding_box),
            )],
        );
        let traffic_flow_provider = self.traffic_flow_provider.clone();
        let bounding_box = *bounding_box;
        run_cached(
            self.blocking_work_executor.as_ref(),
            &self.response_store,
            &descriptor,
            self.time_to_live_seconds,
            move || async move {
                traffic_flow_provider
                    .fetch_traffic_flow(&bounding_box)
                    .await
            },
        )
        .await
    }
}

/// Cached raster tiles, the longest-lived category.
pub struct CachedRasterTileProvider {
    raster_tile_provider: Arc<dyn RasterTileProvider>,
    /// The store, namespace, retention and executor this wrapper uses.
    tile_cache_handle: TileResponseCacheHandle<RasterTileData>,
}

impl CachedRasterTileProvider {
    /// Wraps a provider, caching under the tile category default TTL.
    ///
    /// `tile_source_namespace` names the provider or source that produces the
    /// tiles — the catalog provider key for a catalog provider, a stable
    /// caller key for an application provider. Two wrappers over one store
    /// with different namespaces never serve each other's tiles. Use a stable
    /// value that carries no credential.
    pub fn new(
        raster_tile_provider: Arc<dyn RasterTileProvider>,
        response_store: Arc<dyn CachedResponseStore<RasterTileData>>,
        tile_source_namespace: impl Into<String>,
    ) -> Self {
        Self {
            raster_tile_provider,
            tile_cache_handle: TileResponseCacheHandle::new(
                response_store,
                tile_source_namespace,
                ResponseCategory::RasterTiles.default_time_to_live_seconds(),
            ),
        }
    }

    /// The namespace this wrapper's cache entries and pack members carry.
    pub fn tile_source_namespace(&self) -> &str {
        self.tile_cache_handle.tile_source_namespace()
    }

    /// Overrides the TTL of this wrapper.
    pub fn with_time_to_live_seconds(mut self, time_to_live_seconds: u64) -> Self {
        self.tile_cache_handle = self
            .tile_cache_handle
            .with_time_to_live_seconds(time_to_live_seconds);
        self
    }

    /// Runs the store's synchronous lookups and writes through `executor`,
    /// keeping them off the async reactor that drives provider futures.
    pub fn with_blocking_work_executor(
        mut self,
        blocking_work_executor: Arc<dyn BlockingWorkExecutor>,
    ) -> Self {
        self.tile_cache_handle = self
            .tile_cache_handle
            .with_blocking_work_executor(blocking_work_executor);
        self
    }
}

#[async_trait::async_trait]
impl RasterTileProvider for CachedRasterTileProvider {
    async fn fetch_raster_tile(
        &self,
        tile_x: u32,
        tile_y: u32,
        zoom_level: u8,
        image_format: RasterImageFormat,
        tile_resolution: RasterTileResolution,
    ) -> Result<RasterTileData, SDKError> {
        self.fetch_raster_tile_with_cache_outcome(
            tile_x,
            tile_y,
            zoom_level,
            image_format,
            tile_resolution,
        )
        .await
        .map(|(fetched_tile, _fetch_outcome)| fetched_tile)
    }
}

impl CachedRasterTileProvider {
    /// Fetches one tile and additionally reports whether the response came
    /// from the cache or the network. The tile-pack manager's honest
    /// progress reporting reads this: a pack prepare counts the tiles it
    /// actually downloaded separately from the ones the store already held.
    pub async fn fetch_raster_tile_with_cache_outcome(
        &self,
        tile_x: u32,
        tile_y: u32,
        zoom_level: u8,
        image_format: RasterImageFormat,
        tile_resolution: RasterTileResolution,
    ) -> Result<(RasterTileData, CacheThenNetworkOutcome), SDKError> {
        let descriptor = tile_request_descriptor(
            self.tile_source_namespace(),
            tile_x,
            tile_y,
            zoom_level,
            image_format,
            tile_resolution,
        );
        let raster_tile_provider = self.raster_tile_provider.clone();
        self.tile_cache_handle
            .lookup_or_fetch(&descriptor.to_fingerprint(), move || async move {
                raster_tile_provider
                    .fetch_raster_tile(tile_x, tile_y, zoom_level, image_format, tile_resolution)
                    .await
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use geospatial_traits::GeocodingProvider;
    use geospatial_types::{GeocodeQuality, GeographicCoordinate, PlaceResult, SDKError};
    use rustic_maps_core::CachedResponseStore;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// The namespace the raster-tile tests fingerprint under.
    const TEST_TILE_SOURCE_NAMESPACE: &str = "test-provider";

    use super::CachedGeocodingProvider;
    use crate::disk_quota::FixedDiskSpaceProbe;
    use crate::sqlite_store::{SqliteResponseCache, SqliteResponseCacheConfiguration};

    /// Counting decorator over the contract, asserting cache behavior.
    struct CountingGeocodingProvider {
        forward_call_count: AtomicUsize,
        last_address_text: Mutex<String>,
    }

    impl CountingGeocodingProvider {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                forward_call_count: AtomicUsize::new(0),
                last_address_text: Mutex::new(String::new()),
            })
        }
    }

    #[async_trait::async_trait]
    impl GeocodingProvider for CountingGeocodingProvider {
        async fn forward_geocode(&self, address_text: &str) -> Result<Vec<PlaceResult>, SDKError> {
            self.forward_call_count.fetch_add(1, Ordering::SeqCst);
            *self.last_address_text.lock().unwrap() = address_text.to_string();
            Ok(vec![PlaceResult {
                structured_address_text: format!("Mock result for {address_text}"),
                coordinate: GeographicCoordinate::from_degrees(38.5766, -121.4944).unwrap(),
                geocode_quality: GeocodeQuality::City,
            }])
        }

        async fn reverse_geocode(
            &self,
            coordinate: &GeographicCoordinate,
        ) -> Result<Vec<PlaceResult>, SDKError> {
            Ok(vec![PlaceResult {
                structured_address_text: format!(
                    "Reverse at {:.4}, {:.4}",
                    coordinate.latitude, coordinate.longitude
                ),
                coordinate: *coordinate,
                geocode_quality: GeocodeQuality::Street,
            }])
        }
    }

    fn sqlite_store(test_label: &str) -> Arc<SqliteResponseCache> {
        // Parallel test execution shares the process: each test needs its own
        // database file or a sibling's cleanup deletes an open store.
        let database_path = std::env::temp_dir().join(format!(
            "rustic-maps-cache-cached-provider-{}-{test_label}.sqlite",
            std::process::id()
        ));
        std::fs::remove_file(&database_path).ok();
        SqliteResponseCache::with_disk_space_probe(
            SqliteResponseCacheConfiguration {
                database_path,
                ..SqliteResponseCacheConfiguration::default()
            },
            Arc::new(FixedDiskSpaceProbe::reporting(u64::MAX)),
        )
        .unwrap()
    }

    #[test]
    fn route_strategy_distinctly_fingerprints_route_requests() {
        use super::options_to_fingerprint_text;
        use geospatial_types::route::{RouteOptions, RouteStrategy};

        let fastest_options = RouteOptions {
            route_strategy: RouteStrategy::Fastest,
            ..RouteOptions::default()
        };
        let shortest_options = RouteOptions {
            route_strategy: RouteStrategy::Shortest,
            ..RouteOptions::default()
        };
        assert_ne!(
            options_to_fingerprint_text(&fastest_options).unwrap(),
            options_to_fingerprint_text(&shortest_options).unwrap(),
            "otherwise identical route requests under different strategies \
             must not share a cache entry"
        );
    }

    #[test]
    fn tile_resolution_distinctly_fingerprints_tile_requests() {
        use super::tile_request_descriptor;
        use geospatial_types::RasterTileResolution;

        let standard_request = tile_request_descriptor(
            TEST_TILE_SOURCE_NAMESPACE,
            2662,
            6285,
            14,
            geospatial_types::RasterImageFormat::Png,
            RasterTileResolution::Standard,
        );
        let retina_request = tile_request_descriptor(
            TEST_TILE_SOURCE_NAMESPACE,
            2662,
            6285,
            14,
            geospatial_types::RasterImageFormat::Png,
            RasterTileResolution::Retina,
        );
        assert_ne!(
            standard_request.to_fingerprint(),
            retina_request.to_fingerprint(),
            "otherwise identical tile requests under different resolutions \
             must not share a cache entry"
        );
    }

    #[tokio::test]
    async fn second_identical_call_is_served_from_cache() {
        let counting_provider = CountingGeocodingProvider::new();
        let wrapper = CachedGeocodingProvider::new(
            counting_provider.clone(),
            sqlite_store("second-identical-call") as Arc<dyn CachedResponseStore<Vec<PlaceResult>>>,
        );
        let first = wrapper.forward_geocode("Sacramento").await.unwrap();
        let second = wrapper.forward_geocode("Sacramento").await.unwrap();
        assert_eq!(first, second);
        assert_eq!(
            counting_provider.forward_call_count.load(Ordering::SeqCst),
            1
        );
        wrapper.forward_geocode("Oakland").await.unwrap();
        assert_eq!(
            counting_provider.forward_call_count.load(Ordering::SeqCst),
            2
        );
    }

    #[tokio::test]
    async fn distinct_reverse_coordinates_miss_independently() {
        let counting_provider = CountingGeocodingProvider::new();
        let wrapper = CachedGeocodingProvider::new(
            counting_provider.clone(),
            sqlite_store("distinct-reverse") as Arc<dyn CachedResponseStore<Vec<PlaceResult>>>,
        );
        let coordinate = GeographicCoordinate::from_degrees(38.5766, -121.4944).unwrap();
        wrapper.reverse_geocode(&coordinate).await.unwrap();
        wrapper.reverse_geocode(&coordinate).await.unwrap();
        assert_eq!(
            counting_provider.forward_call_count.load(Ordering::SeqCst),
            0
        );
    }
}

/// Cached vector tiles — like [`CachedRasterTileProvider`], the
/// longest-lived category, namespaced per tileset source.
pub struct CachedVectorTileProvider {
    vector_tile_provider: Arc<dyn VectorTileProvider>,
    /// The store, namespace, retention and executor this wrapper uses.
    tile_cache_handle: TileResponseCacheHandle<VectorTileData>,
}

impl CachedVectorTileProvider {
    /// Wraps a provider, caching under the vector-tiles category default TTL.
    ///
    /// `tile_source_namespace` names the tileset that produces the payloads —
    /// the catalog provider key for a catalog provider, a stable caller key
    /// for an application provider. Two wrappers over one store with
    /// different namespaces never serve each other's payloads.
    pub fn new(
        vector_tile_provider: Arc<dyn VectorTileProvider>,
        response_store: Arc<dyn CachedResponseStore<VectorTileData>>,
        tile_source_namespace: impl Into<String>,
    ) -> Self {
        Self {
            vector_tile_provider,
            tile_cache_handle: TileResponseCacheHandle::new(
                response_store,
                tile_source_namespace,
                ResponseCategory::VectorTiles.default_time_to_live_seconds(),
            ),
        }
    }

    /// The namespace this wrapper's payloads are stored under.
    pub fn tile_source_namespace(&self) -> &str {
        self.tile_cache_handle.tile_source_namespace()
    }

    /// Overrides the TTL of this wrapper.
    pub fn with_time_to_live_seconds(mut self, time_to_live_seconds: u64) -> Self {
        self.tile_cache_handle = self
            .tile_cache_handle
            .with_time_to_live_seconds(time_to_live_seconds);
        self
    }

    /// Runs the store's synchronous lookups and writes through `executor`,
    /// keeping them off the async reactor that drives provider futures.
    pub fn with_blocking_work_executor(
        mut self,
        blocking_work_executor: Arc<dyn BlockingWorkExecutor>,
    ) -> Self {
        self.tile_cache_handle = self
            .tile_cache_handle
            .with_blocking_work_executor(blocking_work_executor);
        self
    }
}

#[async_trait::async_trait]
impl VectorTileProvider for CachedVectorTileProvider {
    fn source_descriptor(&self) -> VectorTileSourceDescriptor {
        self.vector_tile_provider.source_descriptor()
    }

    async fn fetch_vector_tile(
        &self,
        tile_x: u32,
        tile_y: u32,
        zoom_level: u8,
    ) -> Result<VectorTileData, SDKError> {
        // The renderer builds this same key before a payload reaches a store,
        // so a cached fetch and a scheduler fetch share one entry.
        let request_fingerprint = rustic_maps_core::vector_tile_payload_fingerprint(
            self.tile_source_namespace(),
            zoom_level,
            tile_x,
            tile_y,
        );
        let vector_tile_provider = self.vector_tile_provider.clone();
        self.tile_cache_handle
            .lookup_or_fetch(&request_fingerprint, move || async move {
                vector_tile_provider
                    .fetch_vector_tile(tile_x, tile_y, zoom_level)
                    .await
            })
            .await
            .map(|(fetched_tile, _fetch_outcome)| fetched_tile)
    }
}

#[cfg(test)]
mod vector_tile_cached_provider_tests {
    use crate::disk_quota::FixedDiskSpaceProbe;
    use crate::sqlite_store::{SqliteResponseCache, SqliteResponseCacheConfiguration};
    use geospatial_traits::VectorTileProvider as _;
    use geospatial_types::SDKError;
    use rustic_maps_core::CachedResponseStore;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn sqlite_store(test_label: &str) -> Arc<SqliteResponseCache> {
        // Parallel test execution shares the process: each test needs its own
        // database file or a sibling's cleanup deletes an open store.
        let database_path = std::env::temp_dir().join(format!(
            "rustic-maps-cache-cached-provider-{}-{test_label}.sqlite",
            std::process::id()
        ));
        std::fs::remove_file(&database_path).ok();
        SqliteResponseCache::with_disk_space_probe(
            SqliteResponseCacheConfiguration {
                database_path,
                ..SqliteResponseCacheConfiguration::default()
            },
            Arc::new(FixedDiskSpaceProbe::reporting(u64::MAX)),
        )
        .unwrap()
    }

    /// Counting vector-tile decorator asserting cache behavior end-to-end.
    /// The inner provider is the compile-time mock (no HTTP, no network).
    struct CountingVectorTileProvider {
        fetch_call_count: AtomicUsize,
    }

    impl CountingVectorTileProvider {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                fetch_call_count: AtomicUsize::new(0),
            })
        }
    }

    #[async_trait::async_trait]
    impl geospatial_traits::VectorTileProvider for CountingVectorTileProvider {
        fn source_descriptor(&self) -> geospatial_traits::VectorTileSourceDescriptor {
            geospatial_traits::mock::MockVectorTileProvider.source_descriptor()
        }

        async fn fetch_vector_tile(
            &self,
            tile_x: u32,
            tile_y: u32,
            zoom_level: u8,
        ) -> Result<geospatial_types::VectorTileData, SDKError> {
            self.fetch_call_count.fetch_add(1, Ordering::SeqCst);
            geospatial_traits::mock::MockVectorTileProvider
                .fetch_vector_tile(tile_x, tile_y, zoom_level)
                .await
        }
    }

    #[tokio::test]
    async fn vector_tiles_are_cached_then_served_from_cache() {
        use super::CachedVectorTileProvider;
        let counting_provider = CountingVectorTileProvider::new();
        let wrapper = CachedVectorTileProvider::new(
            counting_provider.clone(),
            sqlite_store("vector-tiles-cache-hit")
                as Arc<dyn CachedResponseStore<geospatial_types::VectorTileData>>,
            "alpha-vector-tileset",
        );
        let first = wrapper
            .fetch_vector_tile(18, 27, 6)
            .await
            .expect("first vector fetch must succeed");
        let second = wrapper
            .fetch_vector_tile(18, 27, 6)
            .await
            .expect("second vector fetch must succeed");
        assert_eq!(first, second);
        assert_eq!(counting_provider.fetch_call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn two_tileset_namespaces_never_share_a_stored_payload() {
        use super::CachedVectorTileProvider;
        let store = sqlite_store("vector-tiles-namespaces");
        let alpha_provider = CountingVectorTileProvider::new();
        let beta_provider = CountingVectorTileProvider::new();
        let alpha_wrapper = CachedVectorTileProvider::new(
            alpha_provider.clone(),
            Arc::clone(&store) as Arc<dyn CachedResponseStore<geospatial_types::VectorTileData>>,
            "alpha-vector-tileset",
        );
        let beta_wrapper = CachedVectorTileProvider::new(
            beta_provider.clone(),
            Arc::clone(&store) as Arc<dyn CachedResponseStore<geospatial_types::VectorTileData>>,
            "beta-vector-tileset",
        );
        alpha_wrapper.fetch_vector_tile(18, 27, 6).await.unwrap();
        beta_wrapper.fetch_vector_tile(18, 27, 6).await.unwrap();
        assert_eq!(
            alpha_provider.fetch_call_count.load(Ordering::SeqCst),
            1,
            "the second tileset's request must not be served from the first's entry"
        );
        assert_eq!(beta_provider.fetch_call_count.load(Ordering::SeqCst), 1);
        // Both tilesets stored their own payload for the same address.
        let alpha_key = rustic_maps_core::vector_tile_payload_fingerprint(
            alpha_wrapper.tile_source_namespace(),
            6,
            18,
            27,
        );
        let beta_key = rustic_maps_core::vector_tile_payload_fingerprint(
            beta_wrapper.tile_source_namespace(),
            6,
            18,
            27,
        );
        assert!(
            CachedResponseStore::<geospatial_types::VectorTileData>::lookup_response(
                store.as_ref(),
                &alpha_key
            )
            .unwrap()
            .is_some()
        );
        assert!(
            CachedResponseStore::<geospatial_types::VectorTileData>::lookup_response(
                store.as_ref(),
                &beta_key
            )
            .unwrap()
            .is_some()
        );
    }
}

#[cfg(test)]
mod raster_tile_cached_provider_tests {
    use crate::disk_quota::FixedDiskSpaceProbe;
    use crate::sqlite_store::{SqliteResponseCache, SqliteResponseCacheConfiguration};
    use geospatial_traits::RasterTileProvider;
    use geospatial_types::{RasterImageFormat, RasterTileData, RasterTileResolution, SDKError};
    use rustic_maps_core::CachedResponseStore;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn sqlite_store(test_label: &str) -> Arc<SqliteResponseCache> {
        let database_path = std::env::temp_dir().join(format!(
            "rustic-maps-cache-raster-provider-{}-{test_label}.sqlite",
            std::process::id()
        ));
        std::fs::remove_file(&database_path).ok();
        SqliteResponseCache::with_disk_space_probe(
            SqliteResponseCacheConfiguration {
                database_path,
                ..SqliteResponseCacheConfiguration::default()
            },
            Arc::new(FixedDiskSpaceProbe::reporting(u64::MAX)),
        )
        .unwrap()
    }

    /// Counts fetches and serves the compile-time mock's synthetic tile.
    struct CountingRasterTileProvider {
        fetch_call_count: AtomicUsize,
    }

    impl CountingRasterTileProvider {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                fetch_call_count: AtomicUsize::new(0),
            })
        }
    }

    #[async_trait::async_trait]
    impl RasterTileProvider for CountingRasterTileProvider {
        async fn fetch_raster_tile(
            &self,
            tile_x: u32,
            tile_y: u32,
            zoom_level: u8,
            image_format: RasterImageFormat,
            tile_resolution: RasterTileResolution,
        ) -> Result<RasterTileData, SDKError> {
            self.fetch_call_count.fetch_add(1, Ordering::SeqCst);
            geospatial_traits::mock::MockRasterTileProvider
                .fetch_raster_tile(tile_x, tile_y, zoom_level, image_format, tile_resolution)
                .await
        }
    }

    #[tokio::test]
    async fn two_provider_namespaces_never_share_a_raster_tile() {
        use super::CachedRasterTileProvider;
        let store = sqlite_store("raster-namespaces");
        let alpha_provider = CountingRasterTileProvider::new();
        let beta_provider = CountingRasterTileProvider::new();
        let alpha_wrapper = CachedRasterTileProvider::new(
            alpha_provider.clone(),
            Arc::clone(&store) as Arc<dyn CachedResponseStore<RasterTileData>>,
            "alpha-provider",
        );
        let beta_wrapper = CachedRasterTileProvider::new(
            beta_provider.clone(),
            Arc::clone(&store) as Arc<dyn CachedResponseStore<RasterTileData>>,
            "beta-provider",
        );
        alpha_wrapper
            .fetch_raster_tile(
                2662,
                6285,
                14,
                RasterImageFormat::Png,
                RasterTileResolution::Standard,
            )
            .await
            .unwrap();
        beta_wrapper
            .fetch_raster_tile(
                2662,
                6285,
                14,
                RasterImageFormat::Png,
                RasterTileResolution::Standard,
            )
            .await
            .unwrap();
        assert_eq!(
            alpha_provider.fetch_call_count.load(Ordering::SeqCst),
            1,
            "the second provider's tile must not be served from the first's entry"
        );
        assert_eq!(beta_provider.fetch_call_count.load(Ordering::SeqCst), 1);
        // Each provider's tile is stored under its own namespaced key.
        for (namespace, wrapper) in [
            ("alpha-provider", &alpha_wrapper),
            ("beta-provider", &beta_wrapper),
        ] {
            assert_eq!(wrapper.tile_source_namespace(), namespace);
            let descriptor = super::tile_request_descriptor(
                namespace,
                2662,
                6285,
                14,
                RasterImageFormat::Png,
                RasterTileResolution::Standard,
            );
            assert!(
                CachedResponseStore::<RasterTileData>::lookup_response(
                    store.as_ref(),
                    &descriptor.to_fingerprint()
                )
                .unwrap()
                .is_some()
            );
        }
    }
}
