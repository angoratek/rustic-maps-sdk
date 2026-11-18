//! Adapters presenting an everymap-rs provider through this SDK's contracts.
//!
//! everymap-rs owns the provider layer: the transport seam, pacing, retry,
//! observation and the vendor integrations. The SDK's rendering, cache and
//! application surfaces speak the contracts in `geospatial-traits`, so a
//! provider instance is wrapped in one of the adapters here before it is
//! registered. The adapters convert records and errors only; they hold no
//! vendor knowledge and name no provider.

use async_trait::async_trait;
use everymap_core::domains::autocomplete as everymap_autocomplete;
use everymap_core::domains::matrix as everymap_matrix;
use everymap_core::domains::routing as everymap_routing;
use everymap_core::domains::search as everymap_search;
use everymap_core::domains::tiling as everymap_tiling;
use everymap_core::domains::traffic as everymap_traffic;
use everymap_core::error::EveryMapError;
use geospatial_traits::{
    DistanceMatrixProvider, GeocodingProvider, RasterTileProvider, RoutingProvider,
    SearchPlaceProvider, TrafficFlowProvider, TrafficIncidentProvider, VectorTileProvider,
    VectorTileSourceDescriptor,
};
use geospatial_types::place::GeocodeQuality;
use geospatial_types::route::{
    DistanceMatrix, DistanceMatrixEntry, LaneGuidance, ManeuverType, RouteCapabilities, RouteData,
    RouteLeg, RouteOptions, RouteStep, RouteStrategy, TransportationMode, TurnDirection,
    UnitSystem,
};
use geospatial_types::traffic::{
    TrafficFlowCongestionLevel, TrafficIncidentSeverity, TrafficIncidentType,
};
use geospatial_types::{
    BoundingBox, GeographicCoordinate, PlaceResult, RasterImageFormat, RasterTileData,
    RasterTileResolution, SDKError, TrafficFlowData, TrafficIncident, VectorTileData,
    VectorTileEncoding,
};
use std::sync::Arc;

/// Translates an everymap-rs failure into the SDK's unified error surface.
///
/// The mapping keeps the categories an application reacts to: a refused rate
/// limit stays a rate limit, a transport failure stays retryable, a validation
/// failure names the invalid input, and everything the provider itself
/// rejected stays a provider rejection carrying the provider's detail.
pub fn map_provider_error(error: EveryMapError) -> SDKError {
    match &error {
        EveryMapError::HttpError { status, .. } if *status == 429 => SDKError::RateLimitExhausted,
        EveryMapError::RateLimited { .. } => SDKError::RateLimitExhausted,
        EveryMapError::ValidationError(detail) => SDKError::InvalidCoordinateInput(detail.clone()),
        EveryMapError::SerializationError { .. } => {
            SDKError::ResponseParsingFailure(error.to_string())
        }
        EveryMapError::TransportError(detail) => SDKError::NetworkRequestFailed(detail.clone()),
        #[cfg(not(target_arch = "wasm32"))]
        EveryMapError::ClientError(_) => SDKError::NetworkRequestFailed(error.to_string()),
        EveryMapError::Unknown => {
            SDKError::ResponseParsingFailure("the provider reported an unknown failure".to_string())
        }
        EveryMapError::HttpError { .. }
        | EveryMapError::AuthError { .. }
        | EveryMapError::ProviderError { .. }
        | EveryMapError::UnsupportedDomain { .. } => {
            SDKError::ProviderRequestRejected(error.to_string())
        }
    }
}

/// The longest a tile request may wait for its token before the SDK refuses
/// it.
///
/// A tile request carries no value after ten seconds — the viewport has moved
/// on — so a source whose declared budget would keep it waiting past that is
/// answered with [`SDKError::RateLimitExhausted`] instead of stalling.
pub const TILE_SOURCE_MAXIMUM_WAIT_SECONDS: f64 = 10.0;

/// Converts a declared request budget into the whole-token capacity and
/// refill rate the provider layer's token bucket admits.
///
/// The bucket admits a whole number of requests, so a fractional capacity is
/// refused rather than rounded: rounding would silently change the budget the
/// application declared.
///
/// # Errors
/// Returns [`SDKError::InvalidCoordinateInput`] when the capacity is not a
/// whole number of at least one, or the refill rate is not a finite positive
/// rate.
/// Converts a declared request budget, naming the source that declared it.
///
/// Every registration path reports a refused budget the same way, so a
/// settings surface can point at the offending source rather than at the
/// contract.
///
/// # Errors
/// Returns [`SDKError::InvalidCoordinateInput`] with the same detail
/// [`token_bucket_rates`] produces, prefixed by the declaring source.
pub fn tile_source_budget_rates(
    source_name: &str,
    capacity_tokens: f64,
    refill_tokens_per_second: f64,
) -> Result<(u32, f64), SDKError> {
    token_bucket_rates(capacity_tokens, refill_tokens_per_second).map_err(|error| match error {
        SDKError::InvalidCoordinateInput(detail) => SDKError::InvalidCoordinateInput(format!(
            "the {source_name} tile source declares an unusable request budget: {detail}"
        )),
        other => other,
    })
}

pub fn token_bucket_rates(
    capacity_tokens: f64,
    refill_tokens_per_second: f64,
) -> Result<(u32, f64), SDKError> {
    if !capacity_tokens.is_finite() || capacity_tokens < 1.0 || capacity_tokens.fract() != 0.0 {
        return Err(SDKError::InvalidCoordinateInput(format!(
            "the token-bucket capacity of {capacity_tokens} must be a whole number of at least 1"
        )));
    }
    if !refill_tokens_per_second.is_finite() || refill_tokens_per_second <= 0.0 {
        return Err(SDKError::InvalidCoordinateInput(format!(
            "the refill rate of {refill_tokens_per_second} tokens per second must be finite and \
             positive"
        )));
    }
    Ok((capacity_tokens as u32, refill_tokens_per_second))
}

/// Presents an everymap-rs raster-tile provider through [`RasterTileProvider`].
pub struct EveryMapRasterTileProvider {
    provider: Arc<dyn everymap_tiling::RasterTileProvider>,
}

impl EveryMapRasterTileProvider {
    /// Wraps the everymap-rs provider that serves the tiles.
    pub fn new(provider: Arc<dyn everymap_tiling::RasterTileProvider>) -> Self {
        Self { provider }
    }
}

#[async_trait]
impl RasterTileProvider for EveryMapRasterTileProvider {
    async fn fetch_raster_tile(
        &self,
        tile_x: u32,
        tile_y: u32,
        zoom_level: u8,
        image_format: RasterImageFormat,
        tile_resolution: RasterTileResolution,
    ) -> Result<RasterTileData, SDKError> {
        let request = everymap_tiling::RasterTileRequest::new(
            tile_x,
            tile_y,
            zoom_level,
            everymap_image_format(image_format),
            everymap_tile_resolution(tile_resolution),
        );
        let tile = self
            .provider
            .fetch_raster_tile(&request)
            .await
            .map_err(map_provider_error)?;
        Ok(RasterTileData {
            tile_x: tile.tile_x,
            tile_y: tile.tile_y,
            zoom_level: tile.zoom_level,
            image_format: sdk_image_format(tile.image_format),
            tile_resolution: sdk_tile_resolution(tile.tile_resolution),
            image_bytes: tile.image_bytes,
        })
    }
}

