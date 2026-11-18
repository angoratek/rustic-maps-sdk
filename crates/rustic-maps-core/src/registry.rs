//! Provider registry: one optional slot per provider contract.
//!
//! The application registers its provider implementations at startup and the
//! rest of the SDK resolves them by contract. Nothing in this module depends
//! on a concrete provider.

use crate::style_resources::StyleResourceClient;
use crate::traffic_flow_cache::{TrafficFlowCacheReadback, TrafficFlowGridCache};
use geospatial_traits::{
    DistanceMatrixProvider, ElevationProvider, GeocodingProvider, OfflineRoutingProvider,
    RasterTileProvider, RoutingProvider, SearchPlaceProvider, TrafficFlowProvider,
    TrafficIncidentProvider, VectorTileProvider,
};
use geospatial_types::SDKError;
use std::sync::Arc;

/// The contract a registry slot serves.
///
/// Each variant names one capability an application can configure
/// independently, which is what lets a map draw its imagery from one provider
/// while another plans its routes. [`ProviderContract::contract_name`] is the
/// name the [`SDKError::ProviderNotConfigured`] detail carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ProviderContract {
    /// Forward and reverse address lookup.
    Geocoding,
    /// Route calculation through ordered waypoints.
    Routing,
    /// Travel-time and distance matrix calculation.
    DistanceMatrix,
    /// Place prediction and lookup by free text.
    PlaceSearch,
    /// Traffic incident reports.
    TrafficIncidents,
    /// Traffic flow along road segments.
    TrafficFlow,
    /// Terrain elevation lookup.
    Elevation,
    /// Raster map imagery.
    RasterTiles,
    /// Vector map tiles.
    VectorTiles,
    /// Route calculation on a compiled offline graph.
    OfflineRouting,
}

impl ProviderContract {
    /// Every contract the registry defines, in the order a settings surface
    /// lists them: the request/response capabilities first, then the two map
    /// imagery contracts, then offline routing.
    pub const ALL: [Self; 10] = [
        Self::Geocoding,
        Self::Routing,
        Self::DistanceMatrix,
        Self::PlaceSearch,
        Self::TrafficIncidents,
        Self::TrafficFlow,
        Self::Elevation,
        Self::RasterTiles,
        Self::VectorTiles,
        Self::OfflineRouting,
    ];

    /// The name this contract reports in a
    /// [`SDKError::ProviderNotConfigured`] detail.
    pub fn contract_name(self) -> &'static str {
        match self {
            Self::Geocoding => "GeocodingProvider",
            Self::Routing => "RoutingProvider",
            Self::DistanceMatrix => "DistanceMatrixProvider",
            Self::PlaceSearch => "SearchPlaceProvider",
            Self::TrafficIncidents => "TrafficIncidentProvider",
            Self::TrafficFlow => "TrafficFlowProvider",
            Self::Elevation => "ElevationProvider",
            Self::RasterTiles => "RasterTileProvider",
            Self::VectorTiles => "VectorTileProvider",
            Self::OfflineRouting => "OfflineRoutingProvider",
        }
    }
}

/// Registry of provider implementations keyed by contract, not by vendor.
///
/// Each contract has one optional slot. Slots start unconfigured. Callers
/// resolve them with the `require_*` methods, which report
/// [`SDKError::ProviderNotConfigured`] when the application did not register
/// that provider.
///
/// The registry is cheap to share after configuration: every provider is
/// stored behind an [`Arc`].
#[derive(Default)]
pub struct ProviderRegistry {
    geocoding_provider: Option<Arc<dyn GeocodingProvider>>,
    routing_provider: Option<Arc<dyn RoutingProvider>>,
    distance_matrix_provider: Option<Arc<dyn DistanceMatrixProvider>>,
    search_place_provider: Option<Arc<dyn SearchPlaceProvider>>,
    traffic_incident_provider: Option<Arc<dyn TrafficIncidentProvider>>,
    traffic_flow_provider: Option<Arc<dyn TrafficFlowProvider>>,
    /// The grid cache wrapping the serving traffic-flow provider, kept so a
    /// diagnostics readback can report its counters and budget. `None` while
    /// the provider owns its own freshness policy.
    traffic_flow_cache: Option<Arc<TrafficFlowGridCache>>,
    elevation_provider: Option<Arc<dyn ElevationProvider>>,
    raster_tile_provider: Option<Arc<dyn RasterTileProvider>>,
    vector_tile_provider: Option<Arc<dyn VectorTileProvider>>,
    /// The authenticated fetch of the vector-tile provider's published style
    /// resources (its style document, glyph ranges and sprites). `None` when
    /// the provider publishes none or serves only committed styles.
    vector_tile_style_resources: Option<Arc<dyn StyleResourceClient>>,
    /// The hosts the serving provider declared for its published style
    /// resources. A style document's resource address is fetchable only on the
    /// document's own origin or on one of these hosts, so a document cannot
    /// aim the provider credential at a host the provider did not declare.
    vector_tile_style_resource_hosts: Vec<String>,
    offline_routing_provider: Option<Arc<dyn OfflineRoutingProvider>>,
}

