//! End-to-end calls for the provider contracts the native suite did not yet
//! bridge: the keyless routing engines, Photon's search and reverse
//! geocoding, the full-service provider's place search, distance matrix and
//! raster tiles, and the provider-key readback.
//!
//! Every call is answered by the shared loopback fixture server from a
//! committed payload, so no test reaches the network. The keyless providers
//! take their deployment address in the credential slot, which lets each case
//! point the provider the SDK builds at the fixture server through
//! `MapSdk.configure_provider`. The credentialed full-service provider keeps
//! its key in the credential slot and takes its origin from
//! `MapSdk.set_provider_base_url`, so its cases also run entirely through
//! `configure_provider`; the key readback is proven separately.
//!
//! Each case proves the seam the SDK owns: the provider is constructed, the
//! call crosses the SDK bridge, and the parsed record carries the anchor
//! value the fixture was recorded for.

mod common;

use common::{LoopbackFixtureServer, ScriptedResponse};
use rustic_maps::{
    AvoidOptions, GeographicCoordinate, MapSdk, ProviderContract, RasterImageFormat,
    RasterTileResolution, RouteOptions, RouteStrategy, SdkError, TransportationMode, UnitSystem,
};

/// HTTP status the scripted server returns for a payload.
const HTTP_OK_STATUS: u16 = 200;

/// The media type a raster-tile response carries; the providers read the
/// bytes.
const TILE_CONTENT_TYPE: &str = "application/octet-stream";

/// The placeholder credential a credentialed provider registers with. It is
/// never sent: the key-readback case makes no request.
const PLACEHOLDER_CREDENTIAL: &str = "placeholder-key";

/// Serializes the cases that configure the process-wide base URL store, so one
/// case's loopback origin cannot reach another case's configuration.
static BASE_URL_OVERRIDE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A valid deployment address used by the key-readback case. Nothing listens
/// on it and no request is made.
const PLACEHOLDER_DEPLOYMENT_ADDRESS: &str = "http://127.0.0.1:1";

/// The capabilities the full-service provider registers.
const MAPQUEST_SERVED_CONTRACTS: &[ProviderContract] = &[
    ProviderContract::Geocoding,
    ProviderContract::Routing,
    ProviderContract::DistanceMatrix,
    ProviderContract::PlaceSearch,
    ProviderContract::TrafficIncidents,
    ProviderContract::TrafficFlow,
    ProviderContract::RasterTiles,
];
/// The capability OSRM registers.
const OSRM_SERVED_CONTRACTS: &[ProviderContract] = &[ProviderContract::Routing];
/// The capabilities Photon registers.
const PHOTON_SERVED_CONTRACTS: &[ProviderContract] =
    &[ProviderContract::Geocoding, ProviderContract::PlaceSearch];
/// The capability Valhalla registers.
const VALHALLA_SERVED_CONTRACTS: &[ProviderContract] = &[ProviderContract::Routing];

// ---------------------------------------------------------------------------
// Routing anchors: the totals the committed recordings carry.
// ---------------------------------------------------------------------------

/// The distance the OSRM recording reports, in meters.
const OSRM_RECORDED_DISTANCE_METERS: f64 = 1_302.3;
/// The distance the Valhalla recording reports once its kilometers are
/// normalized to meters.
const VALHALLA_RECORDED_DISTANCE_METERS: f64 = 3_200.0;
/// Tolerance for a recorded distance, in meters.
const RECORDED_DISTANCE_TOLERANCE_METERS: f64 = 0.5;

/// The Sacramento endpoints the OSRM recording was captured for.
const OSRM_ROUTE_START: (f64, f64) = (38.57657, -121.4936);
const OSRM_ROUTE_END: (f64, f64) = (38.5781, -121.50555);
/// The Sacramento endpoints the Valhalla recording was captured for.
const VALHALLA_ROUTE_START: (f64, f64) = (38.57657, -121.4936);
const VALHALLA_ROUTE_END: (f64, f64) = (38.57887, -121.50797);

// ---------------------------------------------------------------------------
// Search anchors: the first placeable record each committed payload carries.
// ---------------------------------------------------------------------------

/// The display line Photon's first Sacramento feature carries.
const PHOTON_FIRST_PLACE_TEXT: &str = "Capitol Mall";
/// The position Photon's first Sacramento feature carries.
const PHOTON_FIRST_PLACE_POSITION: (f64, f64) = (38.579, -121.493);

/// The display line MapQuest's first placeable Denver prediction carries.
const MAPQUEST_FIRST_PLACE_TEXT: &str = "Denver Int'l Airport";
/// The position MapQuest's first placeable Denver prediction carries.
const MAPQUEST_FIRST_PLACE_POSITION: (f64, f64) = (39.85352, -104.67377);

