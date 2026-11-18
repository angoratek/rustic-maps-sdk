//! The foreign-callback provider injection surface: the exported callback
//! contracts that foreign code (Kotlin, Swift) implements to serve a
//! provider contract of the SDK, and the adapters that bridge those
//! implementations onto the native `geospatial-traits` traits behind the
//! `rustic-maps-core` [`ProviderRegistry`](rustic_maps_core::ProviderRegistry).
//!
//! The contract names here are the plain provider-contract names of the UDL
//! (`GeocodingProvider`, `RasterTileProvider`), so they resolve exactly as
//! the uniffi-generated scaffolding expects while staying the names a
//! foreign implementor reads (see the UDL provider-contract documentation).
//! The callback methods exchange binding records and throw
//! [`crate::SdkError`]; the adapters convert the mirror records back to the
//! native `geospatial-types` values through the [`crate::records`]
//! converters and re-raise the binding error as the core error, so the
//! provider registry sees an ordinary native provider implementation.
//!
//! The adapters are also the provider-failure diagnostic seam's write sites:
//! each holds the owning `MapSdk`'s [`ProviderFailureDiagnosticsRing`] and
//! records every failed foreign contract call with the contract and the
//! method it occurred on, its classification and the verbatim failure text
//! (see the inline `foreign_failure_with_diagnostic` helper).
//!
//! The traits are `#[async_trait]` desugared to match the generated
//! foreign-callback adapters, which uniffi emits implementing these same
//! traits over the foreign vtable.

use std::sync::Arc;

use geospatial_traits as native_traits;
use geospatial_types::SDKError;

use crate::provider_diagnostics::{
    ProviderFailureDiagnosticsRing, SharedProviderFailureDiagnostics,
};
use crate::records::{
    BoundingBox, DistanceMatrix, ElevationPoint, GeographicCoordinate, Place,
    ProviderFailureClassification, RasterImageFormat, RasterTileData, RasterTileResolution, Route,
    RouteOptions, TrafficFlowData, TrafficIncident, TransportationMode, VectorTileData,
    VectorTileSourceDescriptor,
};

/// Contract name the provider-failure diagnostic seam reports for the
/// geocoding callback contract (the plain UDL contract name).
const GEOCODING_CONTRACT_NAME: &str = "GeocodingProvider";
/// Contract name the provider-failure diagnostic seam reports for the
/// raster-tile callback contract (the plain UDL contract name).
const RASTER_TILE_CONTRACT_NAME: &str = "RasterTileProvider";
/// Method name the provider-failure diagnostic seam reports for the
/// forward-geocode callback method.
const FORWARD_GEOCODE_METHOD_NAME: &str = "forward_geocode";
/// Method name the provider-failure diagnostic seam reports for the
/// reverse-geocode callback method.
const REVERSE_GEOCODE_METHOD_NAME: &str = "reverse_geocode";
/// Method name the provider-failure diagnostic seam reports for the
/// raster-tile-fetch callback method.
const FETCH_RASTER_TILE_METHOD_NAME: &str = "fetch_raster_tile";
/// Contract name the provider-failure diagnostic seam reports for the
/// vector-tile callback contract (the plain UDL contract name).
const VECTOR_TILE_CONTRACT_NAME: &str = "VectorTileProvider";
/// Method name the provider-failure diagnostic seam reports for the
/// vector-tile-fetch callback method.
const FETCH_VECTOR_TILE_METHOD_NAME: &str = "fetch_vector_tile";
/// Contract name the provider-failure diagnostic seam reports for the routing
/// callback contract (the plain UDL contract name).
const ROUTING_CONTRACT_NAME: &str = "RoutingProvider";
/// Contract name the provider-failure diagnostic seam reports for the
/// distance-matrix callback contract (the plain UDL contract name).
const DISTANCE_MATRIX_CONTRACT_NAME: &str = "DistanceMatrixProvider";
/// Contract name the provider-failure diagnostic seam reports for the
/// place-search callback contract (the plain UDL contract name).
const SEARCH_PLACE_CONTRACT_NAME: &str = "SearchPlaceProvider";
/// Contract name the provider-failure diagnostic seam reports for the
/// traffic-incident callback contract (the plain UDL contract name).
const TRAFFIC_INCIDENT_CONTRACT_NAME: &str = "TrafficIncidentProvider";
/// Contract name the provider-failure diagnostic seam reports for the
/// traffic-flow callback contract (the plain UDL contract name).
const TRAFFIC_FLOW_CONTRACT_NAME: &str = "TrafficFlowProvider";
/// Contract name the provider-failure diagnostic seam reports for the
/// elevation callback contract (the plain UDL contract name).
const ELEVATION_CONTRACT_NAME: &str = "ElevationProvider";
/// Method name the provider-failure diagnostic seam reports for the
/// route-calculation callback method.
const CALCULATE_ROUTE_METHOD_NAME: &str = "calculate_route";
/// Method name the provider-failure diagnostic seam reports for the
/// distance-matrix callback method.
const CALCULATE_DISTANCE_MATRIX_METHOD_NAME: &str = "calculate_distance_matrix";
/// Method name the provider-failure diagnostic seam reports for the
/// place-autocomplete callback method.
const SEARCH_AUTOCOMPLETE_METHOD_NAME: &str = "search_autocomplete";
/// Method name the provider-failure diagnostic seam reports for the
/// place-search callback method.
const SEARCH_PLACES_METHOD_NAME: &str = "search_places";
/// Method name the provider-failure diagnostic seam reports for the
/// traffic-incident callback method.
const FETCH_TRAFFIC_INCIDENTS_METHOD_NAME: &str = "fetch_traffic_incidents";
/// Method name the provider-failure diagnostic seam reports for the
/// traffic-flow callback method.
const FETCH_TRAFFIC_FLOW_METHOD_NAME: &str = "fetch_traffic_flow";
/// Method name the provider-failure diagnostic seam reports for the
/// elevation callback method.
const FETCH_ELEVATION_POINTS_METHOD_NAME: &str = "fetch_elevation_points";