/// Presents an everymap-rs vector-tile provider through [`VectorTileProvider`].
pub struct EveryMapVectorTileProvider {
    provider: Arc<dyn everymap_tiling::VectorTileProvider>,
}

impl EveryMapVectorTileProvider {
    /// Wraps the everymap-rs provider that serves the tileset.
    pub fn new(provider: Arc<dyn everymap_tiling::VectorTileProvider>) -> Self {
        Self { provider }
    }
}

#[async_trait]
impl VectorTileProvider for EveryMapVectorTileProvider {
    fn source_descriptor(&self) -> VectorTileSourceDescriptor {
        let descriptor = self.provider.source_descriptor();
        VectorTileSourceDescriptor {
            source_name: descriptor.source_name,
            attribution_text: descriptor.attribution_text,
            minimum_zoom_level: descriptor.minimum_zoom_level,
            maximum_zoom_level: descriptor.maximum_zoom_level,
            style_hints: descriptor.style_hints,
        }
    }

    async fn fetch_vector_tile(
        &self,
        tile_x: u32,
        tile_y: u32,
        zoom_level: u8,
    ) -> Result<VectorTileData, SDKError> {
        let tile = self
            .provider
            .fetch_vector_tile(tile_x, tile_y, zoom_level)
            .await
            .map_err(map_provider_error)?;
        Ok(VectorTileData {
            tile_x: tile.tile_x,
            tile_y: tile.tile_y,
            zoom_level: tile.zoom_level,
            tile_encoding: sdk_tile_encoding(tile.tile_encoding),
            tile_bytes: tile.tile_bytes,
        })
    }
}

fn everymap_image_format(image_format: RasterImageFormat) -> everymap_tiling::RasterImageFormat {
    match image_format {
        RasterImageFormat::Png => everymap_tiling::RasterImageFormat::Png,
        RasterImageFormat::Jpeg => everymap_tiling::RasterImageFormat::Jpeg,
    }
}

fn sdk_image_format(image_format: everymap_tiling::RasterImageFormat) -> RasterImageFormat {
    match image_format {
        everymap_tiling::RasterImageFormat::Png => RasterImageFormat::Png,
        everymap_tiling::RasterImageFormat::Jpeg => RasterImageFormat::Jpeg,
    }
}

fn everymap_tile_resolution(
    tile_resolution: RasterTileResolution,
) -> everymap_tiling::RasterTileResolution {
    match tile_resolution {
        RasterTileResolution::Standard => everymap_tiling::RasterTileResolution::Standard,
        RasterTileResolution::Retina => everymap_tiling::RasterTileResolution::Retina,
    }
}

fn sdk_tile_resolution(
    tile_resolution: everymap_tiling::RasterTileResolution,
) -> RasterTileResolution {
    match tile_resolution {
        everymap_tiling::RasterTileResolution::Standard => RasterTileResolution::Standard,
        everymap_tiling::RasterTileResolution::Retina => RasterTileResolution::Retina,
    }
}

fn sdk_tile_encoding(tile_encoding: everymap_tiling::VectorTileEncoding) -> VectorTileEncoding {
    match tile_encoding {
        everymap_tiling::VectorTileEncoding::MapboxVectorTile => {
            VectorTileEncoding::MapboxVectorTile
        }
        everymap_tiling::VectorTileEncoding::GzippedMapboxVectorTile => {
            VectorTileEncoding::GzippedMapboxVectorTile
        }
    }
}

/// Presents an everymap-rs router through [`RoutingProvider`].
pub struct EveryMapRouter {
    provider: Arc<dyn everymap_routing::Router>,
}

impl EveryMapRouter {
    /// Wraps the everymap-rs router that calculates the route.
    pub fn new(provider: Arc<dyn everymap_routing::Router>) -> Self {
        Self { provider }
    }
}

#[async_trait]
impl RoutingProvider for EveryMapRouter {
    async fn calculate_route(
        &self,
        waypoints: &[GeographicCoordinate],
        route_options: &RouteOptions,
    ) -> Result<RouteData, SDKError> {
        let upstream_waypoints = waypoints
            .iter()
            .map(everymap_coordinate)
            .collect::<Result<Vec<_>, _>>()?;
        let upstream_options = everymap_route_options(route_options);
        let response = self
            .provider
            .calculate_route_with_waypoints(&upstream_waypoints, &upstream_options)
            .await
            .map_err(map_provider_error)?;
        let route = response.routes.into_iter().next().ok_or_else(|| {
            SDKError::ProviderRequestRejected(
                "the provider returned no route for the request".to_string(),
            )
        })?;
        Ok(sdk_route_data(route))
    }

    fn route_capabilities(&self) -> RouteCapabilities {
        let capabilities = self.provider.route_capabilities();
        RouteCapabilities {
            supported_transportation_modes: capabilities
                .supported_transport_modes
                .iter()
                .filter_map(|transport_mode| sdk_transportation_mode(*transport_mode))
                .collect(),
            supports_shortest_route_strategy: capabilities.supports_shortest_route_strategy,
        }
    }
}

fn everymap_coordinate(
    coordinate: &GeographicCoordinate,
) -> Result<everymap_core::Coordinate, SDKError> {
    everymap_core::Coordinate::new(coordinate.latitude, coordinate.longitude)
        .map_err(|error| SDKError::InvalidCoordinateInput(error.to_string()))
}

fn sdk_coordinate(coordinate: &everymap_core::Coordinate) -> GeographicCoordinate {
    GeographicCoordinate {
        latitude: coordinate.lat,
        longitude: coordinate.lng,
    }
}

fn everymap_route_options(route_options: &RouteOptions) -> everymap_routing::RouteOptions {
    let mut avoid = Vec::new();
    if route_options.avoid_options.toll_roads {
        avoid.push(everymap_routing::AvoidType::Tolls);
    }
    if route_options.avoid_options.limited_access_highways {
        avoid.push(everymap_routing::AvoidType::Highways);
    }
    if route_options.avoid_options.ferries {
        avoid.push(everymap_routing::AvoidType::Ferries);
    }
    if route_options.avoid_options.unpaved_roads {
        avoid.push(everymap_routing::AvoidType::DirtRoads);
    }
    everymap_routing::RouteOptions {
        transport_mode: Some(everymap_transport_mode(route_options.transportation_mode)),
        avoid,
        departure_time: route_options
            .departure_timestamp_seconds
            .map(|seconds| everymap_routing::DepartureTime::Timestamp(seconds as i64)),
        route_strategy: Some(match route_options.route_strategy {
            RouteStrategy::Fastest => everymap_routing::RouteStrategy::Fastest,
            RouteStrategy::Shortest => everymap_routing::RouteStrategy::Shortest,
        }),
        unit_system: Some(match route_options.unit_system {
            UnitSystem::Metric => everymap_routing::UnitSystem::Metric,
            UnitSystem::Imperial => everymap_routing::UnitSystem::Imperial,
        }),
        ..everymap_routing::RouteOptions::default()
    }
}