/// Tolerance for a recorded place position, in degrees, matching the
/// coordinate rounding the provider layer applies.
const RECORDED_POSITION_TOLERANCE_DEGREES: f64 = 0.001;

// ---------------------------------------------------------------------------
// Distance-matrix anchors: the first origin-destination cell the recording
// carries once its kilometers are converted to meters.
// ---------------------------------------------------------------------------

/// The distance the recorded matrix's first measured cell reports, in meters.
const MAPQUEST_MATRIX_FIRST_DISTANCE_METERS: f64 = 1_700.0;
/// The duration the recorded matrix's first measured cell reports, in
/// seconds.
const MAPQUEST_MATRIX_FIRST_DURATION_SECONDS: f64 = 235.0;
/// Tolerance for a recorded matrix distance, in meters.
const RECORDED_MATRIX_DISTANCE_TOLERANCE_METERS: f64 = 0.5;
/// Tolerance for a recorded matrix duration, in seconds.
const RECORDED_MATRIX_DURATION_TOLERANCE_SECONDS: f64 = 0.5;

// ---------------------------------------------------------------------------
// Raster-tile anchors: the committed rendering tile MapQuest's request
// serves.
// ---------------------------------------------------------------------------

/// The recorded PNG the raster request serves.
const RASTER_TILE_FIXTURE: &str =
    "crates/rustic-maps-rendering/fixtures/render/tiles/13_1331_3143.png";
/// The tile address the recorded PNG carries.
const RASTER_TILE_X: u32 = 1331;
/// The tile row the recorded PNG carries.
const RASTER_TILE_Y: u32 = 3143;
/// The zoom the recorded PNG was rendered at.
const RASTER_TILE_ZOOM_LEVEL: u8 = 13;

/// A server answering every request with one committed JSON payload.
fn fixture_server(payload: Vec<u8>) -> LoopbackFixtureServer {
    LoopbackFixtureServer::start(vec![ScriptedResponse::json(HTTP_OK_STATUS, payload)])
}

/// A server answering every request with one committed tile payload.
fn tile_fixture_server(payload: Vec<u8>) -> LoopbackFixtureServer {
    LoopbackFixtureServer::start(vec![ScriptedResponse::with_content_type(
        HTTP_OK_STATUS,
        TILE_CONTENT_TYPE,
        payload,
    )])
}

/// Reads one committed fixture, keyed by provider.
fn provider_fixture_bytes(provider_key: &str, file_name: &str) -> Vec<u8> {
    common::fixture_bytes(&format!("fixtures/{provider_key}/{file_name}"))
}

/// Builds the route options every routing case asks with.
fn route_options() -> RouteOptions {
    RouteOptions {
        transportation_mode: TransportationMode::Automobile,
        route_strategy: RouteStrategy::Fastest,
        unit_system: UnitSystem::Metric,
        avoid_options: AvoidOptions {
            toll_roads: false,
            limited_access_highways: false,
            ferries: false,
            unpaved_roads: false,
        },
        departure_timestamp_seconds: None,
    }
}

/// A coordinate built from the degrees a fixture was recorded for.
fn recorded_coordinate((latitude, longitude): (f64, f64)) -> GeographicCoordinate {
    common::coordinate(latitude, longitude)
}

/// Asserts that the readback names `provider_key` for `contract`.
fn assert_registration_names(map_sdk: &MapSdk, contract: ProviderContract, provider_key: &str) {
    let registration = map_sdk
        .provider_registrations()
        .into_iter()
        .find(|registration| registration.contract == contract)
        .unwrap_or_else(|| panic!("{provider_key} reports the {contract:?} capability"));
    assert!(
        registration.registered,
        "{provider_key} reports its {contract:?} capability as installed"
    );
    assert_eq!(
        registration.provider_key, provider_key,
        "{provider_key} names the key that was configured for {contract:?}"
    );
    assert!(
        !registration.display_name.is_empty(),
        "{provider_key} reports its display name for {contract:?}"
    );
}

/// Asserts that a parsed place carries the recorded anchor, naming only the
/// provider in a failure message.
///
/// The position is position data the code-scanning cleartext-logging rule
/// keeps out of logs, so the message never prints it.
fn assert_recorded_place(
    places: &[rustic_maps::Place],
    provider_key: &str,
    expected_text: &str,
    expected_position: (f64, f64),
) {
    let first = places
        .first()
        .unwrap_or_else(|| panic!("{provider_key} answers one place"));
    assert!(
        first.structured_address_text.contains(expected_text),
        "{provider_key} reports the fixture place text"
    );
    let position_matches = (first.coordinate.latitude - expected_position.0).abs()
        < RECORDED_POSITION_TOLERANCE_DEGREES
        && (first.coordinate.longitude - expected_position.1).abs()
            < RECORDED_POSITION_TOLERANCE_DEGREES;
    assert!(
        position_matches,
        "{provider_key} reports the fixture place position"
    );
}