/// Geocoding contract implemented by foreign code and registered on
/// `MapSdk` through `register_geocoding_provider_from_foreign`.
#[async_trait::async_trait]
pub trait GeocodingProvider: Send + Sync {
    /// Resolves a free-form address text into candidate places.
    async fn forward_geocode(&self, address_text: String) -> Result<Vec<Place>, crate::SdkError>;

    /// Resolves a coordinate into candidate places describing that position.
    async fn reverse_geocode(
        &self,
        coordinate: GeographicCoordinate,
    ) -> Result<Vec<Place>, crate::SdkError>;

    /// Internally required by the uniffi foreign-handle machinery (the
    /// foreign-handle probe of the uniffi 0.31 trait-object lowering);
    /// Rust-side implementations keep the default, which reports a
    /// Rust-implemented object.
    #[doc(hidden)]
    fn uniffi_foreign_handle(&self) -> Option<::uniffi::Handle> {
        Option::None
    }
}

/// Raster-tile contract implemented by foreign code and registered on
/// `MapSdk` through `register_raster_tile_provider_from_foreign`.
#[async_trait::async_trait]
pub trait RasterTileProvider: Send + Sync {
    /// Fetches one web-mercator XYZ raster tile at the requested resolution.
    async fn fetch_raster_tile(
        &self,
        tile_x: u32,
        tile_y: u32,
        zoom_level: u8,
        image_format: RasterImageFormat,
        tile_resolution: RasterTileResolution,
    ) -> Result<RasterTileData, crate::SdkError>;

    /// Internally required by the uniffi foreign-handle machinery (see
    /// [`GeocodingProvider::uniffi_foreign_handle`]).
    #[doc(hidden)]
    fn uniffi_foreign_handle(&self) -> Option<::uniffi::Handle> {
        Option::None
    }
}

/// Bridges a foreign implementation of the [`GeocodingProvider`] callback
/// contract onto the native `geospatial-traits` trait: an ordinary native
/// provider for the registry whose calls lower into the foreign
/// implementation, recording every failed call on the owning `MapSdk`'s
/// provider-failure diagnostic ring.
pub(crate) struct ForeignGeocodingProvider {
    /// The foreign implementation serving the contract calls.
    geocoding_implementation: Arc<dyn GeocodingProvider>,
    /// The owning `MapSdk`'s provider-failure diagnostics ring.
    provider_failure_diagnostics: SharedProviderFailureDiagnostics,
}