fn everymap_transport_mode(
    transportation_mode: TransportationMode,
) -> everymap_routing::TransportMode {
    match transportation_mode {
        TransportationMode::Automobile => everymap_routing::TransportMode::Car,
        TransportationMode::Bicycle => everymap_routing::TransportMode::Bicycle,
        TransportationMode::Pedestrian => everymap_routing::TransportMode::Pedestrian,
    }
}

fn sdk_transportation_mode(
    transport_mode: everymap_routing::TransportMode,
) -> Option<TransportationMode> {
    match transport_mode {
        everymap_routing::TransportMode::Car | everymap_routing::TransportMode::Truck => {
            Some(TransportationMode::Automobile)
        }
        everymap_routing::TransportMode::Bicycle => Some(TransportationMode::Bicycle),
        everymap_routing::TransportMode::Pedestrian => Some(TransportationMode::Pedestrian),
        _ => None,
    }
}

fn sdk_route_data(route: everymap_routing::RouteResult) -> RouteData {
    let route_bounding_box = route.bounding_box.as_ref().map(|bounding_box| BoundingBox {
        south_west: sdk_coordinate(&bounding_box.south_west),
        north_east: sdk_coordinate(&bounding_box.north_east),
    });
    let geometry: Vec<GeographicCoordinate> =
        route.geometry.points.iter().map(sdk_coordinate).collect();
    let legs: Vec<RouteLeg> = route
        .legs
        .into_iter()
        .map(|leg| RouteLeg {
            distance_meters: leg.distance,
            duration_seconds: leg.duration,
            steps: leg.steps.into_iter().map(sdk_route_step).collect(),
        })
        .collect();
    let bounding_box = route_bounding_box
        .or_else(|| bounding_box_of_points(&geometry))
        .or_else(|| bounding_box_of(&legs))
        .unwrap_or(BoundingBox {
            south_west: GeographicCoordinate {
                latitude: 0.0,
                longitude: 0.0,
            },
            north_east: GeographicCoordinate {
                latitude: 0.0,
                longitude: 0.0,
            },
        });
    RouteData {
        distance_meters: route.distance,
        duration_seconds: route.duration,
        bounding_box,
        geometry,
        legs,
    }
}

/// Derives the route's bounds from its full geometry.
fn bounding_box_of_points(points: &[GeographicCoordinate]) -> Option<BoundingBox> {
    BoundingBox::from_coordinates(points.iter().copied())
}

/// Derives the route's bounds from its step geometry when the provider does
/// not report them.
fn bounding_box_of(legs: &[RouteLeg]) -> Option<BoundingBox> {
    BoundingBox::from_coordinates(
        legs.iter()
            .flat_map(|leg| leg.steps.iter())
            .flat_map(|step| step.shape.iter())
            .copied(),
    )
}

fn sdk_route_step(step: everymap_routing::RouteStep) -> RouteStep {
    RouteStep {
        distance_meters: step.distance.unwrap_or_default(),
        duration_seconds: step.duration.unwrap_or_default(),
        maneuver_type: step
            .maneuver_type
            .as_ref()
            .map(sdk_maneuver_type)
            .unwrap_or(ManeuverType::Continue),
        instruction_display_text: step.instruction.unwrap_or_default(),
        instruction_speech_text: step.speech_instruction.unwrap_or_default(),
        shape: step.shape.iter().map(sdk_coordinate).collect(),
        lane_guidances: step
            .lane_guidances
            .into_iter()
            .map(|lane| LaneGuidance {
                lane_index: lane.lane_index,
                permitted_directions: lane
                    .permitted_directions
                    .iter()
                    .map(|direction| sdk_turn_direction(*direction))
                    .collect(),
                is_recommended: lane.is_recommended,
            })
            .collect(),
    }
}

fn sdk_maneuver_type(maneuver_type: &everymap_routing::ManeuverType) -> ManeuverType {
    match maneuver_type {
        everymap_routing::ManeuverType::Departure => ManeuverType::Departure,
        everymap_routing::ManeuverType::Arrival => ManeuverType::Arrival,
        everymap_routing::ManeuverType::Continue => ManeuverType::Continue,
        everymap_routing::ManeuverType::Turn(direction) => {
            ManeuverType::Turn(sdk_turn_direction(*direction))
        }
        everymap_routing::ManeuverType::Merge => ManeuverType::Merge,
        everymap_routing::ManeuverType::OnRamp => ManeuverType::OnRamp,
        everymap_routing::ManeuverType::OffRamp => ManeuverType::OffRamp,
        everymap_routing::ManeuverType::Fork => ManeuverType::Fork,
        everymap_routing::ManeuverType::Roundabout(exit_index) => {
            ManeuverType::Roundabout(*exit_index)
        }
        everymap_routing::ManeuverType::Ferry => ManeuverType::Ferry,
    }
}

fn sdk_turn_direction(direction: everymap_routing::TurnDirection) -> TurnDirection {
    match direction {
        everymap_routing::TurnDirection::Straight => TurnDirection::Straight,
        everymap_routing::TurnDirection::SharpLeft => TurnDirection::SharpLeft,
        everymap_routing::TurnDirection::Left => TurnDirection::Left,
        everymap_routing::TurnDirection::SlightLeft => TurnDirection::SlightLeft,
        everymap_routing::TurnDirection::SlightRight => TurnDirection::SlightRight,
        everymap_routing::TurnDirection::Right => TurnDirection::Right,
        everymap_routing::TurnDirection::SharpRight => TurnDirection::SharpRight,
        everymap_routing::TurnDirection::UTurn => TurnDirection::UTurn,
        everymap_routing::TurnDirection::LeavingRoad => TurnDirection::LeavingRoad,
    }
}

/// Presents an everymap-rs geocoder through [`GeocodingProvider`].
///
/// The SDK's contract has no options argument, so every request uses the
/// provider layer's documented defaults.
pub struct EveryMapGeocoder {
    provider: Arc<dyn everymap_search::Geocoder>,
}

impl EveryMapGeocoder {
    /// Wraps the everymap-rs geocoder that answers the requests.
    pub fn new(provider: Arc<dyn everymap_search::Geocoder>) -> Self {
        Self { provider }
    }
}

#[async_trait]
impl GeocodingProvider for EveryMapGeocoder {
    async fn forward_geocode(&self, address_text: &str) -> Result<Vec<PlaceResult>, SDKError> {
        let response = self
            .provider
            .geocode(address_text, &everymap_search::GeocodeOptions::default())
            .await
            .map_err(map_provider_error)?;
        Ok(response.items.into_iter().map(sdk_place_result).collect())
    }

    async fn reverse_geocode(
        &self,
        coordinate: &GeographicCoordinate,
    ) -> Result<Vec<PlaceResult>, SDKError> {
        let upstream_coordinate = everymap_coordinate(coordinate)?;
        let response = self
            .provider
            .reverse_geocode(
                &upstream_coordinate,
                &everymap_search::ReverseGeocodeOptions::default(),
            )
            .await
            .map_err(map_provider_error)?;
        Ok(response.items.into_iter().map(sdk_place_result).collect())
    }
}