impl std::fmt::Debug for ProviderRegistry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderRegistry")
            .field("geocoding_provider", &self.geocoding_provider.is_some())
            .field("routing_provider", &self.routing_provider.is_some())
            .field(
                "distance_matrix_provider",
                &self.distance_matrix_provider.is_some(),
            )
            .field(
                "search_place_provider",
                &self.search_place_provider.is_some(),
            )
            .field(
                "traffic_incident_provider",
                &self.traffic_incident_provider.is_some(),
            )
            .field(
                "traffic_flow_provider",
                &self.traffic_flow_provider.is_some(),
            )
            .field("elevation_provider", &self.elevation_provider.is_some())
            .field("raster_tile_provider", &self.raster_tile_provider.is_some())
            .field("vector_tile_provider", &self.vector_tile_provider.is_some())
            .field(
                "vector_tile_style_resources",
                &self.vector_tile_style_resources.is_some(),
            )
            .field(
                "offline_routing_provider",
                &self.offline_routing_provider.is_some(),
            )
            .finish()
    }
}

fn not_configured_error(contract_name: &str) -> SDKError {
    SDKError::ProviderNotConfigured(contract_name.to_string())
}

impl ProviderRegistry {
    /// Creates a registry with every contract slot unconfigured.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers the geocoding implementation, replacing any earlier one.
    pub fn register_geocoding_provider(&mut self, provider: Arc<dyn GeocodingProvider>) {
        self.geocoding_provider = Some(provider);
    }

    /// Registers the routing implementation, replacing any earlier one.
    pub fn register_routing_provider(&mut self, provider: Arc<dyn RoutingProvider>) {
        self.routing_provider = Some(provider);
    }

    /// Registers the distance-matrix implementation, replacing any earlier one.
    pub fn register_distance_matrix_provider(&mut self, provider: Arc<dyn DistanceMatrixProvider>) {
        self.distance_matrix_provider = Some(provider);
    }

    /// Registers the search implementation, replacing any earlier one.
    pub fn register_search_place_provider(&mut self, provider: Arc<dyn SearchPlaceProvider>) {
        self.search_place_provider = Some(provider);
    }

    /// Registers the traffic-incident implementation, replacing any earlier
    /// one.
    pub fn register_traffic_incident_provider(
        &mut self,
        provider: Arc<dyn TrafficIncidentProvider>,
    ) {
        self.traffic_incident_provider = Some(provider);
    }

    /// Registers the traffic-flow implementation, replacing any earlier one.
    ///
    /// A provider registered here answers uncached: it owns its own freshness
    /// policy, so the registry holds no cache to report.
    pub fn register_traffic_flow_provider(&mut self, provider: Arc<dyn TrafficFlowProvider>) {
        self.traffic_flow_provider = Some(provider);
        self.traffic_flow_cache = None;
    }

    /// Registers the traffic-flow implementation behind the grid cache that
    /// answers it, replacing any earlier provider and cache. The statistics
    /// readback reports that cache's counters while this provider serves.
    pub fn register_traffic_flow_provider_with_cache(
        &mut self,
        provider: Arc<dyn TrafficFlowProvider>,
        cache: Arc<TrafficFlowGridCache>,
    ) {
        self.traffic_flow_provider = Some(provider);
        self.traffic_flow_cache = Some(cache);
    }

