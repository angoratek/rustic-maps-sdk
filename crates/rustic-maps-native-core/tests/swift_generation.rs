//! Bindings-compilation test for the Swift surface: the frozen UDL contract
//! must generate Swift bindings in-process and keep the version probe —
//! `get_sdk_compiled_version()`'s last open platform coverage (the M4
//! checkbox "version everywhere"). Swift was previously verified only through
//! the packaging script (`scripts/build-ios.sh`); this test pins the
//! version probe, the camera object and the fielded error type the same
//! way the Kotlin generation test does.

use camino::Utf8PathBuf;

/// Marker of the namespace in the generated Swift bindings: the FFI module
/// the bindings import through `canImport`.
const EXPECTED_SWIFT_NAMESPACE_MARKER: &str = "import rustic_mapsFFI";
/// Marker of the version probe in the generated Swift bindings: the audit
/// under test — every platform surface must carry the compiled version.
const EXPECTED_SWIFT_VERSION_PROBE_MARKER: &str = "public func getSdkCompiledVersion() -> String";
/// Marker of the camera object in the generated Swift bindings.
const EXPECTED_SWIFT_CAMERA_MARKER: &str = "open class MapCamera: MapCameraProtocol";
/// Marker of the fielded error type in the generated Swift bindings.
const EXPECTED_SWIFT_ERROR_ENUM_MARKER: &str = "enum SdkError: Swift.Error";
/// Marker of the fielded error variant in the generated Swift bindings.
const EXPECTED_SWIFT_ERROR_VARIANT_MARKER: &str = "case InvalidCoordinateInput(detail: String";
/// Marker of the round-8 surface-render input error variant in the generated
/// Swift bindings.
const EXPECTED_SWIFT_RENDER_INPUT_ERROR_MARKER: &str = "case InvalidRenderInput(detail: String";
/// Marker of the round-8 surface-unavailable error variant in the generated
/// Swift bindings.
const EXPECTED_SWIFT_SURFACE_UNAVAILABLE_ERROR_MARKER: &str =
    "case RenderingSurfaceUnavailable(detail: String";
/// Marker of the surface view object in the generated Swift bindings.
const EXPECTED_SWIFT_SURFACE_VIEW_MARKER: &str = "open class MapView: MapViewProtocol";
/// Marker of the attach member in the generated Swift bindings: the handle
/// record plus the physical-pixel extent, throwable, not async.
const EXPECTED_SWIFT_ATTACH_MARKER: &str =
    "func attach(surfaceHandle: SurfaceHandle, widthPixels: UInt32, heightPixels: UInt32) throws";
/// Marker of the per-frame draw member in the generated Swift bindings: the
/// per-member `[Async]` lowers to `async` alongside `throws`.
const EXPECTED_SWIFT_DRAW_MARKER: &str = "func draw(camera: MapCamera) async throws";
/// Marker of the overlay-list replacement in the generated Swift bindings.
const EXPECTED_SWIFT_SET_OVERLAYS_MARKER: &str =
    "func setOverlays(overlayDescriptors: [MapOverlay]) throws";
/// Marker of the overlay descriptor record in the generated Swift bindings.
const EXPECTED_SWIFT_OVERLAY_DESCRIPTOR_MARKER: &str = "public struct MapOverlay";
/// Marker of the nullable `u64` platform pointer in the generated Swift
/// bindings: `u64?` lowers to `UInt64?`.
const EXPECTED_SWIFT_POINTER_MARKER: &str = "public var appkitViewPointer: UInt64?";
/// Marker of the platform one-call whole-provider-set registration in the
/// generated Swift bindings.
const EXPECTED_SWIFT_MAPQUEST_REGISTRATION_MARKER: &str =
    "func configureProviderSet(providerKey: String, credential: String) throws";