/// Presents an everymap-rs settled search and its completion endpoint through
/// [`SearchPlaceProvider`].
///
/// The SDK's contract pairs the two searches on one provider, while the
/// provider layer keeps them as separate traits because they usually have
/// separate rate limits and billing. Both are constructed from the same
/// provider client, so one adapter carries one credential.
pub struct EveryMapPlaceSearchProvider {
    settled_search: Arc<dyn everymap_search::PlaceSearchProvider>,
    autocomplete: Arc<dyn everymap_autocomplete::AutocompleteProvider>,
}

impl EveryMapPlaceSearchProvider {
    /// Wraps the everymap-rs settled-search and completion providers.
    pub fn new(
        settled_search: Arc<dyn everymap_search::PlaceSearchProvider>,
        autocomplete: Arc<dyn everymap_autocomplete::AutocompleteProvider>,
    ) -> Self {
        Self {
            settled_search,
            autocomplete,
        }
    }
}

#[async_trait]
impl SearchPlaceProvider for EveryMapPlaceSearchProvider {
    async fn search_autocomplete(
        &self,
        query_text: &str,
        search_boundary: Option<BoundingBox>,
    ) -> Result<Vec<PlaceResult>, SDKError> {
        let options = everymap_autocomplete::AutocompleteOptions {
            bounding_box: search_boundary.map(|bounding_box| everymap_bounding_box(&bounding_box)),
            ..everymap_autocomplete::AutocompleteOptions::default()
        };
        let response = self
            .autocomplete
            .autocomplete(query_text, &options)
            .await
            .map_err(map_provider_error)?;
        // A completion candidate may describe an area with no single
        // position; the SDK's place record requires one, so a candidate
        // without it is left to the settled search that follows.
        Ok(response
            .items
            .into_iter()
            .filter_map(sdk_place_result_from_autocomplete)
            .collect())
    }

    async fn search_places(
        &self,
        query_text: &str,
        search_boundary: Option<BoundingBox>,
    ) -> Result<Vec<PlaceResult>, SDKError> {
        let options = everymap_search::PlaceSearchOptions {
            bounding_box: search_boundary.map(|bounding_box| everymap_bounding_box(&bounding_box)),
            ..everymap_search::PlaceSearchOptions::default()
        };
        let response = self
            .settled_search
            .search_places(query_text, &options)
            .await
            .map_err(map_provider_error)?;
        Ok(response.items.into_iter().map(sdk_place_result).collect())
    }
}

/// Presents an everymap-rs traffic provider through
/// [`TrafficIncidentProvider`] and [`TrafficFlowProvider`].
///
/// The provider layer answers both traffic shapes from one area request, so
/// one wrapped provider serves the SDK's two incident and flow slots.
pub struct EveryMapTrafficProvider {
    provider: Arc<dyn everymap_traffic::TrafficProvider>,
}

impl EveryMapTrafficProvider {
    /// Wraps the everymap-rs traffic provider that answers the requests.
    pub fn new(provider: Arc<dyn everymap_traffic::TrafficProvider>) -> Self {
        Self { provider }
    }

    /// Asks the provider for one bounding box, requesting the incidents and
    /// the flow the caller wants.
    async fn traffic_in_area(
        &self,
        bounding_box: &BoundingBox,
        include_incidents: bool,
    ) -> Result<everymap_traffic::TrafficResponse, SDKError> {
        let center = everymap_coordinate(&bounding_box_center(bounding_box))?;
        let options = everymap_traffic::TrafficOptions {
            bounding_box: Some(everymap_bounding_box(bounding_box)),
            include_incidents: Some(include_incidents),
            ..everymap_traffic::TrafficOptions::default()
        };
        self.provider
            .get_traffic(&center, &options)
            .await
            .map_err(map_provider_error)
    }
}

#[async_trait]
impl TrafficIncidentProvider for EveryMapTrafficProvider {
    async fn fetch_traffic_incidents(
        &self,
        bounding_box: &BoundingBox,
    ) -> Result<Vec<TrafficIncident>, SDKError> {
        let response = self.traffic_in_area(bounding_box, true).await?;
        Ok(response
            .incidents
            .into_iter()
            .filter_map(sdk_traffic_incident)
            .collect())
    }
}

#[async_trait]
impl TrafficFlowProvider for EveryMapTrafficProvider {
    async fn fetch_traffic_flow(
        &self,
        bounding_box: &BoundingBox,
    ) -> Result<Vec<TrafficFlowData>, SDKError> {
        let response = self.traffic_in_area(bounding_box, false).await?;
        Ok(response.flows.into_iter().map(sdk_traffic_flow).collect())
    }
}

/// Presents an everymap-rs distance-matrix provider through
/// [`DistanceMatrixProvider`].
pub struct EveryMapDistanceMatrixProvider {
    provider: Arc<dyn everymap_matrix::DistanceMatrixProvider>,
}

impl EveryMapDistanceMatrixProvider {
    /// Wraps the everymap-rs matrix provider that answers the requests.
    pub fn new(provider: Arc<dyn everymap_matrix::DistanceMatrixProvider>) -> Self {
        Self { provider }
    }
}

#[async_trait]
impl DistanceMatrixProvider for EveryMapDistanceMatrixProvider {
    async fn calculate_distance_matrix(
        &self,
        origins: &[GeographicCoordinate],
        destinations: &[GeographicCoordinate],
        transportation_mode: TransportationMode,
    ) -> Result<DistanceMatrix, SDKError> {
        let upstream_origins = origins
            .iter()
            .map(everymap_coordinate)
            .collect::<Result<Vec<_>, _>>()?;
        let upstream_destinations = destinations
            .iter()
            .map(everymap_coordinate)
            .collect::<Result<Vec<_>, _>>()?;
        let options = everymap_matrix::DistanceMatrixOptions {
            transport_mode: Some(everymap_transport_mode(transportation_mode)),
            ..everymap_matrix::DistanceMatrixOptions::default()
        };
        let response = self
            .provider
            .distance_matrix(&upstream_origins, &upstream_destinations, &options)
            .await
            .map_err(map_provider_error)?;
        // A pair the service could not measure arrives without its distance
        // or duration; the SDK's cell carries both, so the pair is omitted
        // rather than reported as zero.
        Ok(DistanceMatrix {
            entries: response
                .entries
                .into_iter()
                .filter_map(sdk_distance_matrix_entry)
                .collect(),
        })
    }
}

/// Projects a search result onto the SDK's place record.
fn sdk_place_result(result: everymap_search::SearchResult) -> PlaceResult {
    PlaceResult {
        structured_address_text: result.address.label.or(result.title).unwrap_or_default(),
        coordinate: sdk_coordinate(&result.coordinate),
        geocode_quality: result
            .quality
            .map(sdk_geocode_quality)
            .unwrap_or(GeocodeQuality::Point),
    }
}