// ---------------------------------------------------------------------------
// The keyless routing engines: configure_provider takes the deployment
// address, so the whole call runs through the SDK.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn osrm_routing_crosses_the_sdk_bridge() {
    let server = fixture_server(provider_fixture_bytes("osrm", "sacramento_route.json"));
    let map_sdk = MapSdk::new();
    map_sdk
        .configure_provider(
            ProviderContract::Routing,
            "osrm".to_string(),
            server.base_url().to_string(),
        )
        .expect("the keyless routing engine registers with the loopback address");
    assert_registration_names(&map_sdk, ProviderContract::Routing, "osrm");

    let route = map_sdk
        .calculate_route(
            vec![
                recorded_coordinate(OSRM_ROUTE_START),
                recorded_coordinate(OSRM_ROUTE_END),
            ],
            route_options(),
        )
        .await
        .expect("the vendor provider answers the committed payload");
    assert!(
        (route.distance_meters - OSRM_RECORDED_DISTANCE_METERS).abs()
            < RECORDED_DISTANCE_TOLERANCE_METERS,
        "OSRM reports the fixture distance"
    );
    assert!(
        !route.geometry.is_empty(),
        "OSRM's recorded shape decoded into geometry"
    );
    assert_eq!(route.legs.len(), 1, "the fixture leg maps into one leg");
    assert_eq!(server.request_count(), 1);
    assert!(
        server.unmatched_requests().is_empty(),
        "the request answers the scripted path: {:?}",
        server.unmatched_requests()
    );
}

#[tokio::test]
async fn valhalla_routing_crosses_the_sdk_bridge() {
    let server = fixture_server(provider_fixture_bytes("valhalla", "route_sacramento.json"));
    let map_sdk = MapSdk::new();
    map_sdk
        .configure_provider(
            ProviderContract::Routing,
            "valhalla".to_string(),
            server.base_url().to_string(),
        )
        .expect("the keyless routing engine registers with the loopback address");
    assert_registration_names(&map_sdk, ProviderContract::Routing, "valhalla");

    let route = map_sdk
        .calculate_route(
            vec![
                recorded_coordinate(VALHALLA_ROUTE_START),
                recorded_coordinate(VALHALLA_ROUTE_END),
            ],
            route_options(),
        )
        .await
        .expect("the vendor provider answers the committed payload");
    assert!(
        (route.distance_meters - VALHALLA_RECORDED_DISTANCE_METERS).abs()
            < RECORDED_DISTANCE_TOLERANCE_METERS,
        "Valhalla reports the fixture distance normalized to meters"
    );
    assert!(
        !route.geometry.is_empty(),
        "Valhalla's recorded shape decoded into geometry"
    );
    assert_eq!(route.legs.len(), 1, "the fixture leg maps into one leg");
    assert_eq!(server.request_count(), 1);
    assert!(
        server.unmatched_requests().is_empty(),
        "the request answers the scripted path: {:?}",
        server.unmatched_requests()
    );
}