impl ForeignGeocodingProvider {
    /// Wraps the foreign geocoding implementation for native registration.
    pub(crate) fn new(
        geocoding_implementation: Arc<dyn GeocodingProvider>,
        provider_failure_diagnostics: SharedProviderFailureDiagnostics,
    ) -> Self {
        Self {
            geocoding_implementation,
            provider_failure_diagnostics,
        }
    }
}

#[async_trait::async_trait]
impl native_traits::GeocodingProvider for ForeignGeocodingProvider {
    async fn forward_geocode(
        &self,
        address_text: &str,
    ) -> Result<Vec<geospatial_types::PlaceResult>, SDKError> {
        let foreign_places = self
            .geocoding_implementation
            .forward_geocode(address_text.to_string())
            .await
            .map_err(|foreign_failure| {
                foreign_failure_with_diagnostic(
                    &self.provider_failure_diagnostics,
                    GEOCODING_CONTRACT_NAME,
                    FORWARD_GEOCODE_METHOD_NAME,
                    foreign_failure,
                )
            })?;
        Ok(foreign_places
            .into_iter()
            .map(Place::into_native_place_result)
            .collect())
    }

    async fn reverse_geocode(
        &self,
        coordinate: &geospatial_types::GeographicCoordinate,
    ) -> Result<Vec<geospatial_types::PlaceResult>, SDKError> {
        let foreign_places = self
            .geocoding_implementation
            .reverse_geocode(GeographicCoordinate::from_native_coordinate(coordinate))
            .await
            .map_err(|foreign_failure| {
                foreign_failure_with_diagnostic(
                    &self.provider_failure_diagnostics,
                    GEOCODING_CONTRACT_NAME,
                    REVERSE_GEOCODE_METHOD_NAME,
                    foreign_failure,
                )
            })?;
        Ok(foreign_places
            .into_iter()
            .map(Place::into_native_place_result)
            .collect())
    }
}

/// Bridges a foreign implementation of the [`RasterTileProvider`] callback
/// contract onto the native `geospatial-traits` trait: an ordinary native
/// provider for the registry whose calls lower into the foreign
/// implementation, recording every failed call on the owning `MapSdk`'s
/// provider-failure diagnostic ring.
pub(crate) struct ForeignRasterTileProvider {
    /// The foreign implementation serving the contract calls.
    raster_tile_implementation: Arc<dyn RasterTileProvider>,
    /// The owning `MapSdk`'s provider-failure diagnostics ring.
    provider_failure_diagnostics: SharedProviderFailureDiagnostics,
}

impl ForeignRasterTileProvider {
    /// Wraps the foreign raster-tile implementation for native registration.
    pub(crate) fn new(
        raster_tile_implementation: Arc<dyn RasterTileProvider>,
        provider_failure_diagnostics: SharedProviderFailureDiagnostics,
    ) -> Self {
        Self {
            raster_tile_implementation,
            provider_failure_diagnostics,
        }
    }
}

#[async_trait::async_trait]
impl native_traits::RasterTileProvider for ForeignRasterTileProvider {
    async fn fetch_raster_tile(
        &self,
        tile_x: u32,
        tile_y: u32,
        zoom_level: u8,
        image_format: geospatial_types::RasterImageFormat,
        tile_resolution: geospatial_types::RasterTileResolution,
    ) -> Result<geospatial_types::RasterTileData, SDKError> {
        let foreign_tile = self
            .raster_tile_implementation
            .fetch_raster_tile(
                tile_x,
                tile_y,
                zoom_level,
                RasterImageFormat::from_native_image_format(image_format),
                RasterTileResolution::from_native_tile_resolution(tile_resolution),
            )
            .await
            .map_err(|foreign_failure| {
                foreign_failure_with_diagnostic(
                    &self.provider_failure_diagnostics,
                    RASTER_TILE_CONTRACT_NAME,
                    FETCH_RASTER_TILE_METHOD_NAME,
                    foreign_failure,
                )
            })?;
        Ok(foreign_tile.into_native_raster_tile_data())
    }
}

/// Vector-tile contract implemented by foreign code and registered on
/// `MapSdk` through `register_vector_tile_provider_from_foreign`.
#[async_trait::async_trait]
pub trait VectorTileProvider: Send + Sync {
    /// Describes the served tileset: tileset name, attribution, zoom span,
    /// wire encoding and style hints.
    fn source_descriptor(&self) -> VectorTileSourceDescriptor;

