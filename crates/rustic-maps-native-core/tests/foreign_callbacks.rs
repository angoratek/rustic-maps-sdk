//! End-to-end tests of the foreign-callback provider injection (M4 slice 4):
//! a Rust-side implementation of the exported `[Trait, WithForeign]`
//! callback contracts — standing in for the foreign (Kotlin/Swift)
//! implementations the generated bindings would produce — registers through
//! `MapSdk::register_*_from_foreign`, and the awaited `MapSdk` calls resolve
//! future -> registry -> adapter -> callback contract -> binding record. A
//! failing callback contract must surface its `SdkError` variant after the
//! round trip through the core error model.

mod common;

use common::coordinate;
use std::sync::Arc;

use async_trait::async_trait;

use rustic_maps::{
    GeocodeQuality, GeocodingProvider, MapSdk, Place, RasterImageFormat, RasterTileData,
    RasterTileProvider, RasterTileResolution, SdkError,
};
use rustic_maps_test_support::{
    SACRAMENTO_LATITUDE_DEGREES as REQUEST_LATITUDE_DEGREES,
    SACRAMENTO_LATITUDE_DEGREES as ROUTE_ORIGIN_LATITUDE_DEGREES,
    SACRAMENTO_LONGITUDE_DEGREES as REQUEST_LONGITUDE_DEGREES,
    SACRAMENTO_LONGITUDE_DEGREES as ROUTE_ORIGIN_LONGITUDE_DEGREES,
};

/// Latitude of the forward-geocode response place, in decimal degrees.
const RESPONSE_LATITUDE_DEGREES: f64 = 38.57944;
/// Longitude of the forward-geocode response place, in decimal degrees.
const RESPONSE_LONGITUDE_DEGREES: f64 = -121.49085;
/// Address text the forward-geocode callback answers with.
const RESPONSE_ADDRESS_TEXT: &str = "Tower Bridge, Sacramento";
/// Reverse-geocode response address text composed from the echoed
/// coordinate, matching the mock echo convention of the provider suite.
const RESPONSE_REVERSE_ADDRESS_TEXT: &str = "reverse geocode 38.5766, -121.4944";
/// Detail text the failing geocoding callback throws.
const FOREIGN_FAILURE_DETAIL: &str = "the foreign geocoder rejected the request";
/// Tile column the raster callback echoes.
const TEST_TILE_X: u32 = 8459;
/// Tile row the raster callback echoes.
const TEST_TILE_Y: u32 = 5721;
/// Zoom level the raster test fetches at.
const TEST_ZOOM_LEVEL: u8 = 12;
/// Distinct texel count of the echoed tile payload.
const TEST_IMAGE_BYTE_COUNT: usize = 2048;

fn response_places() -> Vec<Place> {
    vec![Place {
        structured_address_text: RESPONSE_ADDRESS_TEXT.to_string(),
        coordinate: coordinate(RESPONSE_LATITUDE_DEGREES, RESPONSE_LONGITUDE_DEGREES),
        geocode_quality: GeocodeQuality::StreetAddress,
    }]
}

fn response_image_bytes() -> Vec<u8> {
    vec![7; TEST_IMAGE_BYTE_COUNT]
}

/// Geocoding callback implementation answering both contract calls with a
/// fixed success response, or throwing the configured failure.
struct FixedGeocodingCallback {
    success_places: Vec<Place>,
    failure: Option<SdkError>,
}

impl FixedGeocodingCallback {
    /// Builds the callback answering with the fixed places and an echo of
    /// every reverse-geocode coordinate.
    fn succeed_with(success_places: Vec<Place>) -> Self {
        Self {
            success_places,
            failure: None,
        }
    }

    /// Builds the callback throwing the fixed failure on every call.
    fn fail_with(failure: SdkError) -> Self {
        Self {
            success_places: Vec::new(),
            failure: Some(failure),
        }
    }
}

#[async_trait]
impl GeocodingProvider for FixedGeocodingCallback {
    async fn forward_geocode(&self, _address_text: String) -> Result<Vec<Place>, SdkError> {
        match &self.failure {
            Some(failure) => Err(failure.clone()),
            None => Ok(self.success_places.clone()),
        }
    }

    async fn reverse_geocode(
        &self,
        coordinate: rustic_maps::GeographicCoordinate,
    ) -> Result<Vec<Place>, SdkError> {
        match &self.failure {
            Some(failure) => Err(failure.clone()),
            None => Ok(vec![Place {
                structured_address_text: format!(
                    "reverse geocode {}, {}",
                    coordinate.latitude, coordinate.longitude
                ),
                coordinate,
                geocode_quality: GeocodeQuality::Street,
            }]),
        }
    }
}