    /// The serving traffic-flow cache's counters and budget, or `None` while
    /// no traffic-flow provider is registered or the provider answers
    /// uncached.
    pub fn traffic_flow_cache_statistics(&self) -> Option<TrafficFlowCacheReadback> {
        self.traffic_flow_cache
            .as_ref()
            .map(|cache| TrafficFlowCacheReadback {
                statistics: cache.statistics(),
                configuration: cache.configuration(),
            })
    }

    /// Registers the elevation implementation, replacing any earlier one.
    pub fn register_elevation_provider(&mut self, provider: Arc<dyn ElevationProvider>) {
        self.elevation_provider = Some(provider);
    }

    /// Registers the raster-tile implementation, replacing any earlier one.
    pub fn register_raster_tile_provider(&mut self, provider: Arc<dyn RasterTileProvider>) {
        self.raster_tile_provider = Some(provider);
    }

    /// Registers the vector-tile implementation, replacing any earlier one.
    ///
    /// A style-resource fetch registered for the previous provider is dropped
    /// with it: the fetch belongs to the provider whose client carries the
    /// credential, so the caller registers the new provider's fetch after
    /// this call.
    pub fn register_vector_tile_provider(&mut self, provider: Arc<dyn VectorTileProvider>) {
        self.vector_tile_provider = Some(provider);
        self.vector_tile_style_resources = None;
        self.vector_tile_style_resource_hosts.clear();
    }

    /// Registers the vector-tile provider's authenticated fetch of the style
    /// resources it publishes, replacing any earlier one.
    ///
    /// # Parameters
    /// - `style_resources` — the authenticated fetch, which applies the
    ///   provider's credential to every address it requests.
    /// - `declared_hosts` — the hosts the provider publishes its style
    ///   resources on, besides the style document's own origin. The renderer
    ///   refuses any other host before a request is made.
    pub fn register_vector_tile_style_resources(
        &mut self,
        style_resources: Arc<dyn StyleResourceClient>,
        declared_hosts: Vec<String>,
    ) {
        self.vector_tile_style_resources = Some(style_resources);
        self.vector_tile_style_resource_hosts = declared_hosts;
    }

    /// Drops the vector-tile provider and the style-resource fetch registered
    /// beside it, so a cleared registry answers neither slot.
    pub fn clear_vector_tile_provider(&mut self) {
        self.vector_tile_provider = None;
        self.vector_tile_style_resources = None;
        self.vector_tile_style_resource_hosts.clear();
    }

    /// Registers the offline-routing implementation, replacing any earlier
    /// one.
    pub fn register_offline_routing_provider(&mut self, provider: Arc<dyn OfflineRoutingProvider>) {
        self.offline_routing_provider = Some(provider);
    }

    /// Returns the geocoding provider or an [`SDKError::ProviderNotConfigured`] error.
    pub fn require_geocoding_provider(&self) -> Result<Arc<dyn GeocodingProvider>, SDKError> {
        self.geocoding_provider
            .clone()
            .ok_or_else(|| not_configured_error("GeocodingProvider"))
    }

    /// Returns the routing provider or an [`SDKError::ProviderNotConfigured`] error.
    pub fn require_routing_provider(&self) -> Result<Arc<dyn RoutingProvider>, SDKError> {
        self.routing_provider
            .clone()
            .ok_or_else(|| not_configured_error("RoutingProvider"))
    }

    /// Returns the distance-matrix provider or an [`SDKError::ProviderNotConfigured`] error.
    pub fn require_distance_matrix_provider(
        &self,
    ) -> Result<Arc<dyn DistanceMatrixProvider>, SDKError> {
        self.distance_matrix_provider
            .clone()
            .ok_or_else(|| not_configured_error("DistanceMatrixProvider"))
    }

    /// Returns the search provider or an [`SDKError::ProviderNotConfigured`] error.
    pub fn require_search_place_provider(&self) -> Result<Arc<dyn SearchPlaceProvider>, SDKError> {
        self.search_place_provider
            .clone()
            .ok_or_else(|| not_configured_error("SearchPlaceProvider"))
    }

    /// Returns the traffic-incident provider or an [`SDKError::ProviderNotConfigured`] error.
    pub fn require_traffic_incident_provider(
        &self,
    ) -> Result<Arc<dyn TrafficIncidentProvider>, SDKError> {
        self.traffic_incident_provider
            .clone()
            .ok_or_else(|| not_configured_error("TrafficIncidentProvider"))
    }