/// Marker of the round-9 tile-cache provider registration in the generated
/// Swift bindings: the byte budget crosses as `UInt64`.
const EXPECTED_SWIFT_TILE_CACHE_REGISTRATION_MARKER: &str = "func setProviderCredentialWithTileCache(providerKey: String, credential: String, tileCacheDatabasePath: String, tileCacheAbsoluteMaximumBytes: UInt64) throws";
/// Marker of the round-9 camera attribute getters in the generated Swift
/// bindings (plain methods: the UDL parser supports no `[Attribute]`
/// member).
const EXPECTED_SWIFT_CAMERA_GETTERS_MARKER: &str = "func zoomLevel()  -> Double";
/// Marker of the round-9 per-frame draw statistics return in the generated
/// Swift bindings.
const EXPECTED_SWIFT_DRAW_STATISTICS_MARKER: &str =
    "func draw(camera: MapCamera) async throws  -> FrameStatistics";
/// Marker of the M2 tile-cache settings record in the generated Swift
/// bindings.
const EXPECTED_SWIFT_TILE_CACHE_TERMS_MARKER: &str = "public struct TileCacheTerms";
/// Marker of the nullable provider-terms-ceiling field in the generated
/// Swift bindings: `u64?` lowers to `UInt64?`.
const EXPECTED_SWIFT_TILE_CACHE_CEILING_FIELD_MARKER: &str =
    "public var providerTermsCeilingSeconds: UInt64?";
/// Marker of the tile-cache time-to-live override member in the generated
/// Swift bindings: `u64` lowers to `UInt64`, throwing.
const EXPECTED_SWIFT_TILE_CACHE_SETTER_MARKER: &str =
    "func setRasterTileCacheTimeToLive(timeToLiveSeconds: UInt64) throws";
/// Marker of the effective-terms readback member in the generated Swift
/// bindings (a synchronous method returning the settings record).
const EXPECTED_SWIFT_TILE_CACHE_TERMS_READBACK_MARKER: &str =
    "func rasterTileCacheTerms() throws  -> TileCacheTerms";
/// Marker of the installed tile payload cache's statistics record in the
/// generated Swift bindings.
const EXPECTED_SWIFT_TILE_CACHE_STATISTICS_MARKER: &str = "public struct TileCacheStatistics";
/// Marker of the tile payload cache statistics readback in the generated
/// Swift bindings (an optional record return).
const EXPECTED_SWIFT_TILE_CACHE_STATISTICS_READBACK_MARKER: &str =
    "func tileCacheStatistics()  -> TileCacheStatistics?";
/// Marker of the round-11 two-grade deviation enum in the generated Swift
/// bindings.
const EXPECTED_SWIFT_DEVIATION_GRADE_KIND_MARKER: &str =
    "public enum DeviationGradeKind: Equatable, Hashable {";
/// Marker of the grade-kind field on the deviation record in the generated
/// Swift bindings (the addition-only UDL record addition).
const EXPECTED_SWIFT_DEVIATION_GRADE_FIELD_MARKER: &str =
    "public var gradeKind: DeviationGradeKind";
/// Marker of the round-11 suppressed distance-phrase kind in the generated
/// Swift bindings (the frozen countdown's explicit-empty phrase).
const EXPECTED_SWIFT_DISTANCE_PHRASE_SUPPRESSED_MARKER: &str = "case suppressed";
/// Marker of the round-11 reroute-recalculation policy record in the
/// generated Swift bindings (Ferrostar B3's hysteresis).
const EXPECTED_SWIFT_REROUTE_POLICY_MARKER: &str = "public struct ReroutePolicy";
/// Marker of the reroute-recalculation cooldown field in the generated Swift
/// bindings: the seconds threshold lowers as `Double`.
const EXPECTED_SWIFT_REROUTE_COOLDOWN_FIELD_MARKER: &str =
    "public var recalculationCooldownSeconds: Double";
/// Marker of the reroute-recalculation movement-threshold field in the
/// generated Swift bindings: the meters threshold lowers as `Double`.
const EXPECTED_SWIFT_REROUTE_MOVEMENT_FIELD_MARKER: &str =
    "public var recalculationMovementMeters: Double";