    /// Fetches one web-mercator XYZ vector tile in its wire encoding.
    async fn fetch_vector_tile(
        &self,
        tile_x: u32,
        tile_y: u32,
        zoom_level: u8,
    ) -> Result<VectorTileData, crate::SdkError>;

    /// Internally required by the uniffi foreign-handle machinery (see
    /// [`GeocodingProvider::uniffi_foreign_handle`]).
    #[doc(hidden)]
    fn uniffi_foreign_handle(&self) -> Option<::uniffi::Handle> {
        Option::None
    }
}

/// Bridges a foreign implementation of the [`VectorTileProvider`] callback
/// contract onto the native `geospatial-traits` trait: an ordinary native
/// provider for the registry whose calls lower into the foreign
/// implementation, recording every failed call on the owning `MapSdk`'s
/// provider-failure diagnostic ring.
pub(crate) struct ForeignVectorTileProvider {
    /// The foreign implementation serving the contract calls.
    vector_tile_implementation: Arc<dyn VectorTileProvider>,
    /// The owning `MapSdk`'s provider-failure diagnostics ring.
    provider_failure_diagnostics: SharedProviderFailureDiagnostics,
}

impl ForeignVectorTileProvider {
    /// Wraps the foreign vector-tile implementation for native registration.
    pub(crate) fn new(
        vector_tile_implementation: Arc<dyn VectorTileProvider>,
        provider_failure_diagnostics: SharedProviderFailureDiagnostics,
    ) -> Self {
        Self {
            vector_tile_implementation,
            provider_failure_diagnostics,
        }
    }
}

#[async_trait::async_trait]
impl native_traits::VectorTileProvider for ForeignVectorTileProvider {
    fn source_descriptor(&self) -> geospatial_traits::VectorTileSourceDescriptor {
        self.vector_tile_implementation
            .source_descriptor()
            .into_native_vector_tile_source_descriptor()
    }

    async fn fetch_vector_tile(
        &self,
        tile_x: u32,
        tile_y: u32,
        zoom_level: u8,
    ) -> Result<geospatial_types::VectorTileData, SDKError> {
        let foreign_tile = self
            .vector_tile_implementation
            .fetch_vector_tile(tile_x, tile_y, zoom_level)
            .await
            .map_err(|foreign_failure| {
                foreign_failure_with_diagnostic(
                    &self.provider_failure_diagnostics,
                    VECTOR_TILE_CONTRACT_NAME,
                    FETCH_VECTOR_TILE_METHOD_NAME,
                    foreign_failure,
                )
            })?;
        Ok(foreign_tile.into_native_vector_tile_data())
    }
}

/// The provider-failure diagnostic seam's conversion helper: records one
/// failed foreign-callback provider call on the owning `MapSdk`'s ring — the
/// contract and the callback method it occurred on, its classification, and
/// the verbatim failure text — and then converts the foreign
/// implementation's binding failure into the core error the registry path
/// carries.
///
/// The classification is mechanical: a failure carrying the
/// unclassified-callback conversion's verbatim detail prefix is that
/// conversion's subject and records the callback machinery's embedded reason
/// under `UnclassifiedCallbackFailure`; every other failure was a foreign
/// implementation's own typed `SdkError` throw and records its display form
/// under `MappedContractError`. A foreign implementation throwing the
/// conversion's exact phrasing is indistinguishable; the UDL documentation
/// states that limitation.
fn foreign_failure_with_diagnostic(
    provider_failure_diagnostics: &ProviderFailureDiagnosticsRing,
    provider_contract_name: &str,
    provider_method_name: &str,
    foreign_failure: crate::SdkError,
) -> SDKError {
    let (failure_classification, failure_detail) =
        match foreign_failure.unclassified_foreign_callback_reason() {
            Some(callback_failure_reason) => (
                ProviderFailureClassification::UnclassifiedCallbackFailure,
                callback_failure_reason.to_string(),
            ),
            Option::None => (
                ProviderFailureClassification::MappedContractError,
                foreign_failure.to_string(),
            ),
        };
    provider_failure_diagnostics.record(
        provider_contract_name,
        provider_method_name,
        failure_classification,
        failure_detail,
    );
    SDKError::from(foreign_failure)
}
/// Routing contract implemented by foreign code and registered on `MapSdk`
/// through `register_routing_provider_from_foreign`.
#[async_trait::async_trait]
pub trait RoutingProvider: Send + Sync {
    /// Calculates a route through the ordered waypoints with the given
    /// options.
    async fn calculate_route(
        &self,
        waypoints: Vec<GeographicCoordinate>,
        route_options: RouteOptions,
    ) -> Result<Route, crate::SdkError>;