/// Raster-tile callback implementation answering every fetch with the fixed
/// byte payload, or throwing the configured failure.
struct FixedRasterTileCallback {
    success_image_bytes: Vec<u8>,
    failure: Option<SdkError>,
}

impl FixedRasterTileCallback {
    /// Builds the callback answering with the fixed byte payload.
    fn succeed_with(success_image_bytes: Vec<u8>) -> Self {
        Self {
            success_image_bytes,
            failure: None,
        }
    }

    /// Builds the callback throwing the fixed failure on every fetch.
    fn fail_with(failure: SdkError) -> Self {
        Self {
            success_image_bytes: Vec::new(),
            failure: Some(failure),
        }
    }
}

#[async_trait]
impl RasterTileProvider for FixedRasterTileCallback {
    async fn fetch_raster_tile(
        &self,
        tile_x: u32,
        tile_y: u32,
        zoom_level: u8,
        image_format: RasterImageFormat,
        tile_resolution: RasterTileResolution,
    ) -> Result<RasterTileData, SdkError> {
        match &self.failure {
            Some(failure) => Err(failure.clone()),
            None => Ok(RasterTileData {
                tile_x,
                tile_y,
                zoom_level,
                image_format,
                tile_resolution,
                image_bytes: self.success_image_bytes.clone(),
            }),
        }
    }
}

#[tokio::test]
async fn foreign_geocoding_callback_serves_forward_geocode_end_to_end() {
    let map_sdk = MapSdk::new();
    map_sdk.register_geocoding_provider_from_foreign(Arc::new(
        FixedGeocodingCallback::succeed_with(response_places()),
    ));
    let places = map_sdk
        .forward_geocode("any address the callback ignores".to_string())
        .await
        .expect("the registered callback geocoder serves the address");
    assert_eq!(places, response_places());
}

#[tokio::test]
async fn foreign_geocoding_callback_serves_reverse_geocode_end_to_end() {
    let map_sdk = MapSdk::new();
    map_sdk.register_geocoding_provider_from_foreign(Arc::new(
        FixedGeocodingCallback::succeed_with(Vec::new()),
    ));
    let places = map_sdk
        .reverse_geocode(coordinate(
            REQUEST_LATITUDE_DEGREES,
            REQUEST_LONGITUDE_DEGREES,
        ))
        .await
        .expect("the registered callback geocoder serves the coordinate");
    assert_eq!(places.len(), 1);
    assert_eq!(
        places[0].structured_address_text,
        RESPONSE_REVERSE_ADDRESS_TEXT
    );
    assert_eq!(
        places[0].coordinate,
        coordinate(REQUEST_LATITUDE_DEGREES, REQUEST_LONGITUDE_DEGREES)
    );
    assert_eq!(places[0].geocode_quality, GeocodeQuality::Street);
}

#[tokio::test]
async fn foreign_raster_tile_callback_serves_the_fetch_end_to_end() {
    let map_sdk = MapSdk::new();
    map_sdk.register_raster_tile_provider_from_foreign(Arc::new(
        FixedRasterTileCallback::succeed_with(response_image_bytes()),
    ));
    let raster_tile = map_sdk
        .fetch_raster_tile(
            TEST_TILE_X,
            TEST_TILE_Y,
            TEST_ZOOM_LEVEL,
            RasterImageFormat::Png,
            RasterTileResolution::Retina,
        )
        .await
        .expect("the registered callback tile provider serves the address");
    assert_eq!(raster_tile.tile_x, TEST_TILE_X);
    assert_eq!(raster_tile.tile_y, TEST_TILE_Y);
    assert_eq!(raster_tile.zoom_level, TEST_ZOOM_LEVEL);
    assert_eq!(raster_tile.image_format, RasterImageFormat::Png);
    assert_eq!(raster_tile.tile_resolution, RasterTileResolution::Retina);
    assert_eq!(raster_tile.image_bytes, response_image_bytes());
}