/// Marker of the reroute-recalculation begin member in the generated Swift
/// bindings (a synchronous gate probe returning the boolean answer).
const EXPECTED_SWIFT_REROUTE_BEGIN_MARKER: &str = "func beginRerouteCalculation()  -> Bool";
/// Marker of the reroute-cancellation member in the generated Swift bindings
/// (settle without a route, non-throwing).
const EXPECTED_SWIFT_REROUTE_CANCEL_MARKER: &str = "func cancelRerouteCalculation()";
/// Marker of the reroute-policy configuration member in the generated Swift
/// bindings, validating both thresholds through the thrown error.
const EXPECTED_SWIFT_REROUTE_POLICY_SETTER_MARKER: &str =
    "func configureReroutePolicy(reroutePolicy: ReroutePolicy) throws";
/// Marker of the reroute-policy defaults probe in the generated Swift
/// bindings.
const EXPECTED_SWIFT_REROUTE_POLICY_DEFAULT_MARKER: &str =
    "public func defaultReroutePolicy() -> ReroutePolicy";
/// Marker of the trip-state in-flight readback field in the generated Swift
/// bindings (the controller wall-clock gate fact).
const EXPECTED_SWIFT_IN_FLIGHT_FIELD_MARKER: &str = "public var isCalculatingNewRoute: Bool";
/// Marker of the round-11 speech-utterance identity field in the generated
/// Swift bindings (Ferrostar B4's monotonic id, `u64` lowering to `UInt64`).
const EXPECTED_SWIFT_UTTERANCE_ID_FIELD_MARKER: &str = "public var utteranceId: UInt64";
/// Marker of the round-12 provider-failure diagnostic readback in the
/// generated Swift bindings (a synchronous method returning the retained
/// diagnostics).
const EXPECTED_SWIFT_PROVIDER_FAILURE_DIAGNOSTICS_READBACK_MARKER: &str =
    "func latestProviderFailureDiagnostics()  -> [ProviderFailureDiagnostic]";
/// Marker of the round-12 provider-failure diagnostic record in the
/// generated Swift bindings.
const EXPECTED_SWIFT_PROVIDER_FAILURE_DIAGNOSTIC_MARKER: &str =
    "public struct ProviderFailureDiagnostic: Equatable, Hashable {";
/// Marker of the failure-classification field on the diagnostic record in
/// the generated Swift bindings.
const EXPECTED_SWIFT_FAILURE_CLASSIFICATION_FIELD_MARKER: &str =
    "public var failureClassification: ProviderFailureClassification";
/// Marker of the provider-failure classification enum in the generated
/// Swift bindings.
const EXPECTED_SWIFT_PROVIDER_FAILURE_CLASSIFICATION_MARKER: &str =
    "public enum ProviderFailureClassification: Equatable, Hashable {";
/// Marker of the unclassified-callback-failure variant in the generated
/// Swift bindings.
const EXPECTED_SWIFT_UNCLASSIFIED_VARIANT_MARKER: &str = "case unclassifiedCallbackFailure";
/// Marker of the M17 engine facades in the generated Swift bindings.
const EXPECTED_SWIFT_SEARCH_ENGINE_MARKER: &str = "open class SearchEngine: SearchEngineProtocol";
/// Marker of the routing engine facade in the generated Swift bindings.
const EXPECTED_SWIFT_ROUTING_ENGINE_MARKER: &str =
    "open class RoutingEngine: RoutingEngineProtocol";
/// Marker of the offline manager facade in the generated Swift bindings.
const EXPECTED_SWIFT_OFFLINE_MANAGER_MARKER: &str =
    "open class OfflineManager: OfflineManagerProtocol";
/// Marker of the offline archive-route function in the generated Swift
/// bindings: the UDL `bytes` argument lowers to `Data`, and the per-member
/// `[Async]` lowers to `async throws`.
const EXPECTED_SWIFT_OFFLINE_ROUTE_MARKER: &str = "public func offlineRouteFromArchiveBytes(archiveBytes: Data, start: GeographicCoordinate, end: GeographicCoordinate)async throws  -> Route";
/// Marker of the diagnostics facade in the generated Swift bindings.
const EXPECTED_SWIFT_DIAGNOSTICS_MARKER: &str = "open class Diagnostics: DiagnosticsProtocol";
/// Marker of the id-addressed overlay addition in the generated Swift
/// bindings: it answers the generated identifier string.
const EXPECTED_SWIFT_ADD_OVERLAY_MARKER: &str =
    "func addOverlay(overlay: MapOverlay) throws  -> String";