    /// Internally required by the uniffi foreign-handle machinery (see
    /// [`GeocodingProvider::uniffi_foreign_handle`]).
    #[doc(hidden)]
    fn uniffi_foreign_handle(&self) -> Option<::uniffi::Handle> {
        Option::None
    }
}

/// Distance-matrix contract implemented by foreign code and registered on
/// `MapSdk` through `register_distance_matrix_provider_from_foreign`.
#[async_trait::async_trait]
pub trait DistanceMatrixProvider: Send + Sync {
    /// Calculates durations and distances for every origin-destination pair.
    async fn calculate_distance_matrix(
        &self,
        origins: Vec<GeographicCoordinate>,
        destinations: Vec<GeographicCoordinate>,
        transportation_mode: TransportationMode,
    ) -> Result<DistanceMatrix, crate::SdkError>;

    /// Internally required by the uniffi foreign-handle machinery (see
    /// [`GeocodingProvider::uniffi_foreign_handle`]).
    #[doc(hidden)]
    fn uniffi_foreign_handle(&self) -> Option<::uniffi::Handle> {
        Option::None
    }
}

/// Place-search contract implemented by foreign code and registered on
/// `MapSdk` through `register_search_place_provider_from_foreign`.
#[async_trait::async_trait]
pub trait SearchPlaceProvider: Send + Sync {
    /// Returns fast completion candidates while the user types.
    async fn search_autocomplete(
        &self,
        query_text: String,
        search_boundary: Option<BoundingBox>,
    ) -> Result<Vec<Place>, crate::SdkError>;

    /// Returns full search results for a settled query.
    async fn search_places(
        &self,
        query_text: String,
        search_boundary: Option<BoundingBox>,
    ) -> Result<Vec<Place>, crate::SdkError>;

    /// Internally required by the uniffi foreign-handle machinery (see
    /// [`GeocodingProvider::uniffi_foreign_handle`]).
    #[doc(hidden)]
    fn uniffi_foreign_handle(&self) -> Option<::uniffi::Handle> {
        Option::None
    }
}

/// Traffic-incident contract implemented by foreign code and registered on
/// `MapSdk` through `register_traffic_incident_provider_from_foreign`.
#[async_trait::async_trait]
pub trait TrafficIncidentProvider: Send + Sync {
    /// Fetches incidents intersecting a bounding box.
    async fn fetch_traffic_incidents(
        &self,
        bounding_box: BoundingBox,
    ) -> Result<Vec<TrafficIncident>, crate::SdkError>;

    /// Internally required by the uniffi foreign-handle machinery (see
    /// [`GeocodingProvider::uniffi_foreign_handle`]).
    #[doc(hidden)]
    fn uniffi_foreign_handle(&self) -> Option<::uniffi::Handle> {
        Option::None
    }
}

/// Traffic-flow contract implemented by foreign code and registered on
/// `MapSdk` through `register_traffic_flow_provider_from_foreign`.
#[async_trait::async_trait]
pub trait TrafficFlowProvider: Send + Sync {
    /// Fetches per-segment flow observations inside a bounding box.
    async fn fetch_traffic_flow(
        &self,
        bounding_box: BoundingBox,
    ) -> Result<Vec<TrafficFlowData>, crate::SdkError>;

    /// Internally required by the uniffi foreign-handle machinery (see
    /// [`GeocodingProvider::uniffi_foreign_handle`]).
    #[doc(hidden)]
    fn uniffi_foreign_handle(&self) -> Option<::uniffi::Handle> {
        Option::None
    }
}

/// Elevation contract implemented by foreign code and registered on `MapSdk`
/// through `register_elevation_provider_from_foreign`.
#[async_trait::async_trait]
pub trait ElevationProvider: Send + Sync {
    /// Fetches elevation samples for ordered points.
    async fn fetch_elevation_points(
        &self,
        points: Vec<GeographicCoordinate>,
    ) -> Result<Vec<ElevationPoint>, crate::SdkError>;

    /// Internally required by the uniffi foreign-handle machinery (see
    /// [`GeocodingProvider::uniffi_foreign_handle`]).
    #[doc(hidden)]
    fn uniffi_foreign_handle(&self) -> Option<::uniffi::Handle> {
        Option::None
    }
}