#[tokio::test]
async fn foreign_geocoding_callback_error_surfaces_as_the_mapped_sdk_error_variant() {
    let map_sdk = MapSdk::new();
    map_sdk.register_geocoding_provider_from_foreign(Arc::new(FixedGeocodingCallback::fail_with(
        SdkError::ProviderRequestRejected {
            detail: FOREIGN_FAILURE_DETAIL.to_string(),
        },
    )));
    let failure = map_sdk
        .forward_geocode("any address the callback rejects".to_string())
        .await
        .expect_err("the failing callback must throw");
    assert_eq!(
        failure,
        SdkError::ProviderRequestRejected {
            detail: FOREIGN_FAILURE_DETAIL.to_string(),
        }
    );
}

#[tokio::test]
async fn foreign_raster_tile_callback_error_surfaces_as_the_mapped_sdk_error_variant() {
    let map_sdk = MapSdk::new();
    map_sdk.register_raster_tile_provider_from_foreign(Arc::new(
        FixedRasterTileCallback::fail_with(SdkError::RateLimitExhausted),
    ));
    let failure = map_sdk
        .fetch_raster_tile(
            TEST_TILE_X,
            TEST_TILE_Y,
            TEST_ZOOM_LEVEL,
            RasterImageFormat::Jpeg,
            RasterTileResolution::Standard,
        )
        .await
        .expect_err("the failing callback must throw");
    assert_eq!(failure, SdkError::RateLimitExhausted);
}

#[tokio::test]
async fn foreign_callback_mapped_failure_is_recorded_on_the_diagnostic_seam() {
    let map_sdk = MapSdk::new();
    map_sdk.register_geocoding_provider_from_foreign(Arc::new(FixedGeocodingCallback::fail_with(
        SdkError::NetworkRequestFailed {
            detail: FOREIGN_FAILURE_DETAIL.to_string(),
        },
    )));
    map_sdk
        .forward_geocode("any address the callback rejects".to_string())
        .await
        .expect_err("the failing callback must throw");
    let diagnostics = map_sdk.latest_provider_failure_diagnostics();
    assert_eq!(diagnostics.len(), 1, "exactly one recorded failure");
    let diagnostic = &diagnostics[0];
    assert_eq!(
        diagnostic.provider_contract_name, "GeocodingProvider",
        "the entry names the failing callback contract"
    );
    assert_eq!(
        diagnostic.provider_method_name, "forward_geocode",
        "the entry names the failing callback method"
    );
    assert_eq!(
        diagnostic.failure_classification,
        rustic_maps::ProviderFailureClassification::MappedContractError,
    );
    assert_eq!(
        diagnostic.failure_detail,
        "network request failed: the foreign geocoder rejected the request",
        "the mapped entry carries the thrown binding error's verbatim display form"
    );
    assert!(
        diagnostic.timestamp_milliseconds_since_epoch > 0,
        "the entry carries its recording timestamp"
    );
    // The readback is non-destructive: a second read (nothing failed between
    // the reads) returns the same single entry.
    assert_eq!(
        map_sdk.latest_provider_failure_diagnostics().len(),
        1,
        "the readback never clears the ring"
    );
}

#[tokio::test]
async fn foreign_callback_unclassified_failure_is_recorded_with_the_callback_reason() {
    let map_sdk = MapSdk::new();
    // A Rust-side callback cannot reproduce the uniffi unmapped-exception
    // path (which needs real foreign bindings), so the test stands in for it
    // the way the round-11 FFI-boundary pin tests do: the conversion of the
    // callback machinery's own error is driven directly, then handed to the
    // adapter as the callback's failure.
    let unclassified_failure: SdkError =
        uniffi::UnexpectedUniFFICallbackError::new(FOREIGN_FAILURE_DETAIL).into();
    map_sdk.register_geocoding_provider_from_foreign(Arc::new(FixedGeocodingCallback::fail_with(
        unclassified_failure,
    )));
    for _ in 0..2 {
        let failure = map_sdk
            .forward_geocode("any address the callback rejects".to_string())
            .await
            .expect_err("the failing callback must throw");
        assert!(
            failure
                .to_string()
                .contains("the provider implementation failed without a mapped error"),
            "the failure still surfaces typed as the round-11 conversion: {failure}"
        );
    }
    let diagnostics = map_sdk.latest_provider_failure_diagnostics();
    assert_eq!(diagnostics.len(), 2, "one entry per failed call");
    for diagnostic in &diagnostics {
        assert_eq!(
            diagnostic.provider_contract_name, "GeocodingProvider",
            "the entries name the failing contract"
        );
        assert_eq!(
            diagnostic.provider_method_name, "forward_geocode",
            "the entries name the failing callback method"
        );
        assert_eq!(
            diagnostic.failure_classification,
            rustic_maps::ProviderFailureClassification::UnclassifiedCallbackFailure,
            "the unclassified shape is recognized and reported as such"
        );
        assert_eq!(
            diagnostic.failure_detail, FOREIGN_FAILURE_DETAIL,
            "the unclassified entries carry the callback machinery's verbatim reason"
        );
    }
    assert!(
        diagnostics[0].timestamp_milliseconds_since_epoch
            <= diagnostics[1].timestamp_milliseconds_since_epoch,
        "the entries read back oldest first"
    );
}

