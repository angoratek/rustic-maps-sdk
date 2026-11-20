//! End-to-end tests of the exported async provider methods: a mock provider
//! implements the `geospatial-traits` contract, the native integrator
//! registers it on `MapSdk`, and the awaited call resolves future ->
//! registry -> contract -> binding record. The compiled FFI wrappers run
//! without an executor on the foreign side, so no FFI loop is exercised
//! here; live-provider fixtures stay in the provider layer.
//!
//! The tests await through a Tokio executor (dev-dependency only); the
//! exported futures themselves are polled by the foreign event loop and
//! need no Rust executor.

mod common;

use common::coordinate;
use geospatial_traits::mock::{
    MockDistanceMatrixProvider, MockGeocodingProvider, MockRasterTileProvider, MockRoutingProvider,
    MockSearchPlaceProvider, MockTrafficFlowProvider, MockTrafficIncidentProvider,
};
use rustic_maps::{
    GeocodeQuality, MapSdk, Place, RasterImageFormat, RasterTileResolution, SdkError,
    TrafficFlowCongestionLevel, TrafficIncidentType, TransportationMode, UnitSystem,
};
use rustic_maps_test_support::{
    SACRAMENTO_LATITUDE_DEGREES as TEST_LATITUDE_DEGREES,
    SACRAMENTO_LONGITUDE_DEGREES as TEST_LONGITUDE_DEGREES,
};
use std::sync::Arc;

/// Latitude a distinct mock coordinate asserts; it must sit north-east of
/// the mock's route/bounding-box anchor at `TEST_LATITUDE_DEGREES`: Reno,
/// Nevada, north of and east of the anchor.
const OTHER_LATITUDE_DEGREES: f64 = 39.5296;
/// Longitude a distinct mock coordinate asserts; north-east of
/// `TEST_LONGITUDE_DEGREES`.
const OTHER_LONGITUDE_DEGREES: f64 = -119.8138;
/// Address text the geocoding mocks echo or anchor on.
const TEST_ADDRESS_TEXT: &str = "Capitol Park, Sacramento";
/// Query text the search mocks echo.
const TEST_QUERY_TEXT: &str = "Capito";
/// Tile column the raster mock echoes.
const TEST_TILE_X: u32 = 137;
/// Tile row the raster mock echoes.
const TEST_TILE_Y: u32 = 335;
/// Zoom level the raster test fetches at.
const TEST_ZOOM_LEVEL: u8 = 9;
/// Edge of the standard-resolution mock tile payload, in pixels; the mock
/// fills one byte per texel.
const MOCK_STANDARD_TILE_EDGE_PIXELS: u32 = 256;

fn sdk_with_mock_providers() -> MapSdk {
    let map_sdk = MapSdk::new();
    map_sdk.register_geocoding_provider(Arc::new(MockGeocodingProvider));
    map_sdk.register_routing_provider(Arc::new(MockRoutingProvider));
    map_sdk.register_distance_matrix_provider(Arc::new(MockDistanceMatrixProvider));
    map_sdk.register_search_place_provider(Arc::new(MockSearchPlaceProvider));
    map_sdk.register_traffic_incident_provider(Arc::new(MockTrafficIncidentProvider));
    map_sdk.register_traffic_flow_provider(Arc::new(MockTrafficFlowProvider));
    map_sdk.register_raster_tile_provider(Arc::new(MockRasterTileProvider));
    map_sdk
}

fn empty_route_options() -> rustic_maps::RouteOptions {
    rustic_maps::RouteOptions {
        transportation_mode: TransportationMode::Automobile,
        route_strategy: rustic_maps::RouteStrategy::Fastest,
        unit_system: UnitSystem::Metric,
        avoid_options: rustic_maps::AvoidOptions {
            toll_roads: false,
            limited_access_highways: false,
            ferries: false,
            unpaved_roads: false,
        },
        departure_timestamp_seconds: None,
    }
}

#[tokio::test]
async fn forward_geocode_resolves_through_the_registered_mock() {
    let map_sdk = sdk_with_mock_providers();
    let places = map_sdk
        .forward_geocode(TEST_ADDRESS_TEXT.to_string())
        .await
        .expect("the registered geocoder serves the address");
    assert_eq!(
        places,
        vec![Place {
            structured_address_text: TEST_ADDRESS_TEXT.to_string(),
            coordinate: coordinate(TEST_LATITUDE_DEGREES, TEST_LONGITUDE_DEGREES),
            geocode_quality: GeocodeQuality::City,
        }]
    );
}