    /// Returns the traffic-flow provider or an [`SDKError::ProviderNotConfigured`] error.
    pub fn require_traffic_flow_provider(&self) -> Result<Arc<dyn TrafficFlowProvider>, SDKError> {
        self.traffic_flow_provider
            .clone()
            .ok_or_else(|| not_configured_error("TrafficFlowProvider"))
    }

    /// Returns the elevation provider or an [`SDKError::ProviderNotConfigured`] error.
    pub fn require_elevation_provider(&self) -> Result<Arc<dyn ElevationProvider>, SDKError> {
        self.elevation_provider
            .clone()
            .ok_or_else(|| not_configured_error("ElevationProvider"))
    }

    /// Returns the raster-tile provider or an [`SDKError::ProviderNotConfigured`] error.
    pub fn require_raster_tile_provider(&self) -> Result<Arc<dyn RasterTileProvider>, SDKError> {
        self.raster_tile_provider
            .clone()
            .ok_or_else(|| not_configured_error("RasterTileProvider"))
    }

    /// Returns the vector-tile provider or an [`SDKError::ProviderNotConfigured`] error.
    pub fn require_vector_tile_provider(&self) -> Result<Arc<dyn VectorTileProvider>, SDKError> {
        self.vector_tile_provider
            .clone()
            .ok_or_else(|| not_configured_error("VectorTileProvider"))
    }

    /// Returns the vector-tile provider's authenticated style-resource fetch
    /// or a [`SDKError::ProviderNotConfigured`] error naming it.
    pub fn require_vector_tile_style_resources(
        &self,
    ) -> Result<Arc<dyn StyleResourceClient>, SDKError> {
        self.vector_tile_style_resources
            .clone()
            .ok_or_else(|| not_configured_error("vector-tile style resources"))
    }

    /// The hosts the serving provider declared for its published style
    /// resources, empty while nothing is registered.
    pub fn vector_tile_style_resource_hosts(&self) -> Vec<String> {
        self.vector_tile_style_resource_hosts.clone()
    }

    /// Returns the offline-routing provider or an [`SDKError::ProviderNotConfigured`] error.
    pub fn require_offline_routing_provider(
        &self,
    ) -> Result<Arc<dyn OfflineRoutingProvider>, SDKError> {
        self.offline_routing_provider
            .clone()
            .ok_or_else(|| not_configured_error("OfflineRoutingProvider"))
    }