#[tokio::test]
async fn foreign_callback_failures_accumulate_and_the_ring_is_per_map_sdk() {
    let map_sdk = MapSdk::new();
    map_sdk.register_raster_tile_provider_from_foreign(Arc::new(
        FixedRasterTileCallback::fail_with(SdkError::ProviderRequestRejected {
            detail: FOREIGN_FAILURE_DETAIL.to_string(),
        }),
    ));
    for _ in 0..3 {
        map_sdk
            .fetch_raster_tile(
                TEST_TILE_X,
                TEST_TILE_Y,
                TEST_ZOOM_LEVEL,
                RasterImageFormat::Png,
                RasterTileResolution::Standard,
            )
            .await
            .expect_err("the failing callback must throw");
    }
    let diagnostics = map_sdk.latest_provider_failure_diagnostics();
    assert_eq!(
        diagnostics.len(),
        3,
        "one entry per failed call, oldest first"
    );
    assert_eq!(diagnostics[0].provider_contract_name, "RasterTileProvider");
    assert_eq!(diagnostics[0].provider_method_name, "fetch_raster_tile");
    for window in diagnostics.windows(2) {
        assert!(
            window[0].timestamp_milliseconds_since_epoch
                <= window[1].timestamp_milliseconds_since_epoch,
            "the entries read back in recording order"
        );
    }
    // A separately constructed object's ring starts empty: the seam is
    // per MapSdk, not process-wide.
    assert_eq!(
        MapSdk::new().latest_provider_failure_diagnostics().len(),
        0,
        "a fresh MapSdk serves an empty provider-failure ring"
    );
}

// ---------------------------------------------------------------------------
// The six remaining provider contracts: a foreign implementation serves
// routing, distance matrix, place search, traffic incidents, traffic flow and
// elevation through the same registration seam and the same end-to-end path.
// ---------------------------------------------------------------------------

/// Latitude of the shared test destination, in decimal degrees.
const ROUTE_DESTINATION_LATITUDE_DEGREES: f64 = 38.5805;
/// Longitude of the shared test destination, in decimal degrees.
const ROUTE_DESTINATION_LONGITUDE_DEGREES: f64 = -121.5046;
/// Route distance the routing callback answers with, in meters.
const ROUTE_DISTANCE_METERS: f64 = 1_700.0;
/// Route travel time the routing callback answers with, in seconds.
const ROUTE_DURATION_SECONDS: f64 = 300.0;
/// Distance of the one matrix cell the callback answers with, in meters.
const MATRIX_CELL_DISTANCE_METERS: f64 = 1_680.0;
/// Travel time of the one matrix cell the callback answers with, in seconds.
const MATRIX_CELL_DURATION_SECONDS: f64 = 295.0;
/// Speed the flow callback reports for its one segment, in kilometers per
/// hour.
const FLOW_SPEED_KILOMETERS_PER_HOUR: f64 = 42.0;
/// Free-flow speed the flow callback reports, in kilometers per hour.
const FLOW_FREE_FLOW_KILOMETERS_PER_HOUR: f64 = 65.0;
/// Elevation the callback reports for every requested point, in meters.
const RESPONSE_ELEVATION_METERS: f64 = 9.5;
/// Detail text the failing routing callback throws.
const FOREIGN_ROUTING_FAILURE_DETAIL: &str = "the foreign router rejected the request";

fn route_options() -> rustic_maps::RouteOptions {
    rustic_maps::RouteOptions {
        transportation_mode: rustic_maps::TransportationMode::Automobile,
        route_strategy: rustic_maps::RouteStrategy::Fastest,
        unit_system: rustic_maps::UnitSystem::Metric,
        avoid_options: rustic_maps::AvoidOptions {
            toll_roads: true,
            limited_access_highways: false,
            ferries: false,
            unpaved_roads: false,
        },
        departure_timestamp_seconds: None,
    }
}

