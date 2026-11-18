//! Milestone M7 slice 2 verify gate: the
//! [`ArchiveOfflineRoutingProvider`] registered as the registry's
//! offline-routing provider must load the committed routing-grid fixture
//! archive and answer honest routes over it — distance and duration strictly
//! derived from the archive's edge records. Network-free by construction.

use geospatial_traits::OfflineGraphSource;
use geospatial_types::route::{RouteOptions, RouteStrategy, TransportationMode, UnitSystem};
use geospatial_types::{GeographicCoordinate, OfflineGraphInvalidReason, SDKError};
use rustic_maps_core::offline_routing::ArchiveOfflineRoutingProvider;
use rustic_maps_core::registry::ProviderRegistry;
use rustic_maps_offline_graph_archive::archive::OfflineRoutingNetworkGraph;
use rustic_maps_offline_graph_compiler::compiler::compile_osm_extract_bytes;
use std::sync::Arc;

/// The committed routing-grid fixture extract (relative to the compiler
/// crate's manifest directory; this test compiles it in-process).
const COMMITTED_FIXTURE_PATH: &str =
    "../rustic-maps-offline-graph-compiler/fixtures/osm/routing_grid.osm.pbf";

/// Grid spacing of the fixture (see the compiler crate's round-trip test).
const GRID_SEGMENT_METERS: f64 = 111.0;
/// `GRID_SEGMENT_METERS` at the quantized 30 km/h residential speed.
const RESIDENTIAL_SEGMENT_DURATION_SECONDS: f64 = 13.32;

fn coordinate(latitude_degrees: f64, longitude_degrees: f64) -> GeographicCoordinate {
    GeographicCoordinate::from_degrees(latitude_degrees, longitude_degrees)
        .expect("test coordinates are within the canonical ranges")
}

fn fastest_options() -> RouteOptions {
    RouteOptions {
        transportation_mode: TransportationMode::Automobile,
        route_strategy: RouteStrategy::Fastest,
        unit_system: UnitSystem::Metric,
        avoid_options: geospatial_types::route::AvoidOptions::default(),
        departure_timestamp_seconds: None,
    }
}

/// Compiles the committed fixture extract into archive bytes.
fn fixture_archive_bytes() -> Vec<u8> {
    let fixture_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(COMMITTED_FIXTURE_PATH)
        .canonicalize()
        .expect("the committed fixture extract is checked in");
    compile_osm_extract_bytes(&std::fs::read(fixture_path).expect("the fixture is readable"))
        .expect("the committed fixture extract compiles")
}

/// Distinct temp-file name; no tempfile dependency at test time.
fn unique_temporary_archive_path() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static PATH_COUNTER: AtomicUsize = AtomicUsize::new(0);
    let unique_counter = PATH_COUNTER.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "rustic-maps-offline-registry-test-{}-{unique_counter}.rmog",
        std::process::id()
    ))
}

#[tokio::test]
async fn registry_resolves_the_offline_provider_for_fixture_routes() {
    let mut registry = ProviderRegistry::new();
    registry.register_offline_routing_provider(Arc::new(ArchiveOfflineRoutingProvider::new()));
    let provider = registry
        .require_offline_routing_provider()
        .expect("the offline routing provider is registered");

    // Start and end on the fixture's oneway street (archive nodes 2 and 4:
    // (0.001, 0.000) → (0.001, 0.002)) — a two-segment residential route.
    let route = {
        provider
            .load_offline_graph(OfflineGraphSource::Memory(fixture_archive_bytes()))
            .await
            .expect("the committed fixture compiles into a valid archive");
        provider
            .calculate_offline_route(
                &coordinate(0.001, 0.000),
                &coordinate(0.001, 0.002),
                &fastest_options(),
            )
            .await
            .expect("the oneway street connects the request points")
    };

    let total_distance = 2.0 * GRID_SEGMENT_METERS;
    let total_duration = 2.0 * RESIDENTIAL_SEGMENT_DURATION_SECONDS;
    assert_eq!(route.distance_meters, total_distance);
    assert_eq!(route.duration_seconds, total_duration);
    let only_leg = route.legs.first().expect("route data has one leg");
    assert_eq!(only_leg.distance_meters, total_distance);
    assert_eq!(only_leg.duration_seconds, total_duration);
    assert_eq!(only_leg.steps.len(), 2);
    assert_eq!(only_leg.steps[1].shape.len(), 1);
}