/// Marker of the id-addressed overlay update in the generated Swift
/// bindings.
const EXPECTED_SWIFT_UPDATE_OVERLAY_MARKER: &str =
    "func updateOverlay(overlayIdentifier: String, overlay: MapOverlay) throws";
/// Marker of the id-addressed overlay removal in the generated Swift
/// bindings: it answers whether the identifier was present.
const EXPECTED_SWIFT_REMOVE_OVERLAY_MARKER: &str =
    "func removeOverlay(overlayIdentifier: String)  -> Bool";
/// Marker of the four-topic listener subscription in the generated Swift
/// bindings.
const EXPECTED_SWIFT_ADD_NAVIGATION_LISTENER_MARKER: &str =
    "func addNavigationListener(listener: NavigationListener)  -> Subscription";
/// Marker of the navigation stop verb in the generated Swift bindings.
const EXPECTED_SWIFT_STOP_NAVIGATION_MARKER: &str = "func stopNavigation()";
/// Marker of the offline manager's terms readback in the generated Swift
/// bindings.
const EXPECTED_SWIFT_OFFLINE_TILE_CACHE_TERMS_MARKER: &str =
    "func tileCacheTerms() throws  -> TileCacheTerms";
/// Marker of the offline manager's time-to-live override in the generated
/// Swift bindings.
const EXPECTED_SWIFT_OFFLINE_TILE_TIME_TO_LIVE_MARKER: &str =
    "func setTileTimeToLive(timeToLiveSeconds: UInt64) throws";
/// Marker of the offline region definition record in the generated Swift
/// bindings.
const EXPECTED_SWIFT_REGION_DEFINITION_MARKER: &str =
    "public struct RegionDefinition: Equatable, Hashable {";
/// Marker of the stored offline region record in the generated Swift
/// bindings.
const EXPECTED_SWIFT_REGION_RECORD_MARKER: &str =
    "public struct RegionRecord: Equatable, Hashable {";
/// Marker of the region download progress record in the generated Swift
/// bindings.
const EXPECTED_SWIFT_REGION_DOWNLOAD_PROGRESS_MARKER: &str =
    "public struct RegionDownloadProgress: Equatable, Hashable {";
/// Marker of the region download phase enum in the generated Swift bindings.
const EXPECTED_SWIFT_REGION_DOWNLOAD_PHASE_MARKER: &str =
    "public enum RegionDownloadPhase: Equatable, Hashable {";
/// Marker of the region removal report in the generated Swift bindings.
const EXPECTED_SWIFT_REGION_REMOVAL_REPORT_MARKER: &str =
    "public struct RegionRemovalReport: Equatable, Hashable {";
/// Marker of the foreign region download progress listener contract in the
/// generated Swift bindings.
const EXPECTED_SWIFT_REGION_DOWNLOAD_PROGRESS_LISTENER_MARKER: &str =
    "public protocol RegionDownloadProgressListener: AnyObject, Sendable {";
/// Marker of the listener callback member in the generated Swift bindings.
const EXPECTED_SWIFT_ON_REGION_DOWNLOAD_PROGRESS_MARKER: &str =
    "func onRegionDownloadProgress(progress: RegionDownloadProgress) ";
/// Marker of the region download member in the generated Swift bindings: the
/// per-member `[Async]` lowers to `async throws`.
const EXPECTED_SWIFT_DOWNLOAD_REGION_MARKER: &str = "func downloadRegion(region: RegionDefinition, progressListener: RegionDownloadProgressListener) async throws  -> RegionRecord";
/// Marker of the region cancellation member in the generated Swift bindings
/// (synchronous and non-throwing).
const EXPECTED_SWIFT_CANCEL_REGION_DOWNLOAD_MARKER: &str =
    "func cancelRegionDownload(regionId: String) ";
/// Marker of the synchronous region sizing member in the generated Swift
/// bindings.
const EXPECTED_SWIFT_REGION_TILE_COUNT_MARKER: &str =
    "func regionTileCount(region: RegionDefinition) throws  -> UInt64";