/// Bridges a foreign implementation of the [`RoutingProvider`] callback
/// contract onto the native `geospatial-traits` trait.
pub(crate) struct ForeignRoutingProvider {
    /// The foreign implementation serving the contract calls.
    routing_implementation: Arc<dyn RoutingProvider>,
    /// The owning `MapSdk`'s provider-failure diagnostics ring.
    provider_failure_diagnostics: SharedProviderFailureDiagnostics,
}

impl ForeignRoutingProvider {
    /// Wraps the foreign routing implementation for native registration.
    pub(crate) fn new(
        routing_implementation: Arc<dyn RoutingProvider>,
        provider_failure_diagnostics: SharedProviderFailureDiagnostics,
    ) -> Self {
        Self {
            routing_implementation,
            provider_failure_diagnostics,
        }
    }
}

#[async_trait::async_trait]
impl native_traits::RoutingProvider for ForeignRoutingProvider {
    /// A foreign routing implementation reports no capability set of its own,
    /// so the SDK assumes it honours every mode and both strategies. An
    /// application that cannot serve one of them keeps that knowledge in its
    /// own surface.
    fn route_capabilities(&self) -> geospatial_types::route::RouteCapabilities {
        geospatial_types::route::RouteCapabilities::all_supported()
    }

    async fn calculate_route(
        &self,
        waypoints: &[geospatial_types::GeographicCoordinate],
        route_options: &geospatial_types::route::RouteOptions,
    ) -> Result<geospatial_types::route::RouteData, SDKError> {
        let foreign_route = self
            .routing_implementation
            .calculate_route(
                waypoints
                    .iter()
                    .map(GeographicCoordinate::from_native_coordinate)
                    .collect(),
                RouteOptions::from_native_route_options(route_options),
            )
            .await
            .map_err(|foreign_failure| {
                foreign_failure_with_diagnostic(
                    &self.provider_failure_diagnostics,
                    ROUTING_CONTRACT_NAME,
                    CALCULATE_ROUTE_METHOD_NAME,
                    foreign_failure,
                )
            })?;
        foreign_route
            .into_native_route_data()
            .map_err(geospatial_types::SDKError::from)
    }
}

/// Bridges a foreign implementation of the [`DistanceMatrixProvider`] callback
/// contract onto the native `geospatial-traits` trait.
pub(crate) struct ForeignDistanceMatrixProvider {
    /// The foreign implementation serving the contract calls.
    distance_matrix_implementation: Arc<dyn DistanceMatrixProvider>,
    /// The owning `MapSdk`'s provider-failure diagnostics ring.
    provider_failure_diagnostics: SharedProviderFailureDiagnostics,
}

impl ForeignDistanceMatrixProvider {
    /// Wraps the foreign distance-matrix implementation for native
    /// registration.
    pub(crate) fn new(
        distance_matrix_implementation: Arc<dyn DistanceMatrixProvider>,
        provider_failure_diagnostics: SharedProviderFailureDiagnostics,
    ) -> Self {
        Self {
            distance_matrix_implementation,
            provider_failure_diagnostics,
        }
    }
}

#[async_trait::async_trait]
impl native_traits::DistanceMatrixProvider for ForeignDistanceMatrixProvider {
    async fn calculate_distance_matrix(
        &self,
        origins: &[geospatial_types::GeographicCoordinate],
        destinations: &[geospatial_types::GeographicCoordinate],
        transportation_mode: geospatial_types::TransportationMode,
    ) -> Result<geospatial_types::DistanceMatrix, SDKError> {
        let foreign_distance_matrix = self
            .distance_matrix_implementation
            .calculate_distance_matrix(
                origins
                    .iter()
                    .map(GeographicCoordinate::from_native_coordinate)
                    .collect(),
                destinations
                    .iter()
                    .map(GeographicCoordinate::from_native_coordinate)
                    .collect(),
                TransportationMode::from_native_transportation_mode(transportation_mode),
            )
            .await
            .map_err(|foreign_failure| {
                foreign_failure_with_diagnostic(
                    &self.provider_failure_diagnostics,
                    DISTANCE_MATRIX_CONTRACT_NAME,
                    CALCULATE_DISTANCE_MATRIX_METHOD_NAME,
                    foreign_failure,
                )
            })?;
        Ok(foreign_distance_matrix.into_native_distance_matrix())
    }
}