fn bounding_box() -> rustic_maps::BoundingBox {
    rustic_maps::BoundingBox {
        south_west: coordinate(
            ROUTE_ORIGIN_LATITUDE_DEGREES,
            ROUTE_DESTINATION_LONGITUDE_DEGREES,
        ),
        north_east: coordinate(
            ROUTE_DESTINATION_LATITUDE_DEGREES,
            ROUTE_ORIGIN_LONGITUDE_DEGREES,
        ),
    }
}

fn response_route() -> rustic_maps::Route {
    rustic_maps::Route {
        distance_meters: ROUTE_DISTANCE_METERS,
        duration_seconds: ROUTE_DURATION_SECONDS,
        bounding_box: bounding_box(),
        geometry: Vec::new(),
        legs: vec![rustic_maps::RouteLeg {
            distance_meters: ROUTE_DISTANCE_METERS,
            duration_seconds: ROUTE_DURATION_SECONDS,
            steps: vec![rustic_maps::RouteStep {
                distance_meters: ROUTE_DISTANCE_METERS,
                duration_seconds: ROUTE_DURATION_SECONDS,
                maneuver_type: "turn-left".to_string(),
                instruction_display_text: "Turn left onto Tower Bridge".to_string(),
                instruction_speech_text: "Turn left onto Tower Bridge".to_string(),
                shape: vec![
                    coordinate(
                        ROUTE_ORIGIN_LATITUDE_DEGREES,
                        ROUTE_ORIGIN_LONGITUDE_DEGREES,
                    ),
                    coordinate(
                        ROUTE_DESTINATION_LATITUDE_DEGREES,
                        ROUTE_DESTINATION_LONGITUDE_DEGREES,
                    ),
                ],
            }],
        }],
    }
}

/// Routing callback implementation answering with a fixed route and recording
/// the options it received, or throwing the configured failure.
struct RecordingRoutingCallback {
    response_route: rustic_maps::Route,
    recorded_options: std::sync::Mutex<Option<rustic_maps::RouteOptions>>,
    failure: Option<SdkError>,
}

impl RecordingRoutingCallback {
    fn succeed_with(response_route: rustic_maps::Route) -> Self {
        Self {
            response_route,
            recorded_options: std::sync::Mutex::new(None),
            failure: None,
        }
    }

    fn fail_with(failure: SdkError) -> Self {
        Self {
            response_route: response_route(),
            recorded_options: std::sync::Mutex::new(None),
            failure: Some(failure),
        }
    }
}

#[async_trait]
impl rustic_maps::RoutingProvider for RecordingRoutingCallback {
    async fn calculate_route(
        &self,
        _waypoints: Vec<rustic_maps::GeographicCoordinate>,
        route_options: rustic_maps::RouteOptions,
    ) -> Result<rustic_maps::Route, SdkError> {
        *self
            .recorded_options
            .lock()
            .expect("the recorded options lock is never poisoned") = Some(route_options);
        match &self.failure {
            Some(failure) => Err(failure.clone()),
            None => Ok(self.response_route.clone()),
        }
    }
}

#[tokio::test]
async fn foreign_routing_callback_serves_calculate_route_end_to_end() {
    let map_sdk = MapSdk::new();
    map_sdk.register_routing_provider_from_foreign(Arc::new(
        RecordingRoutingCallback::succeed_with(response_route()),
    ));
    let planned_route = map_sdk
        .calculate_route(
            vec![
                coordinate(
                    ROUTE_ORIGIN_LATITUDE_DEGREES,
                    ROUTE_ORIGIN_LONGITUDE_DEGREES,
                ),
                coordinate(
                    ROUTE_DESTINATION_LATITUDE_DEGREES,
                    ROUTE_DESTINATION_LONGITUDE_DEGREES,
                ),
            ],
            route_options(),
        )
        .await
        .expect("the registered callback router serves the route");
    assert_eq!(planned_route, response_route());
}

#[tokio::test]
async fn foreign_routing_callback_error_surfaces_as_the_mapped_sdk_error_variant() {
    let map_sdk = MapSdk::new();
    map_sdk.register_routing_provider_from_foreign(Arc::new(RecordingRoutingCallback::fail_with(
        SdkError::ProviderRequestRejected {
            detail: FOREIGN_ROUTING_FAILURE_DETAIL.to_string(),
        },
    )));
    let failure = map_sdk
        .calculate_route(
            vec![
                coordinate(
                    ROUTE_ORIGIN_LATITUDE_DEGREES,
                    ROUTE_ORIGIN_LONGITUDE_DEGREES,
                ),
                coordinate(
                    ROUTE_DESTINATION_LATITUDE_DEGREES,
                    ROUTE_DESTINATION_LONGITUDE_DEGREES,
                ),
            ],
            route_options(),
        )
        .await
        .expect_err("the failing callback must throw");
    assert_eq!(
        failure,
        SdkError::ProviderRequestRejected {
            detail: FOREIGN_ROUTING_FAILURE_DETAIL.to_string(),
        }
    );
}