/// Marker of the region listing member in the generated Swift bindings.
const EXPECTED_SWIFT_REGION_RECORDS_MARKER: &str =
    "func regionRecords() async throws  -> [RegionRecord]";
/// Marker of the single-region readback in the generated Swift bindings.
const EXPECTED_SWIFT_READ_REGION_MARKER: &str =
    "func regionRecord(regionId: String) async throws  -> RegionRecord?";
/// Marker of the region deletion member in the generated Swift bindings.
const EXPECTED_SWIFT_DELETE_REGION_MARKER: &str =
    "func deleteRegion(regionId: String) async throws  -> RegionRemovalReport";
/// Marker of the diagnostics listener registration in the generated Swift
/// bindings.
const EXPECTED_SWIFT_DIAGNOSTICS_ADD_LISTENER_MARKER: &str =
    "func addListener(listener: SdkDiagnosticEventListener, minimumSeverity: SdkLogSeverity)";
/// Marker of the diagnostics bounded flush in the generated Swift bindings.
const EXPECTED_SWIFT_DIAGNOSTICS_FLUSH_RECENT_MARKER: &str = "func flushRecent(listener: SdkDiagnosticEventListener, minimumSeverity: SdkLogSeverity, maximumCount: UInt32)  -> UInt32";

#[test]
fn swift_bindings_generate_from_the_frozen_udl() {
    let out_dir =
        Utf8PathBuf::from_path_buf(std::env::temp_dir().join("rustic-maps-native-core-swift"))
            .expect("temporary directory path is utf-8");
    let _ = std::fs::remove_dir_all(&out_dir);
    let source = Utf8PathBuf::from_path_buf(std::path::PathBuf::from("src/rustic_maps.udl"))
        .expect("relative UDL source path is utf-8");
    uniffi::generate(uniffi::GenerateOptions {
        languages: vec![uniffi::TargetLanguage::Swift],
        source,
        out_dir: out_dir.clone(),
        config_override: None,
        format: false,
        crate_filter: None,
        metadata_no_deps: false,
    })
    .expect("Swift generation from the frozen UDL must not fail");
    let swift_bindings_path = out_dir.join("rustic_maps.swift");
    let swift_bindings = std::fs::read_to_string(&swift_bindings_path)
        .expect("Swift bindings file must be written at the module root");
    for marker in [
        EXPECTED_SWIFT_NAMESPACE_MARKER,
        EXPECTED_SWIFT_VERSION_PROBE_MARKER,
        EXPECTED_SWIFT_CAMERA_MARKER,
        EXPECTED_SWIFT_ERROR_ENUM_MARKER,
        EXPECTED_SWIFT_ERROR_VARIANT_MARKER,
        EXPECTED_SWIFT_RENDER_INPUT_ERROR_MARKER,
        EXPECTED_SWIFT_SURFACE_UNAVAILABLE_ERROR_MARKER,
        EXPECTED_SWIFT_SURFACE_VIEW_MARKER,
        EXPECTED_SWIFT_ATTACH_MARKER,
        EXPECTED_SWIFT_DRAW_MARKER,
        EXPECTED_SWIFT_SET_OVERLAYS_MARKER,
        EXPECTED_SWIFT_OVERLAY_DESCRIPTOR_MARKER,
        EXPECTED_SWIFT_POINTER_MARKER,
        EXPECTED_SWIFT_MAPQUEST_REGISTRATION_MARKER,
        EXPECTED_SWIFT_TILE_CACHE_REGISTRATION_MARKER,
        EXPECTED_SWIFT_CAMERA_GETTERS_MARKER,
        EXPECTED_SWIFT_DRAW_STATISTICS_MARKER,
        EXPECTED_SWIFT_TILE_CACHE_TERMS_MARKER,
        EXPECTED_SWIFT_TILE_CACHE_CEILING_FIELD_MARKER,
        EXPECTED_SWIFT_TILE_CACHE_SETTER_MARKER,
        EXPECTED_SWIFT_TILE_CACHE_TERMS_READBACK_MARKER,
        EXPECTED_SWIFT_TILE_CACHE_STATISTICS_MARKER,
        EXPECTED_SWIFT_TILE_CACHE_STATISTICS_READBACK_MARKER,
        EXPECTED_SWIFT_DEVIATION_GRADE_KIND_MARKER,
        EXPECTED_SWIFT_DEVIATION_GRADE_FIELD_MARKER,
        EXPECTED_SWIFT_DISTANCE_PHRASE_SUPPRESSED_MARKER,
        EXPECTED_SWIFT_REROUTE_POLICY_MARKER,
        EXPECTED_SWIFT_REROUTE_COOLDOWN_FIELD_MARKER,
        EXPECTED_SWIFT_REROUTE_MOVEMENT_FIELD_MARKER,
        EXPECTED_SWIFT_REROUTE_BEGIN_MARKER,
        EXPECTED_SWIFT_REROUTE_CANCEL_MARKER,
        EXPECTED_SWIFT_REROUTE_POLICY_SETTER_MARKER,
        EXPECTED_SWIFT_REROUTE_POLICY_DEFAULT_MARKER,
        EXPECTED_SWIFT_IN_FLIGHT_FIELD_MARKER,
        EXPECTED_SWIFT_UTTERANCE_ID_FIELD_MARKER,
        EXPECTED_SWIFT_PROVIDER_FAILURE_DIAGNOSTICS_READBACK_MARKER,
        EXPECTED_SWIFT_PROVIDER_FAILURE_DIAGNOSTIC_MARKER,
        EXPECTED_SWIFT_FAILURE_CLASSIFICATION_FIELD_MARKER,
        EXPECTED_SWIFT_PROVIDER_FAILURE_CLASSIFICATION_MARKER,
        EXPECTED_SWIFT_UNCLASSIFIED_VARIANT_MARKER,
        EXPECTED_SWIFT_SEARCH_ENGINE_MARKER,
        EXPECTED_SWIFT_ROUTING_ENGINE_MARKER,
        EXPECTED_SWIFT_OFFLINE_MANAGER_MARKER,
        EXPECTED_SWIFT_OFFLINE_ROUTE_MARKER,
        EXPECTED_SWIFT_DIAGNOSTICS_MARKER,
        EXPECTED_SWIFT_ADD_OVERLAY_MARKER,
        EXPECTED_SWIFT_UPDATE_OVERLAY_MARKER,
        EXPECTED_SWIFT_REMOVE_OVERLAY_MARKER,
        EXPECTED_SWIFT_ADD_NAVIGATION_LISTENER_MARKER,
        EXPECTED_SWIFT_STOP_NAVIGATION_MARKER,
        EXPECTED_SWIFT_OFFLINE_TILE_CACHE_TERMS_MARKER,
        EXPECTED_SWIFT_OFFLINE_TILE_TIME_TO_LIVE_MARKER,
        EXPECTED_SWIFT_REGION_DEFINITION_MARKER,
        EXPECTED_SWIFT_REGION_RECORD_MARKER,
        EXPECTED_SWIFT_REGION_DOWNLOAD_PROGRESS_MARKER,
        EXPECTED_SWIFT_REGION_DOWNLOAD_PHASE_MARKER,
        EXPECTED_SWIFT_REGION_REMOVAL_REPORT_MARKER,
        EXPECTED_SWIFT_REGION_DOWNLOAD_PROGRESS_LISTENER_MARKER,
        EXPECTED_SWIFT_ON_REGION_DOWNLOAD_PROGRESS_MARKER,
        EXPECTED_SWIFT_DOWNLOAD_REGION_MARKER,
        EXPECTED_SWIFT_CANCEL_REGION_DOWNLOAD_MARKER,
        EXPECTED_SWIFT_REGION_TILE_COUNT_MARKER,
        EXPECTED_SWIFT_REGION_RECORDS_MARKER,
        EXPECTED_SWIFT_READ_REGION_MARKER,
        EXPECTED_SWIFT_DELETE_REGION_MARKER,
        EXPECTED_SWIFT_DIAGNOSTICS_ADD_LISTENER_MARKER,
        EXPECTED_SWIFT_DIAGNOSTICS_FLUSH_RECENT_MARKER,
    ] {
        assert!(
            swift_bindings.contains(marker),
            "generated Swift bindings missing marker `{marker}`"
        );
    }
}