/// Bridges a foreign implementation of the [`SearchPlaceProvider`] callback
/// contract onto the native `geospatial-traits` trait.
pub(crate) struct ForeignSearchPlaceProvider {
    /// The foreign implementation serving the contract calls.
    search_place_implementation: Arc<dyn SearchPlaceProvider>,
    /// The owning `MapSdk`'s provider-failure diagnostics ring.
    provider_failure_diagnostics: SharedProviderFailureDiagnostics,
}

impl ForeignSearchPlaceProvider {
    /// Wraps the foreign place-search implementation for native registration.
    pub(crate) fn new(
        search_place_implementation: Arc<dyn SearchPlaceProvider>,
        provider_failure_diagnostics: SharedProviderFailureDiagnostics,
    ) -> Self {
        Self {
            search_place_implementation,
            provider_failure_diagnostics,
        }
    }
}

#[async_trait::async_trait]
impl native_traits::SearchPlaceProvider for ForeignSearchPlaceProvider {
    async fn search_autocomplete(
        &self,
        query_text: &str,
        search_boundary: Option<geospatial_types::BoundingBox>,
    ) -> Result<Vec<geospatial_types::PlaceResult>, SDKError> {
        let foreign_places = self
            .search_place_implementation
            .search_autocomplete(
                query_text.to_string(),
                search_boundary.map(|native_bounding_box| {
                    BoundingBox::from_native_bounding_box(&native_bounding_box)
                }),
            )
            .await
            .map_err(|foreign_failure| {
                foreign_failure_with_diagnostic(
                    &self.provider_failure_diagnostics,
                    SEARCH_PLACE_CONTRACT_NAME,
                    SEARCH_AUTOCOMPLETE_METHOD_NAME,
                    foreign_failure,
                )
            })?;
        Ok(foreign_places
            .into_iter()
            .map(Place::into_native_place_result)
            .collect())
    }

    async fn search_places(
        &self,
        query_text: &str,
        search_boundary: Option<geospatial_types::BoundingBox>,
    ) -> Result<Vec<geospatial_types::PlaceResult>, SDKError> {
        let foreign_places = self
            .search_place_implementation
            .search_places(
                query_text.to_string(),
                search_boundary.map(|native_bounding_box| {
                    BoundingBox::from_native_bounding_box(&native_bounding_box)
                }),
            )
            .await
            .map_err(|foreign_failure| {
                foreign_failure_with_diagnostic(
                    &self.provider_failure_diagnostics,
                    SEARCH_PLACE_CONTRACT_NAME,
                    SEARCH_PLACES_METHOD_NAME,
                    foreign_failure,
                )
            })?;
        Ok(foreign_places
            .into_iter()
            .map(Place::into_native_place_result)
            .collect())
    }
}

/// Bridges a foreign implementation of the [`TrafficIncidentProvider`]
/// callback contract onto the native `geospatial-traits` trait.
pub(crate) struct ForeignTrafficIncidentProvider {
    /// The foreign implementation serving the contract calls.
    traffic_incident_implementation: Arc<dyn TrafficIncidentProvider>,
    /// The owning `MapSdk`'s provider-failure diagnostics ring.
    provider_failure_diagnostics: SharedProviderFailureDiagnostics,
}

impl ForeignTrafficIncidentProvider {
    /// Wraps the foreign traffic-incident implementation for native
    /// registration.
    pub(crate) fn new(
        traffic_incident_implementation: Arc<dyn TrafficIncidentProvider>,
        provider_failure_diagnostics: SharedProviderFailureDiagnostics,
    ) -> Self {
        Self {
            traffic_incident_implementation,
            provider_failure_diagnostics,
        }
    }
}

#[async_trait::async_trait]
impl native_traits::TrafficIncidentProvider for ForeignTrafficIncidentProvider {
    async fn fetch_traffic_incidents(
        &self,
        bounding_box: &geospatial_types::BoundingBox,
    ) -> Result<Vec<geospatial_types::TrafficIncident>, SDKError> {
        let foreign_incidents = self
            .traffic_incident_implementation
            .fetch_traffic_incidents(BoundingBox::from_native_bounding_box(bounding_box))
            .await
            .map_err(|foreign_failure| {
                foreign_failure_with_diagnostic(
                    &self.provider_failure_diagnostics,
                    TRAFFIC_INCIDENT_CONTRACT_NAME,
                    FETCH_TRAFFIC_INCIDENTS_METHOD_NAME,
                    foreign_failure,
                )
            })?;
        foreign_incidents
            .into_iter()
            .map(TrafficIncident::into_native_traffic_incident)
            .collect::<Result<Vec<_>, crate::SdkError>>()
            .map_err(geospatial_types::SDKError::from)
    }
}