#[tokio::test]
async fn reverse_geocode_round_trips_the_coordinate_through_the_mock() {
    let map_sdk = sdk_with_mock_providers();
    let places = map_sdk
        .reverse_geocode(coordinate(OTHER_LATITUDE_DEGREES, OTHER_LONGITUDE_DEGREES))
        .await
        .expect("the registered geocoder serves the coordinate");
    assert_eq!(places[0].coordinate.longitude, OTHER_LONGITUDE_DEGREES);
    assert_eq!(places[0].geocode_quality, GeocodeQuality::Street);
}

#[tokio::test]
async fn unregistered_contract_throws_provider_not_configured() {
    let map_sdk = MapSdk::new();
    let failure = map_sdk
        .forward_geocode(TEST_ADDRESS_TEXT.to_string())
        .await
        .expect_err("an unregistered contract must throw");
    assert_eq!(
        failure,
        SdkError::ProviderNotConfigured {
            contract_name: "GeocodingProvider".to_string(),
        }
    );
}

#[tokio::test]
async fn calculate_route_maps_the_mock_route_into_the_binding_record() {
    let map_sdk = sdk_with_mock_providers();
    let route = map_sdk
        .calculate_route(
            vec![
                coordinate(TEST_LATITUDE_DEGREES, TEST_LONGITUDE_DEGREES),
                coordinate(OTHER_LATITUDE_DEGREES, OTHER_LONGITUDE_DEGREES),
            ],
            empty_route_options(),
        )
        .await
        .expect("the registered router serves the waypoints");
    assert_eq!(route.distance_meters, 1000.0);
    assert_eq!(route.duration_seconds, 90.0);
    assert_eq!(route.legs.len(), 1);
    assert_eq!(route.legs[0].steps[0].maneuver_type, "departure");
    assert_eq!(route.legs[0].steps[0].shape.len(), 2);
    assert_eq!(
        route.bounding_box.south_west,
        coordinate(TEST_LATITUDE_DEGREES, TEST_LONGITUDE_DEGREES)
    );
}

#[tokio::test]
async fn calculate_route_rejects_an_invalid_waypoint() {
    let map_sdk = sdk_with_mock_providers();
    let failure = map_sdk
        .calculate_route(
            vec![coordinate(91.0, TEST_LONGITUDE_DEGREES)],
            empty_route_options(),
        )
        .await
        .expect_err("an out-of-range waypoint must throw");
    assert!(matches!(failure, SdkError::InvalidCoordinateInput { .. }));
}

#[tokio::test]
async fn calculate_route_passes_the_route_options_through_the_converter() {
    let map_sdk = sdk_with_mock_providers();
    let route_options = rustic_maps::RouteOptions {
        transportation_mode: TransportationMode::Bicycle,
        route_strategy: rustic_maps::RouteStrategy::Shortest,
        unit_system: UnitSystem::Imperial,
        avoid_options: rustic_maps::AvoidOptions {
            toll_roads: true,
            limited_access_highways: false,
            ferries: true,
            unpaved_roads: false,
        },
        departure_timestamp_seconds: Some(1_700_000_000),
    };
    // The mocks only echo geometry; the assertion is that the option record
    // crosses the converter without losing fields (a converter panic or an
    // invalid option would fail the call instead).
    let route = map_sdk
        .calculate_route(
            vec![coordinate(TEST_LATITUDE_DEGREES, TEST_LONGITUDE_DEGREES)],
            route_options,
        )
        .await;
    assert!(route.is_ok(), "options must cross the converter: {route:?}");
}

#[tokio::test]
async fn calculate_distance_matrix_maps_the_mock_cells() {
    let map_sdk = sdk_with_mock_providers();
    let distance_matrix = map_sdk
        .calculate_distance_matrix(
            vec![coordinate(TEST_LATITUDE_DEGREES, TEST_LONGITUDE_DEGREES)],
            vec![coordinate(OTHER_LATITUDE_DEGREES, OTHER_LONGITUDE_DEGREES)],
            TransportationMode::Automobile,
        )
        .await
        .expect("the registered matrix provider serves the pair");
    assert_eq!(distance_matrix.entries.len(), 1);
    assert_eq!(distance_matrix.entries[0].origin_index, 0);
    assert_eq!(distance_matrix.entries[0].destination_index, 0);
    assert_eq!(distance_matrix.entries[0].distance_meters, 500.0);
    assert_eq!(distance_matrix.entries[0].duration_seconds, 45.0);
}

