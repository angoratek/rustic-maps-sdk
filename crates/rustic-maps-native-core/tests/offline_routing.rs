//! End-to-end tests of the contract's offline routing member: the committed
//! compiled `RMOG` fixture routes between known grid coordinates, and bytes
//! that fail archive validation are refused typed. Network-free: the fixture
//! is checked in and read from disk.
//!
//! The tests await through a Tokio executor (dev-dependency only); the
//! exported future itself is polled by the foreign event loop and needs no
//! Rust executor.

use rustic_maps::{GeographicCoordinate, SdkError, offline_route_from_archive_bytes};

/// The committed compiled fixture archive (relative to this crate's manifest
/// directory): the routing-grid extract compiled once by the deterministic
/// compiler, guarded by the compiler crate's byte-equality test.
const COMPILED_ARCHIVE_FIXTURE_PATH: &str =
    "../rustic-maps-offline-graph-compiler/fixtures/rmog/routing_grid.rmog";

/// Grid spacing of the fixture's residential segments, in meters (the same
/// fact the compiler crate's round-trip test and the rustic-maps-core fixture tests
/// pin).
const GRID_SEGMENT_METERS: f64 = 111.0;

/// Start of the routed oneway-street pair: fixture node 1004.
const ONEWAY_STREET_START: GeographicCoordinate = GeographicCoordinate {
    latitude: 0.001,
    longitude: 0.0,
};

/// End of the routed oneway-street pair: fixture node 1006.
const ONEWAY_STREET_END: GeographicCoordinate = GeographicCoordinate {
    latitude: 0.001,
    longitude: 0.002,
};

/// Bytes that are not an `RMOG` archive at all: the reader must reject them
/// before any graph is considered loaded.
const NOT_AN_ARCHIVE_BYTES: &[u8] = b"not an RMOG archive";

fn committed_archive_bytes() -> Vec<u8> {
    let fixture_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(COMPILED_ARCHIVE_FIXTURE_PATH);
    std::fs::read(&fixture_path).expect("the committed compiled archive fixture is checked in")
}

#[tokio::test]
async fn committed_archive_fixture_routes_between_known_coordinates() {
    // Fixture node 1004 -> 1006 is the oneway street's stored direction
    // (east): two axis-aligned residential runs, so the route is departure
    // to arrival with no derived turn.
    let route = offline_route_from_archive_bytes(
        committed_archive_bytes(),
        ONEWAY_STREET_START,
        ONEWAY_STREET_END,
    )
    .await
    .expect("the committed fixture archive routes between its grid coordinates");

    assert_eq!(route.distance_meters, 2.0 * GRID_SEGMENT_METERS);
    assert_eq!(route.legs.len(), 1);
    let steps = &route.legs[0].steps;
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0].maneuver_type, "departure");
    assert_eq!(steps[1].maneuver_type, "arrival");
}

#[tokio::test]
async fn garbage_archive_bytes_are_refused_typed() {
    let refusal = offline_route_from_archive_bytes(
        NOT_AN_ARCHIVE_BYTES.to_vec(),
        ONEWAY_STREET_START,
        ONEWAY_STREET_END,
    )
    .await
    .expect_err("bytes that are not an RMOG archive are refused");

    assert!(
        matches!(refusal, SdkError::OfflineRoutingFailure { .. }),
        "expected OfflineRoutingFailure, got {refusal:?}"
    );
}