/// Bridges a foreign implementation of the [`TrafficFlowProvider`] callback
/// contract onto the native `geospatial-traits` trait.
pub(crate) struct ForeignTrafficFlowProvider {
    /// The foreign implementation serving the contract calls.
    traffic_flow_implementation: Arc<dyn TrafficFlowProvider>,
    /// The owning `MapSdk`'s provider-failure diagnostics ring.
    provider_failure_diagnostics: SharedProviderFailureDiagnostics,
}

impl ForeignTrafficFlowProvider {
    /// Wraps the foreign traffic-flow implementation for native registration.
    pub(crate) fn new(
        traffic_flow_implementation: Arc<dyn TrafficFlowProvider>,
        provider_failure_diagnostics: SharedProviderFailureDiagnostics,
    ) -> Self {
        Self {
            traffic_flow_implementation,
            provider_failure_diagnostics,
        }
    }
}

#[async_trait::async_trait]
impl native_traits::TrafficFlowProvider for ForeignTrafficFlowProvider {
    async fn fetch_traffic_flow(
        &self,
        bounding_box: &geospatial_types::BoundingBox,
    ) -> Result<Vec<geospatial_types::TrafficFlowData>, SDKError> {
        let foreign_flow_observations = self
            .traffic_flow_implementation
            .fetch_traffic_flow(BoundingBox::from_native_bounding_box(bounding_box))
            .await
            .map_err(|foreign_failure| {
                foreign_failure_with_diagnostic(
                    &self.provider_failure_diagnostics,
                    TRAFFIC_FLOW_CONTRACT_NAME,
                    FETCH_TRAFFIC_FLOW_METHOD_NAME,
                    foreign_failure,
                )
            })?;
        foreign_flow_observations
            .into_iter()
            .map(TrafficFlowData::into_native_traffic_flow_data)
            .collect::<Result<Vec<_>, crate::SdkError>>()
            .map_err(geospatial_types::SDKError::from)
    }
}

/// Bridges a foreign implementation of the [`ElevationProvider`] callback
/// contract onto the native `geospatial-traits` trait.
pub(crate) struct ForeignElevationProvider {
    /// The foreign implementation serving the contract calls.
    elevation_implementation: Arc<dyn ElevationProvider>,
    /// The owning `MapSdk`'s provider-failure diagnostics ring.
    provider_failure_diagnostics: SharedProviderFailureDiagnostics,
}

impl ForeignElevationProvider {
    /// Wraps the foreign elevation implementation for native registration.
    pub(crate) fn new(
        elevation_implementation: Arc<dyn ElevationProvider>,
        provider_failure_diagnostics: SharedProviderFailureDiagnostics,
    ) -> Self {
        Self {
            elevation_implementation,
            provider_failure_diagnostics,
        }
    }
}

#[async_trait::async_trait]
impl native_traits::ElevationProvider for ForeignElevationProvider {
    async fn fetch_elevation_points(
        &self,
        points: &[geospatial_types::GeographicCoordinate],
    ) -> Result<Vec<geospatial_types::ElevationPoint>, SDKError> {
        let foreign_elevation_points = self
            .elevation_implementation
            .fetch_elevation_points(
                points
                    .iter()
                    .map(GeographicCoordinate::from_native_coordinate)
                    .collect(),
            )
            .await
            .map_err(|foreign_failure| {
                foreign_failure_with_diagnostic(
                    &self.provider_failure_diagnostics,
                    ELEVATION_CONTRACT_NAME,
                    FETCH_ELEVATION_POINTS_METHOD_NAME,
                    foreign_failure,
                )
            })?;
        foreign_elevation_points
            .into_iter()
            .map(ElevationPoint::into_native_elevation_point)
            .collect::<Result<Vec<_>, crate::SdkError>>()
            .map_err(geospatial_types::SDKError::from)
    }
}