#[tokio::test]
async fn file_path_loads_route_through_the_memory_mapped_seam() {
    let archive_path = unique_temporary_archive_path();
    std::fs::write(&archive_path, fixture_archive_bytes())
        .expect("the test archive writes to the temp directory");
    struct TempFileGuard(std::path::PathBuf);
    impl Drop for TempFileGuard {
        fn drop(&mut self) {
            std::fs::remove_file(&self.0).ok();
        }
    }
    let _temp_file_guard = TempFileGuard(archive_path.clone());

    let mut registry = ProviderRegistry::new();
    registry.register_offline_routing_provider(Arc::new(ArchiveOfflineRoutingProvider::new()));
    let provider = registry
        .require_offline_routing_provider()
        .expect("the offline routing provider is registered");

    provider
        .load_offline_graph(OfflineGraphSource::FilePath(
            archive_path.to_string_lossy().to_string(),
        ))
        .await
        .expect("the mapped fixture archive validates");
    let route = provider
        .calculate_offline_route(
            &coordinate(0.001, 0.000),
            &coordinate(0.001, 0.002),
            &fastest_options(),
        )
        .await
        .expect("the fixture graph answers the route");
    assert_eq!(route.distance_meters, 2.0 * GRID_SEGMENT_METERS);
}

#[tokio::test]
async fn calculated_routes_report_rejected_requests_honestly() {
    let mut registry = ProviderRegistry::new();
    registry.register_offline_routing_provider(Arc::new(ArchiveOfflineRoutingProvider::new()));
    let provider = registry
        .require_offline_routing_provider()
        .expect("the offline routing provider is registered");

    // No graph loaded yet: routes are refused instead of guessed.
    let unloaded_route = provider
        .calculate_offline_route(
            &coordinate(0.001, 0.000),
            &coordinate(0.001, 0.002),
            &fastest_options(),
        )
        .await;
    assert!(matches!(
        unloaded_route,
        Err(SDKError::ProviderRequestRejected(_))
    ));

    provider
        .load_offline_graph(OfflineGraphSource::Memory(fixture_archive_bytes()))
        .await
        .expect("the committed fixture compiles into a valid archive");

    // Automobile-only profile: a pedestrian request is refused.
    let pedestrian_options = RouteOptions {
        transportation_mode: TransportationMode::Pedestrian,
        ..fastest_options()
    };
    let pedestrian_route = provider
        .calculate_offline_route(
            &coordinate(0.001, 0.000),
            &coordinate(0.001, 0.002),
            &pedestrian_options,
        )
        .await;
    assert!(matches!(
        pedestrian_route,
        Err(SDKError::ProviderRequestRejected(_))
    ));

    // A corrupt reload fails typed and leaves the loaded graph answering.
    let mut corrupted_bytes = fixture_archive_bytes();
    corrupted_bytes[4] = 0x2A;
    let failed_reload = provider
        .load_offline_graph(OfflineGraphSource::Memory(corrupted_bytes))
        .await
        .expect_err("the corrupted reload fails typed");
    assert_eq!(
        failed_reload,
        SDKError::OfflineRoutingFailure(OfflineGraphInvalidReason::VersionUnsupported)
    );
    let still_answered_route = provider
        .calculate_offline_route(
            &coordinate(0.001, 0.000),
            &coordinate(0.001, 0.002),
            &fastest_options(),
        )
        .await
        .expect("the loaded graph survives the failed reload");
    assert_eq!(
        still_answered_route.distance_meters,
        2.0 * GRID_SEGMENT_METERS
    );
}

/// The compiled fixture must not silently disagree with the committed
/// round-trip tests (archive-level expectations live in the compiler crate;
/// here only the route-relevant sanity facts are re-asserted cheaply).
#[test]
fn fixture_archive_matches_the_routed_expectations() {
    let network_graph = OfflineRoutingNetworkGraph::from_bytes(&fixture_archive_bytes())
        .expect("the committed fixture compiles into a valid archive");
    assert_eq!(network_graph.nodes().len(), 6);
    assert_eq!(network_graph.edges().len(), 8);
}