/// Distance-matrix callback answering one cell for every origin x destination
/// pair, or throwing the configured failure.
struct FixedDistanceMatrixCallback {
    failure: Option<SdkError>,
}

#[async_trait]
impl rustic_maps::DistanceMatrixProvider for FixedDistanceMatrixCallback {
    async fn calculate_distance_matrix(
        &self,
        origins: Vec<rustic_maps::GeographicCoordinate>,
        destinations: Vec<rustic_maps::GeographicCoordinate>,
        _transportation_mode: rustic_maps::TransportationMode,
    ) -> Result<rustic_maps::DistanceMatrix, SdkError> {
        if let Some(failure) = &self.failure {
            return Err(failure.clone());
        }
        let mut entries = Vec::new();
        for (origin_index, _) in origins.iter().enumerate() {
            for (destination_index, _) in destinations.iter().enumerate() {
                entries.push(rustic_maps::DistanceMatrixEntry {
                    origin_index: origin_index as u32,
                    destination_index: destination_index as u32,
                    distance_meters: MATRIX_CELL_DISTANCE_METERS,
                    duration_seconds: MATRIX_CELL_DURATION_SECONDS,
                });
            }
        }
        Ok(rustic_maps::DistanceMatrix { entries })
    }
}

#[tokio::test]
async fn foreign_distance_matrix_callback_serves_the_matrix_end_to_end() {
    let map_sdk = MapSdk::new();
    map_sdk.register_distance_matrix_provider_from_foreign(Arc::new(FixedDistanceMatrixCallback {
        failure: None,
    }));
    let matrix = map_sdk
        .calculate_distance_matrix(
            vec![coordinate(
                ROUTE_ORIGIN_LATITUDE_DEGREES,
                ROUTE_ORIGIN_LONGITUDE_DEGREES,
            )],
            vec![coordinate(
                ROUTE_DESTINATION_LATITUDE_DEGREES,
                ROUTE_DESTINATION_LONGITUDE_DEGREES,
            )],
            rustic_maps::TransportationMode::Automobile,
        )
        .await
        .expect("the registered callback matrix provider serves the matrix");
    assert_eq!(matrix.entries.len(), 1);
    assert_eq!(matrix.entries[0].origin_index, 0);
    assert_eq!(matrix.entries[0].destination_index, 0);
    assert_eq!(
        matrix.entries[0].distance_meters,
        MATRIX_CELL_DISTANCE_METERS
    );
    assert_eq!(
        matrix.entries[0].duration_seconds,
        MATRIX_CELL_DURATION_SECONDS
    );
}

/// Place-search callback answering the fixed place for both calls and
/// recording whether a boundary arrived.
struct FixedSearchPlaceCallback {
    recorded_boundary_is_present: std::sync::Mutex<bool>,
}

#[async_trait]
impl rustic_maps::SearchPlaceProvider for FixedSearchPlaceCallback {
    async fn search_autocomplete(
        &self,
        _query_text: String,
        search_boundary: Option<rustic_maps::BoundingBox>,
    ) -> Result<Vec<Place>, SdkError> {
        *self
            .recorded_boundary_is_present
            .lock()
            .expect("the recorded boundary lock is never poisoned") = search_boundary.is_some();
        Ok(response_places())
    }

    async fn search_places(
        &self,
        _query_text: String,
        search_boundary: Option<rustic_maps::BoundingBox>,
    ) -> Result<Vec<Place>, SdkError> {
        *self
            .recorded_boundary_is_present
            .lock()
            .expect("the recorded boundary lock is never poisoned") = search_boundary.is_some();
        Ok(response_places())
    }
}