    /// Whether the contract's slot holds a provider.
    ///
    /// The check answers without constructing anything, so a settings surface
    /// can list which capabilities are served before a call is attempted.
    pub fn is_configured(&self, contract: ProviderContract) -> bool {
        match contract {
            ProviderContract::Geocoding => self.geocoding_provider.is_some(),
            ProviderContract::Routing => self.routing_provider.is_some(),
            ProviderContract::DistanceMatrix => self.distance_matrix_provider.is_some(),
            ProviderContract::PlaceSearch => self.search_place_provider.is_some(),
            ProviderContract::TrafficIncidents => self.traffic_incident_provider.is_some(),
            ProviderContract::TrafficFlow => self.traffic_flow_provider.is_some(),
            ProviderContract::Elevation => self.elevation_provider.is_some(),
            ProviderContract::RasterTiles => self.raster_tile_provider.is_some(),
            ProviderContract::VectorTiles => self.vector_tile_provider.is_some(),
            ProviderContract::OfflineRouting => self.offline_routing_provider.is_some(),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::ProviderRegistry;
    use geospatial_traits::mock::{
        MockGeocodingProvider, MockRasterTileProvider, MockVectorTileProvider,
    };
    use geospatial_types::SDKError;
    use std::sync::Arc;

    #[test]
    fn unregistered_slots_report_provider_not_configured() {
        let registry = ProviderRegistry::new();
        let failure = match registry.require_geocoding_provider() {
            Err(failure) => failure,
            Ok(_) => panic!("expected ProviderNotConfigured for GeocodingProvider"),
        };
        assert_eq!(
            failure,
            SDKError::ProviderNotConfigured("GeocodingProvider".to_string())
        );
    }

    #[test]
    fn registered_slots_resolve_through_trait_objects() {
        let mut registry = ProviderRegistry::new();
        registry.register_geocoding_provider(Arc::new(MockGeocodingProvider));
        registry.register_raster_tile_provider(Arc::new(MockRasterTileProvider));

        assert!(registry.require_geocoding_provider().is_ok());
        assert!(registry.require_raster_tile_provider().is_ok());
        assert!(registry.require_routing_provider().is_err());
    }

    #[test]
    fn vector_tile_slot_resolves_through_trait_objects() {
        let mut registry = ProviderRegistry::new();
        registry.register_vector_tile_provider(Arc::new(MockVectorTileProvider));
        assert!(registry.require_vector_tile_provider().is_ok());
        let failure = match registry.require_routing_provider() {
            Err(failure) => failure,
            Ok(_) => panic!("expected ProviderNotConfigured for RoutingProvider"),
        };
        assert_eq!(
            failure,
            SDKError::ProviderNotConfigured("RoutingProvider".to_string())
        );
    }

    /// A style-resource fetch that answers no bytes; the registry only stores
    /// and resolves the handle.
    struct StubStyleResourceClient;

    #[async_trait::async_trait]
    impl crate::style_resources::StyleResourceClient for StubStyleResourceClient {
        async fn fetch_style_resource(&self, _url: &str) -> Result<Vec<u8>, SDKError> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn the_style_resource_fetch_and_its_declared_hosts_follow_the_vector_tile_slot() {
        let mut registry = ProviderRegistry::new();
        assert!(
            registry.require_vector_tile_style_resources().is_err(),
            "an empty registry resolves no style-resource fetch"
        );
        assert!(
            registry.vector_tile_style_resource_hosts().is_empty(),
            "an empty registry declares no style-resource host"
        );
        registry.register_vector_tile_provider(Arc::new(MockVectorTileProvider));
        registry.register_vector_tile_style_resources(
            Arc::new(StubStyleResourceClient),
            vec!["assets.example.test".to_string()],
        );
        assert!(registry.require_vector_tile_style_resources().is_ok());
        assert_eq!(
            registry.vector_tile_style_resource_hosts(),
            vec!["assets.example.test".to_string()]
        );
        // A replacement provider drops the previous provider's fetch and its
        // declared hosts, because that fetch carries the replaced provider's
        // credential.
        registry.register_vector_tile_provider(Arc::new(MockVectorTileProvider));
        assert!(registry.require_vector_tile_style_resources().is_err());
        assert!(registry.vector_tile_style_resource_hosts().is_empty());
        registry.register_vector_tile_style_resources(
            Arc::new(StubStyleResourceClient),
            vec!["assets.example.test".to_string()],
        );
        registry.clear_vector_tile_provider();
        assert!(
            registry.require_vector_tile_provider().is_err(),
            "clearing drops the provider with its style-resource fetch"
        );
        assert!(registry.require_vector_tile_style_resources().is_err());
        assert!(registry.vector_tile_style_resource_hosts().is_empty());
    }

    #[test]
    fn the_traffic_flow_cache_readback_follows_the_serving_provider() {
        use crate::traffic_flow_cache::{
            GridCachedTrafficFlowProvider, TrafficFlowCacheConfiguration,
        };

        let mut registry = ProviderRegistry::new();
        assert!(
            registry.traffic_flow_cache_statistics().is_none(),
            "an empty registry has no cache to report"
        );

        let caching_provider = GridCachedTrafficFlowProvider::new(
            Arc::new(geospatial_traits::mock::MockTrafficFlowProvider),
            TrafficFlowCacheConfiguration::default(),
            Arc::new(|| 0),
        )
        .expect("the default configuration must be accepted");
        let cache = caching_provider.cache();
        registry.register_traffic_flow_provider_with_cache(Arc::new(caching_provider), cache);
        let readback = registry
            .traffic_flow_cache_statistics()
            .expect("a cached registration reports its cache");
        assert_eq!(readback.statistics.miss_count, 0);
        assert_eq!(
            readback.configuration,
            TrafficFlowCacheConfiguration::default()
        );

        // A provider that owns its own freshness policy replaces both the
        // provider and the cache the readback reported.
        registry.register_traffic_flow_provider(Arc::new(
            geospatial_traits::mock::MockTrafficFlowProvider,
        ));
        assert!(
            registry.traffic_flow_cache_statistics().is_none(),
            "an uncached provider leaves no cache statistics behind"
        );
    }
}