/// Projects a completion candidate onto the SDK's place record, or `None`
/// when the candidate has no single position.
fn sdk_place_result_from_autocomplete(
    result: everymap_autocomplete::AutocompleteResult,
) -> Option<PlaceResult> {
    let coordinate = result.coordinate?;
    // A candidate's title is the display line its provider published; a
    // subtitle is supplementary context and never joins it, so the text a
    // suggestion puts in the field is the text the settled search answers for.
    let structured_address_text = result.title;
    Some(PlaceResult {
        structured_address_text,
        coordinate: sdk_coordinate(&coordinate),
        // When the provider classifies the candidate, its scale is shown;
        // otherwise the most precise class is assumed until the settled
        // search reports the real one.
        geocode_quality: result
            .quality
            .map(sdk_geocode_quality)
            .unwrap_or(GeocodeQuality::Point),
    })
}

/// Maps the provider layer's match granularity onto the SDK's.
fn sdk_geocode_quality(quality: everymap_search::SearchQuality) -> GeocodeQuality {
    match quality {
        everymap_search::SearchQuality::Point => GeocodeQuality::Point,
        everymap_search::SearchQuality::StreetAddress => GeocodeQuality::StreetAddress,
        everymap_search::SearchQuality::Intersection => GeocodeQuality::Intersection,
        everymap_search::SearchQuality::Street => GeocodeQuality::Street,
        everymap_search::SearchQuality::PostalCode => GeocodeQuality::PostalCode,
        everymap_search::SearchQuality::City => GeocodeQuality::City,
        everymap_search::SearchQuality::County => GeocodeQuality::County,
        everymap_search::SearchQuality::State => GeocodeQuality::State,
        everymap_search::SearchQuality::Country => GeocodeQuality::Country,
    }
}

/// Projects an incident onto the SDK's record, or `None` when the SDK record
/// cannot carry it.
///
/// An incident without a recognised class, or without its start position, has
/// no honest projection: the SDK record has no unknown class and no absent
/// position, and guessing either would put a wrong marker on a map.
fn sdk_traffic_incident(incident: everymap_traffic::TrafficIncident) -> Option<TrafficIncident> {
    let incident_type = sdk_traffic_incident_type(incident.incident_type.as_deref())?;
    let start_coordinate = incident.start_coordinate.as_ref().map(sdk_coordinate)?;
    Some(TrafficIncident {
        identifier: incident.id.unwrap_or_default(),
        incident_type,
        severity: sdk_traffic_incident_severity(incident.severity),
        short_description_text: incident.description.unwrap_or_default(),
        start_coordinate,
        end_coordinate: incident.end_coordinate.as_ref().map(sdk_coordinate),
    })
}

/// Maps the provider layer's incident vocabulary onto the SDK's classes.
fn sdk_traffic_incident_type(incident_type: Option<&str>) -> Option<TrafficIncidentType> {
    match incident_type? {
        "construction" => Some(TrafficIncidentType::Construction),
        "event" => Some(TrafficIncidentType::Event),
        "accident" => Some(TrafficIncidentType::Accident),
        "weather" => Some(TrafficIncidentType::Weather),
        "closure" | "roadClosure" => Some(TrafficIncidentType::Closure),
        _ => None,
    }
}

/// Maps the provider layer's five incident levels onto the SDK's three.
///
/// The SDK record always carries a severity; an unreported or unknown level
/// is treated as the lowest, which keeps an unclassified disruption visible
/// without overstating it.
fn sdk_traffic_incident_severity(
    severity: Option<everymap_traffic::IncidentSeverity>,
) -> TrafficIncidentSeverity {
    match severity {
        Some(everymap_traffic::IncidentSeverity::Low)
        | Some(everymap_traffic::IncidentSeverity::Minor)
        | None
        | Some(everymap_traffic::IncidentSeverity::Unknown) => TrafficIncidentSeverity::Low,
        Some(everymap_traffic::IncidentSeverity::Moderate) => TrafficIncidentSeverity::Medium,
        Some(everymap_traffic::IncidentSeverity::Major)
        | Some(everymap_traffic::IncidentSeverity::Critical) => TrafficIncidentSeverity::High,
    }
}

/// Projects a flow observation onto the SDK's record.
///
/// The provider layer reports the current and free-flow speeds in the unit
/// the provider used; the SDK field is kilometres per hour, and a provider
/// reached through this adapter must report that unit.
fn sdk_traffic_flow(flow: everymap_traffic::TrafficFlow) -> TrafficFlowData {
    let congestion_level = flow
        .congestion_level
        .map(sdk_congestion_level)
        .or_else(|| {
            flow.jam_factor
                .map(everymap_traffic::CongestionLevel::from_jam_factor)
                .map(sdk_congestion_level)
        })
        .or_else(|| match (flow.speed, flow.free_flow_speed) {
            (Some(current_speed), Some(free_flow_speed)) => flow.geometry.first().map(|midpoint| {
                TrafficFlowData::from_speeds(
                    sdk_coordinate(midpoint),
                    current_speed,
                    free_flow_speed,
                )
                .congestion_level
            }),
            _ => None,
        })
        .unwrap_or(TrafficFlowCongestionLevel::FreeFlow);
    TrafficFlowData {
        shape_coordinates: flow.geometry.iter().map(sdk_coordinate).collect(),
        current_speed_kilometers_per_hour: flow.speed,
        free_flow_speed_kilometers_per_hour: flow.free_flow_speed,
        congestion_level,
    }
}

/// Maps the provider layer's congestion classes onto the SDK's.
fn sdk_congestion_level(
    congestion_level: everymap_traffic::CongestionLevel,
) -> TrafficFlowCongestionLevel {
    match congestion_level {
        everymap_traffic::CongestionLevel::FreeFlow => TrafficFlowCongestionLevel::FreeFlow,
        everymap_traffic::CongestionLevel::Slow => TrafficFlowCongestionLevel::Slow,
        everymap_traffic::CongestionLevel::Congested => TrafficFlowCongestionLevel::Congested,
        everymap_traffic::CongestionLevel::Blocked => TrafficFlowCongestionLevel::Blocked,
    }
}

/// Projects one measured pair onto the SDK's matrix cell, or `None` when the
/// service reported no measurement for it.
fn sdk_distance_matrix_entry(
    entry: everymap_matrix::DistanceMatrixEntry,
) -> Option<DistanceMatrixEntry> {
    Some(DistanceMatrixEntry {
        origin_index: u32::try_from(entry.origin_index).ok()?,
        destination_index: u32::try_from(entry.destination_index).ok()?,
        distance_meters: entry.distance_meters?,
        duration_seconds: entry.duration_seconds?,
    })
}

/// Converts an SDK bounding box into the provider layer's.
fn everymap_bounding_box(bounding_box: &BoundingBox) -> everymap_core::BoundingBox {
    everymap_core::BoundingBox {
        south_west: everymap_core::Coordinate {
            lat: bounding_box.south_west.latitude,
            lng: bounding_box.south_west.longitude,
        },
        north_east: everymap_core::Coordinate {
            lat: bounding_box.north_east.latitude,
            lng: bounding_box.north_east.longitude,
        },
    }
}