#[tokio::test]
async fn foreign_search_place_callback_serves_search_and_suggest_end_to_end() {
    let search_callback = Arc::new(FixedSearchPlaceCallback {
        recorded_boundary_is_present: std::sync::Mutex::new(false),
    });
    let map_sdk = MapSdk::new();
    map_sdk.register_search_place_provider_from_foreign(Arc::clone(&search_callback) as Arc<_>);
    let suggested_places = map_sdk
        .suggest("Tower".to_string(), Some(bounding_box()))
        .await
        .expect("the registered callback search provider serves the suggestion");
    assert_eq!(suggested_places, response_places());
    assert!(
        *search_callback
            .recorded_boundary_is_present
            .lock()
            .expect("the recorded boundary lock is never poisoned"),
        "the boundary reached the foreign implementation",
    );
    let searched_places = map_sdk
        .search("Tower Bridge".to_string(), None)
        .await
        .expect("the registered callback search provider serves the search");
    assert_eq!(searched_places, response_places());
}

/// Traffic-incident callback answering the fixed incident.
struct FixedTrafficIncidentCallback;

#[async_trait]
impl rustic_maps::TrafficIncidentProvider for FixedTrafficIncidentCallback {
    async fn fetch_traffic_incidents(
        &self,
        _bounding_box: rustic_maps::BoundingBox,
    ) -> Result<Vec<rustic_maps::TrafficIncident>, SdkError> {
        Ok(vec![rustic_maps::TrafficIncident {
            identifier: "incident-1".to_string(),
            incident_type: rustic_maps::TrafficIncidentType::Accident,
            severity: rustic_maps::TrafficIncidentSeverity::High,
            short_description_text: "Collision on the bridge".to_string(),
            start_coordinate: coordinate(
                ROUTE_ORIGIN_LATITUDE_DEGREES,
                ROUTE_ORIGIN_LONGITUDE_DEGREES,
            ),
            end_coordinate: None,
        }])
    }
}

#[tokio::test]
async fn foreign_traffic_incident_callback_serves_the_fetch_end_to_end() {
    let map_sdk = MapSdk::new();
    map_sdk.register_traffic_incident_provider_from_foreign(Arc::new(FixedTrafficIncidentCallback));
    let incidents = map_sdk
        .fetch_traffic_incidents(bounding_box())
        .await
        .expect("the registered callback incident provider serves the box");
    assert_eq!(incidents.len(), 1);
    assert_eq!(incidents[0].identifier, "incident-1");
    assert_eq!(
        incidents[0].incident_type,
        rustic_maps::TrafficIncidentType::Accident
    );
    assert_eq!(
        incidents[0].severity,
        rustic_maps::TrafficIncidentSeverity::High
    );
}

/// Traffic-flow callback answering the fixed segment.
struct FixedTrafficFlowCallback;

#[async_trait]
impl rustic_maps::TrafficFlowProvider for FixedTrafficFlowCallback {
    async fn fetch_traffic_flow(
        &self,
        _bounding_box: rustic_maps::BoundingBox,
    ) -> Result<Vec<rustic_maps::TrafficFlowData>, SdkError> {
        Ok(vec![rustic_maps::TrafficFlowData {
            shape_coordinates: vec![
                coordinate(
                    ROUTE_ORIGIN_LATITUDE_DEGREES,
                    ROUTE_ORIGIN_LONGITUDE_DEGREES,
                ),
                coordinate(
                    ROUTE_DESTINATION_LATITUDE_DEGREES,
                    ROUTE_DESTINATION_LONGITUDE_DEGREES,
                ),
            ],
            current_speed_kilometers_per_hour: Some(FLOW_SPEED_KILOMETERS_PER_HOUR),
            free_flow_speed_kilometers_per_hour: Some(FLOW_FREE_FLOW_KILOMETERS_PER_HOUR),
            congestion_level: rustic_maps::TrafficFlowCongestionLevel::Slow,
        }])
    }
}

#[tokio::test]
async fn foreign_traffic_flow_callback_serves_the_fetch_end_to_end() {
    let map_sdk = MapSdk::new();
    map_sdk.register_traffic_flow_provider_from_foreign(Arc::new(FixedTrafficFlowCallback));
    let flow_observations = map_sdk
        .fetch_traffic_flow(bounding_box())
        .await
        .expect("the registered callback flow provider serves the box");
    assert_eq!(flow_observations.len(), 1);
    assert_eq!(
        flow_observations[0].current_speed_kilometers_per_hour,
        Some(FLOW_SPEED_KILOMETERS_PER_HOUR)
    );
    assert_eq!(
        flow_observations[0].congestion_level,
        rustic_maps::TrafficFlowCongestionLevel::Slow
    );
    assert_eq!(flow_observations[0].shape_coordinates.len(), 2);
}

/// Elevation callback answering one sample per requested point.
struct FixedElevationCallback;