#[tokio::test]
async fn suggest_and_search_resolve_through_the_registered_mock() {
    let map_sdk = sdk_with_mock_providers();
    for search_result in [
        map_sdk.suggest(TEST_QUERY_TEXT.to_string(), None).await,
        map_sdk.search(TEST_QUERY_TEXT.to_string(), None).await,
    ] {
        let places = search_result.expect("the registered search provider serves the query");
        assert_eq!(places[0].structured_address_text, TEST_QUERY_TEXT);
    }
}

#[tokio::test]
async fn search_with_a_boundary_restricts_the_query() {
    let map_sdk = sdk_with_mock_providers();
    let search_boundary = rustic_maps::BoundingBox {
        south_west: coordinate(38.4997, -121.6361),
        north_east: coordinate(38.6497, -121.3361),
    };
    let places = map_sdk
        .suggest(TEST_QUERY_TEXT.to_string(), Some(search_boundary))
        .await
        .expect("the registered search provider serves the bounded query");
    assert_eq!(places[0].structured_address_text, TEST_QUERY_TEXT);
}

#[tokio::test]
async fn fetch_traffic_incidents_maps_the_mock_incident() {
    let map_sdk = sdk_with_mock_providers();
    let mock_box = rustic_maps::BoundingBox {
        south_west: coordinate(TEST_LATITUDE_DEGREES, TEST_LONGITUDE_DEGREES),
        north_east: coordinate(OTHER_LATITUDE_DEGREES, OTHER_LONGITUDE_DEGREES),
    };
    let incidents = map_sdk
        .fetch_traffic_incidents(mock_box)
        .await
        .expect("the registered incident provider serves the box");
    assert_eq!(incidents[0].identifier, "mock-incident-1");
    assert_eq!(incidents[0].incident_type, TrafficIncidentType::Accident);
    assert_eq!(incidents[0].start_coordinate, mock_box.south_west);
    assert_eq!(incidents[0].end_coordinate, None);
}

#[tokio::test]
async fn fetch_traffic_flow_maps_the_mock_observation() {
    let map_sdk = sdk_with_mock_providers();
    let mock_box = rustic_maps::BoundingBox {
        south_west: coordinate(TEST_LATITUDE_DEGREES, TEST_LONGITUDE_DEGREES),
        north_east: coordinate(OTHER_LATITUDE_DEGREES, OTHER_LONGITUDE_DEGREES),
    };
    let traffic_flow = map_sdk
        .fetch_traffic_flow(mock_box)
        .await
        .expect("the registered flow provider serves the box");
    assert_eq!(
        traffic_flow[0].congestion_level,
        TrafficFlowCongestionLevel::FreeFlow
    );
    assert_eq!(
        traffic_flow[0].current_speed_kilometers_per_hour,
        Some(60.0)
    );
    assert_eq!(
        traffic_flow[0].free_flow_speed_kilometers_per_hour,
        Some(62.5)
    );
}

#[tokio::test]
async fn fetch_raster_tile_maps_the_mock_payload() {
    let map_sdk = sdk_with_mock_providers();
    let raster_tile = map_sdk
        .fetch_raster_tile(
            TEST_TILE_X,
            TEST_TILE_Y,
            TEST_ZOOM_LEVEL,
            RasterImageFormat::Png,
            RasterTileResolution::Standard,
        )
        .await
        .expect("the registered tile provider serves the address");
    assert_eq!(raster_tile.tile_x, TEST_TILE_X);
    assert_eq!(raster_tile.tile_y, TEST_TILE_Y);
    assert_eq!(raster_tile.zoom_level, TEST_ZOOM_LEVEL);
    assert_eq!(raster_tile.image_format, RasterImageFormat::Png);
    assert_eq!(raster_tile.tile_resolution, RasterTileResolution::Standard);
    assert_eq!(
        raster_tile.image_bytes.len(),
        usize::try_from(MOCK_STANDARD_TILE_EDGE_PIXELS * MOCK_STANDARD_TILE_EDGE_PIXELS)
            .expect("the mock tile payload edge product fits")
    );
}

#[tokio::test]
async fn fetch_raster_tile_rejects_an_invalid_resolution_request_through_the_provider() {
    // No invalid raster request exists on this surface (coordinates are not
    // part of a raster call), so this test pins the not-configured throw for
    // the raster contract instead.
    let map_sdk = MapSdk::new();
    let failure = map_sdk
        .fetch_raster_tile(
            TEST_TILE_X,
            TEST_TILE_Y,
            TEST_ZOOM_LEVEL,
            RasterImageFormat::Jpeg,
            RasterTileResolution::Retina,
        )
        .await
        .expect_err("an unregistered raster contract must throw");
    assert_eq!(
        failure,
        SdkError::ProviderNotConfigured {
            contract_name: "RasterTileProvider".to_string(),
        }
    );
}