/// The centre of a bounding box, the anchor the provider layer's area
/// requests take beside the box itself.
fn bounding_box_center(bounding_box: &BoundingBox) -> GeographicCoordinate {
    GeographicCoordinate {
        latitude: (bounding_box.south_west.latitude + bounding_box.north_east.latitude) / 2.0,
        longitude: (bounding_box.south_west.longitude + bounding_box.north_east.longitude) / 2.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use everymap_core::EveryMapError;
    use std::sync::Mutex;

    /// Serves whatever tile it is asked for, echoing the request.
    struct EchoRasterTileProvider;

    #[async_trait]
    impl everymap_tiling::RasterTileProvider for EchoRasterTileProvider {
        async fn fetch_raster_tile(
            &self,
            request: &everymap_tiling::RasterTileRequest,
        ) -> Result<everymap_tiling::RasterTileData, EveryMapError> {
            Ok(everymap_tiling::RasterTileData::for_request(
                request,
                vec![0x89, b'P', b'N', b'G'],
            ))
        }
    }

    /// Serves one tileset with a fixed descriptor and gzip-encoded tiles.
    struct GzippedVectorTileProvider;

    #[async_trait]
    impl everymap_tiling::VectorTileProvider for GzippedVectorTileProvider {
        fn source_descriptor(&self) -> everymap_tiling::VectorTileSourceDescriptor {
            everymap_tiling::VectorTileSourceDescriptor {
                source_name: "example/streets".to_string(),
                attribution_text: "Map data © example".to_string(),
                minimum_zoom_level: 0,
                maximum_zoom_level: 14,
                tile_encoding: everymap_tiling::VectorTileEncoding::GzippedMapboxVectorTile,
                style_hints: vec!["bright".to_string()],
            }
        }

        async fn fetch_vector_tile(
            &self,
            tile_x: u32,
            tile_y: u32,
            zoom_level: u8,
        ) -> Result<everymap_tiling::VectorTileData, EveryMapError> {
            Ok(everymap_tiling::VectorTileData {
                tile_x,
                tile_y,
                zoom_level,
                tile_encoding: everymap_tiling::VectorTileEncoding::GzippedMapboxVectorTile,
                tile_bytes: vec![0x1f, 0x8b, 0x08, 0x00],
            })
        }
    }

    #[tokio::test]
    async fn a_raster_tile_keeps_the_request_echo_and_its_bytes() {
        let provider = EveryMapRasterTileProvider::new(Arc::new(EchoRasterTileProvider));
        let tile = provider
            .fetch_raster_tile(
                3,
                4,
                5,
                RasterImageFormat::Png,
                RasterTileResolution::Retina,
            )
            .await
            .expect("the provider serves every request");
        assert_eq!(
            (tile.tile_x, tile.tile_y, tile.zoom_level),
            (3, 4, 5),
            "the response echoes the request's tile"
        );
        assert_eq!(tile.image_format, RasterImageFormat::Png);
        assert_eq!(tile.tile_resolution, RasterTileResolution::Retina);
        assert_eq!(tile.image_bytes, vec![0x89, b'P', b'N', b'G']);
    }

    #[tokio::test]
    async fn a_vector_tile_keeps_its_wire_encoding_and_descriptor() {
        let provider = EveryMapVectorTileProvider::new(Arc::new(GzippedVectorTileProvider));
        let descriptor = provider.source_descriptor();
        assert_eq!(descriptor.source_name, "example/streets");
        assert_eq!(descriptor.attribution_text, "Map data © example");
        assert_eq!(
            (descriptor.minimum_zoom_level, descriptor.maximum_zoom_level),
            (0, 14)
        );
        let tile = provider
            .fetch_vector_tile(1, 2, 3)
            .await
            .expect("the provider serves every tile");
        assert_eq!(
            tile.tile_encoding,
            VectorTileEncoding::GzippedMapboxVectorTile
        );
        assert_eq!(tile.tile_bytes, vec![0x1f, 0x8b, 0x08, 0x00]);
    }

    /// Serves one fixed navigation-grade route.
    struct FixedRouteRouter;

    #[async_trait]
    impl everymap_routing::Router for FixedRouteRouter {
        async fn calculate_route(
            &self,
            start: &everymap_core::Coordinate,
            end: &everymap_core::Coordinate,
            options: &everymap_routing::RouteOptions,
        ) -> Result<everymap_routing::RouteResponse, EveryMapError> {
            self.calculate_route_with_waypoints(&[*start, *end], options)
                .await
        }

        async fn calculate_route_with_waypoints(
            &self,
            waypoints: &[everymap_core::Coordinate],
            _options: &everymap_routing::RouteOptions,
        ) -> Result<everymap_routing::RouteResponse, EveryMapError> {
            Ok(everymap_routing::RouteResponse {
                routes: vec![everymap_routing::RouteResult {
                    distance: 1200.0,
                    duration: 90.0,
                    geometry: everymap_core::types::bounds::Polyline {
                        points: waypoints.to_vec(),
                    },
                    transport_mode: Some(everymap_routing::TransportMode::Car),
                    steps: Vec::new(),
                    bounding_box: None,
                    raw: None,
                    legs: vec![everymap_routing::RouteLeg {
                        distance: 1200.0,
                        duration: 90.0,
                        shape: waypoints.to_vec(),
                        steps: vec![everymap_routing::RouteStep {
                            instruction: Some("Turn right onto Main Street".to_string()),
                            distance: Some(1200.0),
                            duration: Some(90.0),
                            start_coordinate: Some(waypoints[0]),
                            end_coordinate: Some(waypoints[1]),
                            maneuver_type: Some(everymap_routing::ManeuverType::Turn(
                                everymap_routing::TurnDirection::Right,
                            )),
                            speech_instruction: Some("Turn right onto Main Street".to_string()),
                            lane_guidances: vec![everymap_routing::LaneGuidance {
                                lane_index: 1,
                                permitted_directions: vec![everymap_routing::TurnDirection::Right],
                                is_recommended: true,
                            }],
                            shape: waypoints.to_vec(),
                        }],
                    }],
                }],
            })
        }

        fn route_capabilities(&self) -> everymap_routing::RouteCapabilities {
            everymap_routing::RouteCapabilities {
                supported_transport_modes: vec![everymap_routing::TransportMode::Car],
                supports_shortest_route_strategy: true,
            }
        }
    }

    #[tokio::test]
    async fn a_route_keeps_its_legs_steps_and_capabilities() {
        let router = EveryMapRouter::new(Arc::new(FixedRouteRouter));
        let waypoints = [
            GeographicCoordinate {
                latitude: 38.58,
                longitude: -121.49,
            },
            GeographicCoordinate {
                latitude: 38.59,
                longitude: -121.48,
            },
        ];
        let route = router
            .calculate_route(&waypoints, &RouteOptions::default())
            .await
            .expect("the router serves every request");
        assert_eq!(route.distance_meters, 1200.0);
        assert_eq!(route.duration_seconds, 90.0);
        assert_eq!(route.legs.len(), 1);
        let step = &route.legs[0].steps[0];
        assert_eq!(step.maneuver_type, ManeuverType::Turn(TurnDirection::Right));
        assert_eq!(step.instruction_display_text, "Turn right onto Main Street");
        assert_eq!(step.lane_guidances[0].lane_index, 1);
        assert!(step.lane_guidances[0].is_recommended);
        // The provider's full geometry crosses the bridge in traversal order.
        assert_eq!(route.geometry.len(), 2);
        assert_eq!(route.geometry[0].latitude, 38.58);
        assert_eq!(route.geometry[1].longitude, -121.48);
        // The route reports no bounds, so they come from the full geometry.
        assert_eq!(route.bounding_box.south_west.latitude, 38.58);
        assert_eq!(route.bounding_box.north_east.longitude, -121.48);
        let capabilities = router.route_capabilities();
        assert_eq!(
            capabilities.supported_transportation_modes,
            vec![TransportationMode::Automobile]
        );
        assert!(capabilities.supports_shortest_route_strategy);
    }

    /// Answers every search with one fixed result at the given place.
    struct FixedGeocoder {
        quality: everymap_search::SearchQuality,
        label: &'static str,
    }

    fn fixed_search_result(
        label: &'static str,
        quality: everymap_search::SearchQuality,
    ) -> everymap_search::SearchResult {
        everymap_search::SearchResult {
            id: Some("place-1".to_string()),
            coordinate: everymap_core::Coordinate {
                lat: 52.52,
                lng: 13.40,
            },
            address: everymap_core::Address {
                label: Some(label.to_string()),
                ..everymap_core::Address::empty()
            },
            title: None,
            result_type: everymap_search::SearchResultType::ExactMatch,
            quality: Some(quality),
            distance: None,
            confidence: None,
            categories: Vec::new(),
            bounding_box: None,
            raw: None,
        }
    }

    #[async_trait]
    impl everymap_search::Geocoder for FixedGeocoder {
        async fn geocode(
            &self,
            _query: &str,
            _options: &everymap_search::GeocodeOptions,
        ) -> Result<everymap_search::SearchResponse, EveryMapError> {
            Ok(everymap_search::SearchResponse {
                items: vec![fixed_search_result(self.label, self.quality)],
            })
        }

        async fn reverse_geocode(
            &self,
            _coordinate: &everymap_core::Coordinate,
            _options: &everymap_search::ReverseGeocodeOptions,
        ) -> Result<everymap_search::SearchResponse, EveryMapError> {
            Ok(everymap_search::SearchResponse {
                items: vec![fixed_search_result(self.label, self.quality)],
            })
        }
    }

    #[tokio::test]
    async fn a_geocoded_place_keeps_its_label_and_granularity() {
        let geocoder = EveryMapGeocoder::new(Arc::new(FixedGeocoder {
            quality: everymap_search::SearchQuality::StreetAddress,
            label: "1 Main Street, Springfield",
        }));
        let places = geocoder
            .forward_geocode("1 Main Street")
            .await
            .expect("the geocoder answers");
        assert_eq!(places.len(), 1);
        assert_eq!(
            places[0].structured_address_text,
            "1 Main Street, Springfield"
        );
        assert_eq!(places[0].coordinate.latitude, 52.52);
        assert_eq!(places[0].geocode_quality, GeocodeQuality::StreetAddress);

        let reverse_places = geocoder
            .reverse_geocode(
                &GeographicCoordinate::from_degrees(52.52, 13.40)
                    .expect("the fixture coordinate is valid"),
            )
            .await
            .expect("the geocoder answers a reverse request");
        assert_eq!(reverse_places.len(), 1);
        assert_eq!(
            reverse_places[0].geocode_quality,
            GeocodeQuality::StreetAddress
        );
    }

    /// Records the boundary it was asked for and answers with one candidate
    /// that has a position and one that does not.
    struct ScriptedSearches {
        observed_boundary: Mutex<Option<Option<everymap_core::BoundingBox>>>,
    }

    #[async_trait]
    impl everymap_search::PlaceSearchProvider for ScriptedSearches {
        async fn search_places(
            &self,
            _query: &str,
            options: &everymap_search::PlaceSearchOptions,
        ) -> Result<everymap_search::SearchResponse, EveryMapError> {
            *self
                .observed_boundary
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(options.bounding_box);
            Ok(everymap_search::SearchResponse {
                items: vec![fixed_search_result(
                    "Settled result",
                    everymap_search::SearchQuality::City,
                )],
            })
        }
    }

    #[async_trait]
    impl everymap_autocomplete::AutocompleteProvider for ScriptedSearches {
        async fn autocomplete(
            &self,
            _query: &str,
            options: &everymap_autocomplete::AutocompleteOptions,
        ) -> Result<everymap_autocomplete::AutocompleteResponse, EveryMapError> {
            *self
                .observed_boundary
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(options.bounding_box);
            Ok(everymap_autocomplete::AutocompleteResponse {
                items: vec![
                    everymap_autocomplete::AutocompleteResult {
                        id: Some("candidate-1".to_string()),
                        title: "Berlin".to_string(),
                        subtitle: Some("Germany".to_string()),
                        coordinate: Some(everymap_core::Coordinate {
                            lat: 52.52,
                            lng: 13.40,
                        }),
                        bounding_box: None,
                        quality: Some(everymap_search::SearchQuality::Street),
                        categories: Vec::new(),
                        raw: None,
                    },
                    // A candidate that describes an area without a position:
                    // the SDK's place record needs one.
                    everymap_autocomplete::AutocompleteResult {
                        id: Some("candidate-2".to_string()),
                        title: "Brandenburg".to_string(),
                        subtitle: None,
                        coordinate: None,
                        bounding_box: None,
                        quality: None,
                        categories: Vec::new(),
                        raw: None,
                    },
                ],
            })
        }
    }

    #[tokio::test]
    async fn a_search_forwards_its_boundary_and_drops_positionless_candidates() {
        let searches = Arc::new(ScriptedSearches {
            observed_boundary: Mutex::new(None),
        });
        let provider = EveryMapPlaceSearchProvider::new(
            Arc::clone(&searches) as Arc<dyn everymap_search::PlaceSearchProvider>,
            Arc::clone(&searches) as Arc<dyn everymap_autocomplete::AutocompleteProvider>,
        );
        let boundary = BoundingBox {
            south_west: GeographicCoordinate::from_degrees(52.0, 13.0)
                .expect("the fixture box is valid"),
            north_east: GeographicCoordinate::from_degrees(53.0, 14.0)
                .expect("the fixture box is valid"),
        };

        let settled = provider
            .search_places("Berlin", Some(boundary))
            .await
            .expect("the search answers");
        assert_eq!(settled.len(), 1);
        assert_eq!(settled[0].geocode_quality, GeocodeQuality::City);
        let forwarded_settled = *searches
            .observed_boundary
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let forwarded_settled = forwarded_settled.expect("the settled search ran");
        let forwarded_settled = forwarded_settled.expect("the boundary reached the settled search");
        assert_eq!(forwarded_settled.south_west.lat, 52.0);
        assert_eq!(forwarded_settled.north_east.lng, 14.0);

        let candidates = provider
            .search_autocomplete("Berl", Some(boundary))
            .await
            .expect("the completion answers");
        assert_eq!(
            candidates.len(),
            1,
            "only the candidate with a position becomes a place"
        );
        assert_eq!(candidates[0].structured_address_text, "Berlin");
        assert_eq!(candidates[0].geocode_quality, GeocodeQuality::Street);
    }

    /// Answers every area request with one incident and three flow shapes.
    struct ScriptedTraffic;

    #[async_trait]
    impl everymap_traffic::TrafficProvider for ScriptedTraffic {
        async fn get_traffic(
            &self,
            _location: &everymap_core::Coordinate,
            options: &everymap_traffic::TrafficOptions,
        ) -> Result<everymap_traffic::TrafficResponse, EveryMapError> {
            let incident_position = everymap_core::Coordinate {
                lat: 39.74,
                lng: -104.99,
            };
            let classified = everymap_traffic::TrafficIncident {
                id: Some("incident-1".to_string()),
                incident_type: Some("accident".to_string()),
                severity: Some(everymap_traffic::IncidentSeverity::Major),
                description: Some("Collision on I-25".to_string()),
                road_name: None,
                start_time: None,
                end_time: None,
                start_coordinate: Some(incident_position),
                end_coordinate: None,
            };
            // The provider reports a disruption but no class for it.
            let unclassified = everymap_traffic::TrafficIncident {
                incident_type: None,
                ..classified.clone()
            };
            let segment = vec![
                everymap_core::Coordinate {
                    lat: 39.70,
                    lng: -105.00,
                },
                everymap_core::Coordinate {
                    lat: 39.71,
                    lng: -105.01,
                },
            ];
            Ok(everymap_traffic::TrafficResponse {
                flows: vec![
                    everymap_traffic::TrafficFlow {
                        speed: None,
                        free_flow_speed: None,
                        jam_factor: None,
                        confidence: None,
                        road_name: None,
                        geometry: segment.clone(),
                        congestion_level: Some(everymap_traffic::CongestionLevel::Congested),
                    },
                    everymap_traffic::TrafficFlow {
                        speed: None,
                        free_flow_speed: None,
                        jam_factor: Some(7.0),
                        confidence: None,
                        road_name: None,
                        geometry: segment.clone(),
                        congestion_level: None,
                    },
                    everymap_traffic::TrafficFlow {
                        speed: Some(20.0),
                        free_flow_speed: Some(100.0),
                        jam_factor: None,
                        confidence: None,
                        road_name: None,
                        geometry: segment,
                        congestion_level: None,
                    },
                ],
                incidents: if options.include_incidents.unwrap_or(false) {
                    vec![classified, unclassified]
                } else {
                    Vec::new()
                },
                raw: None,
            })
        }
    }

    #[tokio::test]
    async fn traffic_keeps_the_incidents_it_can_classify_and_the_flow_it_can_level() {
        let provider = EveryMapTrafficProvider::new(Arc::new(ScriptedTraffic));
        let bounding_box = BoundingBox {
            south_west: GeographicCoordinate::from_degrees(39.6, -105.1)
                .expect("the fixture box is valid"),
            north_east: GeographicCoordinate::from_degrees(39.8, -104.9)
                .expect("the fixture box is valid"),
        };

        let incidents = provider
            .fetch_traffic_incidents(&bounding_box)
            .await
            .expect("the incident request answers");
        assert_eq!(
            incidents.len(),
            1,
            "an incident without a class has no honest projection"
        );
        assert_eq!(incidents[0].incident_type, TrafficIncidentType::Accident);
        assert_eq!(incidents[0].severity, TrafficIncidentSeverity::High);
        assert_eq!(incidents[0].short_description_text, "Collision on I-25");

        let flows = provider
            .fetch_traffic_flow(&bounding_box)
            .await
            .expect("the flow request answers");
        assert_eq!(flows.len(), 3);
        assert_eq!(
            flows[0].congestion_level,
            TrafficFlowCongestionLevel::Congested
        );
        assert_eq!(flows[0].shape_coordinates.len(), 2);
        assert_eq!(
            flows[1].congestion_level,
            TrafficFlowCongestionLevel::Congested,
            "a jam factor classifies the segment when no bucket is reported"
        );
        assert_eq!(
            flows[2].congestion_level,
            TrafficFlowCongestionLevel::Congested,
            "speed ratio classifies the segment when nothing else does"
        );
        assert_eq!(flows[2].current_speed_kilometers_per_hour, Some(20.0));
    }

    /// Answers every matrix with a measured pair and an unmeasured one.
    struct ScriptedMatrix;

    #[async_trait]
    impl everymap_matrix::DistanceMatrixProvider for ScriptedMatrix {
        async fn distance_matrix(
            &self,
            _origins: &[everymap_core::Coordinate],
            _destinations: &[everymap_core::Coordinate],
            _options: &everymap_matrix::DistanceMatrixOptions,
        ) -> Result<everymap_matrix::DistanceMatrixResponse, EveryMapError> {
            Ok(everymap_matrix::DistanceMatrixResponse {
                entries: vec![
                    everymap_matrix::DistanceMatrixEntry {
                        origin_index: 0,
                        destination_index: 0,
                        distance_meters: Some(1200.0),
                        duration_seconds: Some(90.0),
                    },
                    everymap_matrix::DistanceMatrixEntry {
                        origin_index: 0,
                        destination_index: 1,
                        distance_meters: None,
                        duration_seconds: None,
                    },
                ],
            })
        }
    }

    #[tokio::test]
    async fn a_matrix_keeps_only_the_pairs_it_could_measure() {
        let provider = EveryMapDistanceMatrixProvider::new(Arc::new(ScriptedMatrix));
        let origin = GeographicCoordinate::from_degrees(52.52, 13.40)
            .expect("the fixture coordinate is valid");
        let destination = GeographicCoordinate::from_degrees(52.60, 13.50)
            .expect("the fixture coordinate is valid");
        let matrix = provider
            .calculate_distance_matrix(
                &[origin],
                &[origin, destination],
                TransportationMode::Automobile,
            )
            .await
            .expect("the matrix answers");
        assert_eq!(
            matrix.entries.len(),
            1,
            "an unmeasured pair is omitted, not reported as zero"
        );
        assert_eq!(matrix.entries[0].distance_meters, 1200.0);
        assert_eq!(matrix.entries[0].duration_seconds, 90.0);
    }

    #[test]
    fn provider_failures_keep_the_category_an_application_reacts_to() {
        assert!(matches!(
            map_provider_error(EveryMapError::RateLimited {
                provider: "example".to_string(),
                retry_after_secs: Some(2),
            }),
            SDKError::RateLimitExhausted
        ));
        assert!(matches!(
            map_provider_error(EveryMapError::HttpError {
                status: 429,
                message: "too many requests".to_string(),
                body: None,
            }),
            SDKError::RateLimitExhausted
        ));
        assert!(matches!(
            map_provider_error(EveryMapError::TransportError(
                "connection reset".to_string()
            )),
            SDKError::NetworkRequestFailed(_)
        ));
        assert!(matches!(
            map_provider_error(EveryMapError::ValidationError(
                "zoom out of range".to_string()
            )),
            SDKError::InvalidCoordinateInput(_)
        ));
        assert!(matches!(
            map_provider_error(EveryMapError::HttpError {
                status: 404,
                message: "not found".to_string(),
                body: None,
            }),
            SDKError::ProviderRequestRejected(_)
        ));
    }
}