#[async_trait]
impl rustic_maps::ElevationProvider for FixedElevationCallback {
    async fn fetch_elevation_points(
        &self,
        points: Vec<rustic_maps::GeographicCoordinate>,
    ) -> Result<Vec<rustic_maps::ElevationPoint>, SdkError> {
        Ok(points
            .into_iter()
            .map(|point| rustic_maps::ElevationPoint {
                coordinate: point,
                elevation_meters: RESPONSE_ELEVATION_METERS,
            })
            .collect())
    }
}

#[tokio::test]
async fn foreign_elevation_callback_serves_the_fetch_end_to_end() {
    let map_sdk = MapSdk::new();
    map_sdk.register_elevation_provider_from_foreign(Arc::new(FixedElevationCallback));
    let requested_points = vec![
        coordinate(
            ROUTE_ORIGIN_LATITUDE_DEGREES,
            ROUTE_ORIGIN_LONGITUDE_DEGREES,
        ),
        coordinate(
            ROUTE_DESTINATION_LATITUDE_DEGREES,
            ROUTE_DESTINATION_LONGITUDE_DEGREES,
        ),
    ];
    let elevation_points = map_sdk
        .fetch_elevation_points(requested_points.clone())
        .await
        .expect("the registered callback elevation provider serves the points");
    assert_eq!(elevation_points.len(), requested_points.len());
    for (elevation_point, requested_point) in elevation_points.iter().zip(requested_points) {
        assert_eq!(elevation_point.coordinate, requested_point);
        assert_eq!(elevation_point.elevation_meters, RESPONSE_ELEVATION_METERS);
    }
}

#[tokio::test]
async fn elevation_without_a_provider_reports_not_configured() {
    let map_sdk = MapSdk::new();
    let failure = map_sdk
        .fetch_elevation_points(vec![coordinate(
            ROUTE_ORIGIN_LATITUDE_DEGREES,
            ROUTE_ORIGIN_LONGITUDE_DEGREES,
        )])
        .await
        .expect_err("an unregistered elevation slot refuses the call");
    assert!(matches!(failure, SdkError::ProviderNotConfigured { .. }));
}

#[tokio::test]
async fn foreign_registration_reports_the_application_provider_for_each_contract() {
    let map_sdk = MapSdk::new();
    map_sdk.register_routing_provider_from_foreign(Arc::new(
        RecordingRoutingCallback::succeed_with(response_route()),
    ));
    map_sdk.register_distance_matrix_provider_from_foreign(Arc::new(FixedDistanceMatrixCallback {
        failure: None,
    }));
    map_sdk.register_search_place_provider_from_foreign(Arc::new(FixedSearchPlaceCallback {
        recorded_boundary_is_present: std::sync::Mutex::new(false),
    }));
    map_sdk.register_traffic_incident_provider_from_foreign(Arc::new(FixedTrafficIncidentCallback));
    map_sdk.register_traffic_flow_provider_from_foreign(Arc::new(FixedTrafficFlowCallback));
    map_sdk.register_elevation_provider_from_foreign(Arc::new(FixedElevationCallback));

    let registrations = map_sdk.provider_registrations();
    for contract in [
        rustic_maps::ProviderContract::Routing,
        rustic_maps::ProviderContract::DistanceMatrix,
        rustic_maps::ProviderContract::PlaceSearch,
        rustic_maps::ProviderContract::TrafficIncidents,
        rustic_maps::ProviderContract::TrafficFlow,
        rustic_maps::ProviderContract::Elevation,
    ] {
        let registration = registrations
            .iter()
            .find(|entry| entry.contract == contract)
            .expect("every capability is reported exactly once");
        assert!(registration.registered, "{contract:?} reports registered");
        assert_eq!(registration.provider_key, "application");
        assert_eq!(registration.display_name, "Application-provided");
    }
}

#[tokio::test]
async fn route_capabilities_reports_the_serving_provider_and_fails_without_one() {
    let map_sdk = MapSdk::new();
    let failure = map_sdk
        .route_capabilities()
        .expect_err("an unregistered routing slot refuses the readback");
    assert!(matches!(failure, SdkError::ProviderNotConfigured { .. }));

    map_sdk.register_routing_provider_from_foreign(Arc::new(
        RecordingRoutingCallback::succeed_with(response_route()),
    ));
    let capabilities = map_sdk
        .route_capabilities()
        .expect("the registered router reports its capabilities");
    assert_eq!(capabilities.supported_transportation_modes.len(), 3);
    assert!(capabilities.supports_shortest_route_strategy);
}