// ---------------------------------------------------------------------------
// Photon: one keyless deployment serves forward geocoding, settled place
// search and reverse geocoding.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn photon_forward_geocoding_crosses_the_sdk_bridge() {
    let server = fixture_server(provider_fixture_bytes("photon", "search_sacramento.json"));
    let map_sdk = MapSdk::new();
    map_sdk
        .configure_provider(
            ProviderContract::Geocoding,
            "photon".to_string(),
            server.base_url().to_string(),
        )
        .expect("the keyless geocoder registers with the loopback address");
    assert_registration_names(&map_sdk, ProviderContract::Geocoding, "photon");

    let places = map_sdk
        .forward_geocode("Capitol Mall, Sacramento".to_string())
        .await
        .expect("the vendor provider answers the committed payload");
    assert_recorded_place(
        &places,
        "photon",
        PHOTON_FIRST_PLACE_TEXT,
        PHOTON_FIRST_PLACE_POSITION,
    );
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn photon_place_search_crosses_the_sdk_bridge() {
    let server = fixture_server(provider_fixture_bytes("photon", "search_sacramento.json"));
    let map_sdk = MapSdk::new();
    map_sdk
        .configure_provider(
            ProviderContract::PlaceSearch,
            "photon".to_string(),
            server.base_url().to_string(),
        )
        .expect("the keyless search provider registers with the loopback address");
    assert_registration_names(&map_sdk, ProviderContract::PlaceSearch, "photon");

    let places = map_sdk
        .search("Capitol Mall, Sacramento".to_string(), None)
        .await
        .expect("the vendor provider answers the committed payload");
    assert_recorded_place(
        &places,
        "photon",
        PHOTON_FIRST_PLACE_TEXT,
        PHOTON_FIRST_PLACE_POSITION,
    );
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn photon_reverse_geocoding_crosses_the_sdk_bridge() {
    let server = fixture_server(provider_fixture_bytes("photon", "reverse_sacramento.json"));
    let map_sdk = MapSdk::new();
    map_sdk
        .configure_provider(
            ProviderContract::Geocoding,
            "photon".to_string(),
            server.base_url().to_string(),
        )
        .expect("the keyless geocoder registers with the loopback address");
    assert_registration_names(&map_sdk, ProviderContract::Geocoding, "photon");

    let places = map_sdk
        .reverse_geocode(recorded_coordinate(PHOTON_FIRST_PLACE_POSITION))
        .await
        .expect("the vendor provider answers the committed payload");
    assert_recorded_place(
        &places,
        "photon",
        PHOTON_FIRST_PLACE_TEXT,
        PHOTON_FIRST_PLACE_POSITION,
    );
    assert_eq!(server.request_count(), 1);
}

// ---------------------------------------------------------------------------
// The full-service provider's remaining contracts. The key stays in the
// credential slot and the origin comes from the base URL seam, so every case
// registers through the catalog exactly as an application does.
// ---------------------------------------------------------------------------

/// Points the full-service provider at the fixture server and registers one of
/// its capabilities through `configure_provider`.
///
/// The base URL test lock is held only through configuration: the client
/// captures its origin when it is constructed, so the request that follows
/// cannot pick up another case's override.
fn configure_mapquest_at_loopback(
    map_sdk: &MapSdk,
    server: &LoopbackFixtureServer,
    contract: ProviderContract,
) {
    let _test_lock = BASE_URL_OVERRIDE_TEST_LOCK
        .lock()
        .expect("the base URL store test lock is healthy");
    map_sdk
        .set_provider_base_url("mapquest".to_string(), Some(server.base_url().to_string()))
        .expect("the loopback origin is a valid base URL");
    map_sdk
        .configure_provider(
            contract,
            "mapquest".to_string(),
            PLACEHOLDER_CREDENTIAL.to_string(),
        )
        .expect("the credentialed provider registers against the loopback origin");
}

#[tokio::test]
async fn mapquest_place_search_crosses_the_sdk_bridge() {
    let server = fixture_server(provider_fixture_bytes(
        "mapquest",
        "search_prediction_denver.json",
    ));
    let map_sdk = MapSdk::new();
    configure_mapquest_at_loopback(&map_sdk, &server, ProviderContract::PlaceSearch);
    assert_registration_names(&map_sdk, ProviderContract::PlaceSearch, "mapquest");

    let places = map_sdk
        .search("Denver".to_string(), None)
        .await
        .expect("the vendor provider answers the committed payload");
    assert_recorded_place(
        &places,
        "mapquest",
        MAPQUEST_FIRST_PLACE_TEXT,
        MAPQUEST_FIRST_PLACE_POSITION,
    );
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn mapquest_distance_matrix_crosses_the_sdk_bridge() {
    let server = fixture_server(provider_fixture_bytes(
        "mapquest",
        "routematrix_three_locations_unit_k.json",
    ));
    let map_sdk = MapSdk::new();
    configure_mapquest_at_loopback(&map_sdk, &server, ProviderContract::DistanceMatrix);
    assert_registration_names(&map_sdk, ProviderContract::DistanceMatrix, "mapquest");

    // The recording holds three locations; one origin and two destinations
    // keep the requested pair set inside it.
    let distance_matrix = map_sdk
        .calculate_distance_matrix(
            vec![recorded_coordinate((38.57657, -121.4936))],
            vec![
                recorded_coordinate((38.5781, -121.50555)),
                recorded_coordinate((38.57887, -121.50797)),
            ],
            TransportationMode::Automobile,
        )
        .await
        .expect("the vendor provider answers the committed payload");
    let first_cell = distance_matrix
        .entries
        .first()
        .unwrap_or_else(|| panic!("mapquest answers the first measured cell"));
    assert_eq!(first_cell.origin_index, 0);
    assert_eq!(first_cell.destination_index, 0);
    assert!(
        (first_cell.distance_meters - MAPQUEST_MATRIX_FIRST_DISTANCE_METERS).abs()
            < RECORDED_MATRIX_DISTANCE_TOLERANCE_METERS,
        "mapquest reports the fixture cell distance converted to meters"
    );
    assert!(
        (first_cell.duration_seconds - MAPQUEST_MATRIX_FIRST_DURATION_SECONDS).abs()
            < RECORDED_MATRIX_DURATION_TOLERANCE_SECONDS,
        "mapquest reports the fixture cell duration"
    );
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn mapquest_raster_tiles_carry_the_recorded_png() {
    let recorded_png = common::fixture_bytes(RASTER_TILE_FIXTURE);
    let server = tile_fixture_server(recorded_png.clone());
    let map_sdk = MapSdk::new();
    configure_mapquest_at_loopback(&map_sdk, &server, ProviderContract::RasterTiles);
    assert_registration_names(&map_sdk, ProviderContract::RasterTiles, "mapquest");

    let tile = map_sdk
        .fetch_raster_tile(
            RASTER_TILE_X,
            RASTER_TILE_Y,
            RASTER_TILE_ZOOM_LEVEL,
            RasterImageFormat::Png,
            RasterTileResolution::Standard,
        )
        .await
        .expect("the vendor provider answers the committed tile");
    assert_eq!(
        tile.image_bytes, recorded_png,
        "the recorded tile's bytes survive the bridge"
    );
    assert_eq!(tile.tile_x, RASTER_TILE_X);
    assert_eq!(tile.tile_y, RASTER_TILE_Y);
    assert_eq!(tile.zoom_level, RASTER_TILE_ZOOM_LEVEL);
    assert_eq!(tile.image_format, RasterImageFormat::Png);
    assert_eq!(tile.tile_resolution, RasterTileResolution::Standard);
    assert_eq!(server.request_count(), 1);
}

// ---------------------------------------------------------------------------
// The base URL seam refuses an address that would smuggle a credential: the
// detail names the reason and never echoes the value.
// ---------------------------------------------------------------------------

#[test]
fn set_provider_base_url_refuses_an_address_carrying_credentials() {
    const EMBEDDED_CREDENTIAL: &str = "embedded-secret";
    let map_sdk = MapSdk::new();
    let _test_lock = BASE_URL_OVERRIDE_TEST_LOCK
        .lock()
        .expect("the base URL store test lock is healthy");
    for rejected_address in [
        format!("https://consumer:{EMBEDDED_CREDENTIAL}@example.test"),
        format!("https://example.test/?key={EMBEDDED_CREDENTIAL}"),
        format!("https://example.test/#{EMBEDDED_CREDENTIAL}"),
    ] {
        match map_sdk.set_provider_base_url("mapquest".to_string(), Some(rejected_address)) {
            Err(SdkError::ProviderRequestRejected { detail }) => {
                assert!(
                    !detail.is_empty(),
                    "the refusal names the reason it rejected the address"
                );
                assert!(
                    !detail.contains(EMBEDDED_CREDENTIAL),
                    "the refusal never echoes the value it refused"
                );
            }
            other => panic!("the address carrying credentials is refused: {other:?}"),
        }
    }
}

// ---------------------------------------------------------------------------
// The provider-key readback: every provider this suite covers names the key
// that was configured. Registration constructs the providers and makes no
// request, so no fixture server is needed.
// ---------------------------------------------------------------------------

#[test]
fn the_readback_names_each_configured_provider_key() {
    for (provider_key, contracts, credential) in [
        (
            "mapquest",
            MAPQUEST_SERVED_CONTRACTS,
            PLACEHOLDER_CREDENTIAL,
        ),
        (
            "osrm",
            OSRM_SERVED_CONTRACTS,
            PLACEHOLDER_DEPLOYMENT_ADDRESS,
        ),
        (
            "photon",
            PHOTON_SERVED_CONTRACTS,
            PLACEHOLDER_DEPLOYMENT_ADDRESS,
        ),
        (
            "valhalla",
            VALHALLA_SERVED_CONTRACTS,
            PLACEHOLDER_DEPLOYMENT_ADDRESS,
        ),
    ] {
        let map_sdk = MapSdk::new();
        for contract in contracts {
            map_sdk
                .configure_provider(*contract, provider_key.to_string(), credential.to_string())
                .unwrap_or_else(|error| panic!("{provider_key} registers {contract:?}: {error:?}"));
            assert_registration_names(&map_sdk, *contract, provider_key);
        }
    }
}
