//! Bindings-compilation test: the frozen UDL contract must generate foreign
//! bindings without errors. Kotlin is the binding the platform slices build
//! first, so it is generated in-process through the pinned `uniffi` release
//! and checked for the core API markers.

use camino::Utf8PathBuf;

/// Marker of the namespace in the generated Kotlin bindings: the package
/// path every Kotlin call site imports.
const EXPECTED_KOTLIN_NAMESPACE_MARKER: &str = "package uniffi.rustic_maps";
/// Marker of the camera object in the generated Kotlin bindings.
const EXPECTED_KOTLIN_CAMERA_MARKER: &str = "open class MapCamera:";
/// Marker of the fielded error variant in the generated Kotlin bindings.
const EXPECTED_KOTLIN_ERROR_VARIANT_MARKER: &str = "class InvalidCoordinateInput(";
/// Marker of the version probe in the generated Kotlin bindings.
const EXPECTED_KOTLIN_VERSION_PROBE_MARKER: &str = "getSdkCompiledVersion";
/// Kotlin marker text of the optional screen projection return type.
const EXPECTED_KOTLIN_OPTIONAL_SCREEN_RETURN_MARKER: &str = "ScreenPoint?";
/// Marker of the inset record in the generated Kotlin bindings.
const EXPECTED_KOTLIN_EDGE_INSETS_MARKER: &str = "data class EdgeInsets (";
/// Marker of the fit-bounds companion constructor in the generated Kotlin
/// bindings: the named UDL constructor surfaces as a companion function.
const EXPECTED_KOTLIN_FIT_BOUNDS_CONSTRUCTOR_MARKER: &str = "fun `fitBounds`(";
/// Marker that the fit-bounds companion constructor throws the generated
/// `SdkException` rather than returning it.
const EXPECTED_KOTLIN_FIT_BOUNDS_THROW_MARKER: &str =
    "@Throws(SdkException::class) fun `fitBounds`(";
/// Marker of the async provider object in the generated Kotlin bindings.
const EXPECTED_KOTLIN_PROVIDER_OBJECT_MARKER: &str = "open class MapSdk:";
/// Marker of an exported async method in the generated Kotlin bindings:
/// `uniffi` lowers per-member `[Async]` UDL methods to `suspend fun`.
const EXPECTED_KOTLIN_ASYNC_METHOD_MARKER: &str = "suspend fun `forwardGeocode`(";
/// Marker of the place record in the generated Kotlin bindings.
const EXPECTED_KOTLIN_PLACE_RESULT_MARKER: &str = "data class Place (";
/// Marker of the route-options record in the generated Kotlin bindings.
const EXPECTED_KOTLIN_ROUTE_OPTIONS_MARKER: &str = "data class RouteOptions (";
/// Marker of the raster-tile record in the generated Kotlin bindings.
const EXPECTED_KOTLIN_RASTER_TILE_MARKER: &str = "data class RasterTileData (";
/// Marker of the provider rate-limit error variant in the generated Kotlin
/// bindings.
const EXPECTED_KOTLIN_RATE_LIMIT_MARKER: &str = "class RateLimitExhausted(";
/// Marker of the not-configured error variant in the generated Kotlin
/// bindings.
const EXPECTED_KOTLIN_PROVIDER_NOT_CONFIGURED_MARKER: &str = "class ProviderNotConfigured(";
/// Marker of the foreign-callback geocoding contract in the generated Kotlin
/// bindings: a `[Trait, WithForeign]` interface the Kotlin implementor
/// realizes (`override suspend fun ...`).
const EXPECTED_KOTLIN_GEOCODING_CALLBACK_INTERFACE_MARKER: &str =
    "public interface GeocodingProvider {";
/// Marker of an exported async callback member in the generated Kotlin
/// bindings: the per-member `[Async]` UDL methods of the callback contracts
/// lower to `suspend fun`.
const EXPECTED_KOTLIN_CALLBACK_ASYNC_METHOD_MARKER: &str =
    "suspend fun `forwardGeocode`(`addressText`: kotlin.String): List<Place>";
/// Marker of the foreign-provider registration method in the generated
/// Kotlin bindings, taking the callback interface.
const EXPECTED_KOTLIN_REGISTER_GEOCODING_FROM_FOREIGN_MARKER: &str =
    "fun `registerGeocodingProviderFromForeign`(`geocodingProvider`: GeocodingProvider)";
/// Marker of the foreign raster-tile provider registration method in the
/// generated Kotlin bindings.
const EXPECTED_KOTLIN_REGISTER_RASTER_TILE_FROM_FOREIGN_MARKER: &str =
    "fun `registerRasterTileProviderFromForeign`(`rasterTileProvider`: RasterTileProvider)";
/// Marker of the round-8 surface view object in the generated Kotlin
/// bindings.
const EXPECTED_KOTLIN_SURFACE_VIEW_MARKER: &str = "open class MapView:";
/// Marker of the attach member's signature in the generated Kotlin
/// bindings: the handle record plus the physical-pixel extents lower as
/// `UInt`, and the throwable member carries no suspend (a synchronous GPU
/// setup call).
const EXPECTED_KOTLIN_ATTACH_MARKER: &str = "fun `attach`(`surfaceHandle`: SurfaceHandle, `widthPixels`: kotlin.UInt, `heightPixels`: kotlin.UInt)";
/// Marker of the per-frame draw member in the generated Kotlin bindings:
/// the per-member `[Async]` lowers to `suspend fun` even over a `void`
/// UDL return type.
const EXPECTED_KOTLIN_ASYNC_DRAW_MARKER: &str = "suspend fun `draw`(`camera`: MapCamera)";
/// Marker of the overlay list replacement in the generated Kotlin bindings.
const EXPECTED_KOTLIN_SET_OVERLAYS_MARKER: &str =
    "fun `setOverlays`(`overlayDescriptors`: List<MapOverlay>)";
/// Marker of the overlay descriptor record in the generated Kotlin bindings.
const EXPECTED_KOTLIN_OVERLAY_DESCRIPTOR_MARKER: &str = "data class MapOverlay (";
/// Marker of the style record of the overlays in the generated Kotlin
/// bindings.
const EXPECTED_KOTLIN_OVERLAY_COLOR_MARKER: &str = "data class OverlayColor (";
/// Marker of the nullable `u64` platform pointer in the generated Kotlin
/// bindings: `u64?` lowers to `ULong?`.
const EXPECTED_KOTLIN_POINTER_MARKER: &str = "var `appkitViewPointer`: kotlin.ULong?";
/// Marker of the one-call whole-provider-set registration in the generated
/// Kotlin bindings, throwing the generated exception like every fallible
/// surface member does.
const EXPECTED_KOTLIN_MAPQUEST_REGISTRATION_MARKER: &str = "@Throws(SdkException::class)override fun `configureProviderSet`(`providerKey`: kotlin.String, `credential`: kotlin.String)";
/// Marker of the round-9 tile-cache provider registration in the generated
/// Kotlin bindings: the byte budget crosses as `ULong`.
const EXPECTED_KOTLIN_TILE_CACHE_REGISTRATION_MARKER: &str = "@Throws(SdkException::class)override fun `setProviderCredentialWithTileCache`(`providerKey`: kotlin.String, `credential`: kotlin.String, `tileCacheDatabasePath`: kotlin.String, `tileCacheAbsoluteMaximumBytes`: kotlin.ULong)";
/// Marker of the round-9 camera attribute getters in the generated Kotlin
/// bindings (plain methods: the UDL parser supports no `[Attribute]`
/// member).
const EXPECTED_KOTLIN_CAMERA_GETTERS_MARKER: &str = "fun `zoomLevel`(): kotlin.Double";
/// Marker of the round-9 per-frame draw statistics return in the generated
/// Kotlin bindings.
const EXPECTED_KOTLIN_DRAW_STATISTICS_MARKER: &str =
    "suspend fun `draw`(`camera`: MapCamera): FrameStatistics";
/// Marker of the round-9 tile-frame statistics record in the generated
/// Kotlin bindings.
const EXPECTED_KOTLIN_TILE_FRAME_STATISTICS_MARKER: &str = "data class FrameStatistics (";
/// Marker of the round-8 surface-render-input error variant in the
/// generated Kotlin bindings.
const EXPECTED_KOTLIN_RENDER_INPUT_ERROR_MARKER: &str = "class InvalidRenderInput(";
/// Marker of the round-8 surface-unavailable error variant in the generated
/// Kotlin bindings.
const EXPECTED_KOTLIN_SURFACE_UNAVAILABLE_ERROR_MARKER: &str = "class RenderingSurfaceUnavailable(";
/// Marker of the M2 tile-cache settings record in the generated Kotlin
/// bindings.
const EXPECTED_KOTLIN_TILE_CACHE_TERMS_MARKER: &str = "data class TileCacheTerms (";
/// Marker of the nullable provider-terms-ceiling field in the generated
/// Kotlin bindings: `u64?` lowers to `ULong?`.
const EXPECTED_KOTLIN_TILE_CACHE_CEILING_FIELD_MARKER: &str =
    "var `providerTermsCeilingSeconds`: kotlin.ULong?";
/// Marker of the tile-cache time-to-live override member in the generated
/// Kotlin bindings: `u64` lowers to `ULong`, throwing the generated
/// exception.
const EXPECTED_KOTLIN_TILE_CACHE_SETTER_MARKER: &str = "@Throws(SdkException::class)override fun `setRasterTileCacheTimeToLive`(`timeToLiveSeconds`: kotlin.ULong)";
/// Marker of the effective-terms readback member in the generated Kotlin
/// bindings (a synchronous method returning the settings record).
const EXPECTED_KOTLIN_TILE_CACHE_TERMS_READBACK_MARKER: &str =
    "fun `rasterTileCacheTerms`(): TileCacheTerms";
/// Marker of the installed tile payload cache's statistics record in the
/// generated Kotlin bindings.
const EXPECTED_KOTLIN_TILE_CACHE_STATISTICS_MARKER: &str = "data class TileCacheStatistics (";
/// Marker of the tile payload cache statistics readback in the generated
/// Kotlin bindings (an optional record return).
const EXPECTED_KOTLIN_TILE_CACHE_STATISTICS_READBACK_MARKER: &str =
    "fun `tileCacheStatistics`(): TileCacheStatistics?";
/// Marker of the round-11 two-grade deviation enum in the generated Kotlin
/// bindings.
const EXPECTED_KOTLIN_DEVIATION_GRADE_KIND_MARKER: &str = "enum class DeviationGradeKind {";
/// Marker of the grade-kind field on the deviation record in the generated
/// Kotlin bindings (the addition-only UDL record addition).
const EXPECTED_KOTLIN_DEVIATION_GRADE_FIELD_MARKER: &str = "var `gradeKind`: DeviationGradeKind";
/// Marker of the round-11 suppressed distance-phrase kind in the generated
/// Kotlin bindings (the frozen countdown's explicit-empty phrase).
const EXPECTED_KOTLIN_DISTANCE_PHRASE_SUPPRESSED_MARKER: &str = "SUPPRESSED;";
/// Marker of the round-11 reroute-recalculation policy record in the
/// generated Kotlin bindings (Ferrostar B3's hysteresis).
const EXPECTED_KOTLIN_REROUTE_POLICY_MARKER: &str = "data class ReroutePolicy (";
/// Marker of the reroute-recalculation cooldown field in the generated
/// Kotlin bindings: the seconds threshold lowers as `Double`.
const EXPECTED_KOTLIN_REROUTE_COOLDOWN_FIELD_MARKER: &str =
    "var `recalculationCooldownSeconds`: kotlin.Double";
/// Marker of the reroute-recalculation movement-threshold field in the
/// generated Kotlin bindings: the meters threshold lowers as `Double`.
const EXPECTED_KOTLIN_REROUTE_MOVEMENT_FIELD_MARKER: &str =
    "var `recalculationMovementMeters`: kotlin.Double";
/// Marker of the reroute-recalculation begin member in the generated Kotlin
/// bindings (a synchronous gate probe returning the boolean answer).
const EXPECTED_KOTLIN_REROUTE_BEGIN_MARKER: &str =
    "fun `beginRerouteCalculation`(): kotlin.Boolean";
/// Marker of the reroute-cancellation member in the generated Kotlin
/// bindings (settle without a route, non-throwing).
const EXPECTED_KOTLIN_REROUTE_CANCEL_MARKER: &str = "fun `cancelRerouteCalculation`()";
/// Marker of the reroute-policy configuration member in the generated Kotlin
/// bindings, validating both thresholds through the generated exception.
const EXPECTED_KOTLIN_REROUTE_POLICY_SETTER_MARKER: &str = "@Throws(SdkException::class)override fun `configureReroutePolicy`(`reroutePolicy`: ReroutePolicy)";
/// Marker of the reroute-policy defaults probe in the generated Kotlin
/// bindings.
const EXPECTED_KOTLIN_REROUTE_POLICY_DEFAULT_MARKER: &str =
    "fun `defaultReroutePolicy`(): ReroutePolicy";
/// Marker of the trip-state in-flight readback field in the generated Kotlin
/// bindings (the controller wall-clock gate fact).
const EXPECTED_KOTLIN_IN_FLIGHT_FIELD_MARKER: &str = "var `isCalculatingNewRoute`: kotlin.Boolean";
/// Marker of the round-11 speech-utterance identity field in the generated
/// Kotlin bindings (Ferrostar B4's monotonic id, `u64` lowering to `ULong`).
const EXPECTED_KOTLIN_UTTERANCE_ID_FIELD_MARKER: &str = "var `utteranceId`: kotlin.ULong";
/// Marker of the round-12 provider-failure diagnostic readback in the
/// generated Kotlin bindings (a synchronous method returning the retained
/// diagnostics).
const EXPECTED_KOTLIN_PROVIDER_FAILURE_DIAGNOSTICS_READBACK_MARKER: &str =
    "fun `latestProviderFailureDiagnostics`(): List<ProviderFailureDiagnostic>";
/// Marker of the round-12 provider-failure diagnostic record in the
/// generated Kotlin bindings.
const EXPECTED_KOTLIN_PROVIDER_FAILURE_DIAGNOSTIC_MARKER: &str =
    "data class ProviderFailureDiagnostic (";
/// Marker of the failure-classification field on the diagnostic record in
/// the generated Kotlin bindings.
const EXPECTED_KOTLIN_FAILURE_CLASSIFICATION_FIELD_MARKER: &str =
    "var `failureClassification`: ProviderFailureClassification";
/// Marker of the provider-failure classification enum in the generated
/// Kotlin bindings.
const EXPECTED_KOTLIN_PROVIDER_FAILURE_CLASSIFICATION_MARKER: &str =
    "enum class ProviderFailureClassification {";
/// Marker of the unclassified-callback-failure variant in the generated
/// Kotlin bindings.
const EXPECTED_KOTLIN_UNCLASSIFIED_VARIANT_MARKER: &str = "UNCLASSIFIED_CALLBACK_FAILURE,";
/// Marker of the M17 engine facades in the generated Kotlin bindings.
const EXPECTED_KOTLIN_SEARCH_ENGINE_MARKER: &str = "open class SearchEngine:";
/// Marker of the routing engine facade in the generated Kotlin bindings.
const EXPECTED_KOTLIN_ROUTING_ENGINE_MARKER: &str = "open class RoutingEngine:";
/// Marker of the offline manager facade in the generated Kotlin bindings.
const EXPECTED_KOTLIN_OFFLINE_MANAGER_MARKER: &str = "open class OfflineManager:";
/// Marker of the offline archive-route function in the generated Kotlin
/// bindings: the UDL `bytes` argument lowers to `ByteArray`, and the
/// per-member `[Async]` lowers to `suspend fun`.
const EXPECTED_KOTLIN_OFFLINE_ROUTE_MARKER: &str = "suspend fun `offlineRouteFromArchiveBytes`(`archiveBytes`: kotlin.ByteArray, `start`: GeographicCoordinate, `end`: GeographicCoordinate) : Route";
/// Marker of the diagnostics facade in the generated Kotlin bindings.
const EXPECTED_KOTLIN_DIAGNOSTICS_MARKER: &str = "open class Diagnostics:";
/// Marker of the id-addressed overlay addition in the generated Kotlin
/// bindings: it answers the generated identifier string.
const EXPECTED_KOTLIN_ADD_OVERLAY_MARKER: &str =
    "fun `addOverlay`(`overlay`: MapOverlay): kotlin.String";
/// Marker of the id-addressed overlay update in the generated Kotlin
/// bindings.
const EXPECTED_KOTLIN_UPDATE_OVERLAY_MARKER: &str =
    "fun `updateOverlay`(`overlayIdentifier`: kotlin.String, `overlay`: MapOverlay)";
/// Marker of the id-addressed overlay removal in the generated Kotlin
/// bindings: it answers whether the identifier was present.
const EXPECTED_KOTLIN_REMOVE_OVERLAY_MARKER: &str =
    "fun `removeOverlay`(`overlayIdentifier`: kotlin.String): kotlin.Boolean";
/// Marker of the four-topic listener subscription in the generated Kotlin
/// bindings.
const EXPECTED_KOTLIN_ADD_NAVIGATION_LISTENER_MARKER: &str =
    "fun `addNavigationListener`(`listener`: NavigationListener): Subscription";
/// Marker of the navigation stop verb in the generated Kotlin bindings.
const EXPECTED_KOTLIN_STOP_NAVIGATION_MARKER: &str = "fun `stopNavigation`()";
/// Marker of the offline manager's terms readback in the generated Kotlin
/// bindings.
const EXPECTED_KOTLIN_OFFLINE_TILE_CACHE_TERMS_MARKER: &str =
    "fun `tileCacheTerms`(): TileCacheTerms";
/// Marker of the offline manager's time-to-live override in the generated
/// Kotlin bindings.
const EXPECTED_KOTLIN_OFFLINE_TILE_TIME_TO_LIVE_MARKER: &str =
    "fun `setTileTimeToLive`(`timeToLiveSeconds`: kotlin.ULong)";
/// Marker of the offline region definition record in the generated Kotlin
/// bindings.
const EXPECTED_KOTLIN_REGION_DEFINITION_MARKER: &str = "data class RegionDefinition (";
/// Marker of the stored offline region record in the generated Kotlin
/// bindings.
const EXPECTED_KOTLIN_REGION_RECORD_MARKER: &str = "data class RegionRecord (";
/// Marker of the region download progress record in the generated Kotlin
/// bindings.
const EXPECTED_KOTLIN_REGION_DOWNLOAD_PROGRESS_MARKER: &str = "data class RegionDownloadProgress (";
/// Marker of the region download phase enum in the generated Kotlin
/// bindings.
const EXPECTED_KOTLIN_REGION_DOWNLOAD_PHASE_MARKER: &str = "enum class RegionDownloadPhase {";
/// Marker of the region removal report in the generated Kotlin bindings.
const EXPECTED_KOTLIN_REGION_REMOVAL_REPORT_MARKER: &str = "data class RegionRemovalReport (";
/// Marker of the foreign region download progress listener contract in the
/// generated Kotlin bindings.
const EXPECTED_KOTLIN_REGION_DOWNLOAD_PROGRESS_LISTENER_MARKER: &str =
    "public interface RegionDownloadProgressListener {";
/// Marker of the region download member in the generated Kotlin bindings:
/// the per-member `[Async]` lowers to `suspend fun` and the listener lowers
/// as a callback interface argument.
const EXPECTED_KOTLIN_DOWNLOAD_REGION_MARKER: &str = "suspend fun `downloadRegion`(`region`: RegionDefinition, `progressListener`: RegionDownloadProgressListener): RegionRecord";
/// Marker of the region cancellation member in the generated Kotlin bindings
/// (synchronous and non-throwing).
const EXPECTED_KOTLIN_CANCEL_REGION_DOWNLOAD_MARKER: &str =
    "fun `cancelRegionDownload`(`regionId`: kotlin.String)";
/// Marker of the synchronous region sizing member in the generated Kotlin
/// bindings (it throws the generated exception).
const EXPECTED_KOTLIN_REGION_TILE_COUNT_MARKER: &str = "@Throws(SdkException::class)override fun `regionTileCount`(`region`: RegionDefinition): kotlin.ULong";
/// Marker of the region listing member in the generated Kotlin bindings.
const EXPECTED_KOTLIN_REGION_RECORDS_MARKER: &str =
    "suspend fun `regionRecords`(): List<RegionRecord>";
/// Marker of the single-region readback in the generated Kotlin bindings.
const EXPECTED_KOTLIN_READ_REGION_MARKER: &str =
    "suspend fun `regionRecord`(`regionId`: kotlin.String): RegionRecord?";
/// Marker of the region deletion member in the generated Kotlin bindings.
const EXPECTED_KOTLIN_DELETE_REGION_MARKER: &str =
    "suspend fun `deleteRegion`(`regionId`: kotlin.String): RegionRemovalReport";
/// Marker of the diagnostics listener registration in the generated Kotlin
/// bindings.
const EXPECTED_KOTLIN_DIAGNOSTICS_ADD_LISTENER_MARKER: &str =
    "fun `addListener`(`listener`: SdkDiagnosticEventListener, `minimumSeverity`: SdkLogSeverity)";
/// Marker of the diagnostics bounded flush in the generated Kotlin bindings.
const EXPECTED_KOTLIN_DIAGNOSTICS_FLUSH_RECENT_MARKER: &str = "fun `flushRecent`(`listener`: SdkDiagnosticEventListener, `minimumSeverity`: SdkLogSeverity, `maximumCount`: kotlin.UInt): kotlin.UInt";

#[test]
fn kotlin_bindings_generate_from_the_frozen_udl() {
    let out_dir =
        Utf8PathBuf::from_path_buf(std::env::temp_dir().join("rustic-maps-native-core-kotlin"))
            .expect("temporary directory path is utf-8");
    let _ = std::fs::remove_dir_all(&out_dir);
    let source = Utf8PathBuf::from_path_buf(std::path::PathBuf::from("src/rustic_maps.udl"))
        .expect("relative UDL source path is utf-8");
    uniffi::generate(uniffi::GenerateOptions {
        languages: vec![uniffi::TargetLanguage::Kotlin],
        source,
        out_dir: out_dir.clone(),
        config_override: None,
        format: false,
        crate_filter: None,
        metadata_no_deps: false,
    })
    .expect("Kotlin generation from the frozen UDL must not fail");
    let kotlin_bindings_path = out_dir.join("uniffi/rustic_maps/rustic_maps.kt");
    let kotlin_bindings = std::fs::read_to_string(&kotlin_bindings_path)
        .expect("Kotlin bindings file must be written next to the defaulted package");
    for marker in [
        EXPECTED_KOTLIN_NAMESPACE_MARKER,
        EXPECTED_KOTLIN_CAMERA_MARKER,
        EXPECTED_KOTLIN_ERROR_VARIANT_MARKER,
        EXPECTED_KOTLIN_VERSION_PROBE_MARKER,
        EXPECTED_KOTLIN_OPTIONAL_SCREEN_RETURN_MARKER,
        EXPECTED_KOTLIN_EDGE_INSETS_MARKER,
        EXPECTED_KOTLIN_FIT_BOUNDS_CONSTRUCTOR_MARKER,
        EXPECTED_KOTLIN_FIT_BOUNDS_THROW_MARKER,
        EXPECTED_KOTLIN_PROVIDER_OBJECT_MARKER,
        EXPECTED_KOTLIN_ASYNC_METHOD_MARKER,
        EXPECTED_KOTLIN_PLACE_RESULT_MARKER,
        EXPECTED_KOTLIN_ROUTE_OPTIONS_MARKER,
        EXPECTED_KOTLIN_RASTER_TILE_MARKER,
        EXPECTED_KOTLIN_RATE_LIMIT_MARKER,
        EXPECTED_KOTLIN_PROVIDER_NOT_CONFIGURED_MARKER,
        EXPECTED_KOTLIN_GEOCODING_CALLBACK_INTERFACE_MARKER,
        EXPECTED_KOTLIN_CALLBACK_ASYNC_METHOD_MARKER,
        EXPECTED_KOTLIN_REGISTER_GEOCODING_FROM_FOREIGN_MARKER,
        EXPECTED_KOTLIN_REGISTER_RASTER_TILE_FROM_FOREIGN_MARKER,
        EXPECTED_KOTLIN_SURFACE_VIEW_MARKER,
        EXPECTED_KOTLIN_ATTACH_MARKER,
        EXPECTED_KOTLIN_ASYNC_DRAW_MARKER,
        EXPECTED_KOTLIN_SET_OVERLAYS_MARKER,
        EXPECTED_KOTLIN_OVERLAY_DESCRIPTOR_MARKER,
        EXPECTED_KOTLIN_OVERLAY_COLOR_MARKER,
        EXPECTED_KOTLIN_POINTER_MARKER,
        EXPECTED_KOTLIN_MAPQUEST_REGISTRATION_MARKER,
        EXPECTED_KOTLIN_TILE_CACHE_REGISTRATION_MARKER,
        EXPECTED_KOTLIN_CAMERA_GETTERS_MARKER,
        EXPECTED_KOTLIN_DRAW_STATISTICS_MARKER,
        EXPECTED_KOTLIN_TILE_FRAME_STATISTICS_MARKER,
        EXPECTED_KOTLIN_RENDER_INPUT_ERROR_MARKER,
        EXPECTED_KOTLIN_SURFACE_UNAVAILABLE_ERROR_MARKER,
        EXPECTED_KOTLIN_TILE_CACHE_TERMS_MARKER,
        EXPECTED_KOTLIN_TILE_CACHE_CEILING_FIELD_MARKER,
        EXPECTED_KOTLIN_TILE_CACHE_SETTER_MARKER,
        EXPECTED_KOTLIN_TILE_CACHE_TERMS_READBACK_MARKER,
        EXPECTED_KOTLIN_TILE_CACHE_STATISTICS_MARKER,
        EXPECTED_KOTLIN_TILE_CACHE_STATISTICS_READBACK_MARKER,
        EXPECTED_KOTLIN_DEVIATION_GRADE_KIND_MARKER,
        EXPECTED_KOTLIN_DEVIATION_GRADE_FIELD_MARKER,
        EXPECTED_KOTLIN_DISTANCE_PHRASE_SUPPRESSED_MARKER,
        EXPECTED_KOTLIN_REROUTE_POLICY_MARKER,
        EXPECTED_KOTLIN_REROUTE_COOLDOWN_FIELD_MARKER,
        EXPECTED_KOTLIN_REROUTE_MOVEMENT_FIELD_MARKER,
        EXPECTED_KOTLIN_REROUTE_BEGIN_MARKER,
        EXPECTED_KOTLIN_REROUTE_CANCEL_MARKER,
        EXPECTED_KOTLIN_REROUTE_POLICY_SETTER_MARKER,
        EXPECTED_KOTLIN_REROUTE_POLICY_DEFAULT_MARKER,
        EXPECTED_KOTLIN_IN_FLIGHT_FIELD_MARKER,
        EXPECTED_KOTLIN_UTTERANCE_ID_FIELD_MARKER,
        EXPECTED_KOTLIN_PROVIDER_FAILURE_DIAGNOSTICS_READBACK_MARKER,
        EXPECTED_KOTLIN_PROVIDER_FAILURE_DIAGNOSTIC_MARKER,
        EXPECTED_KOTLIN_FAILURE_CLASSIFICATION_FIELD_MARKER,
        EXPECTED_KOTLIN_PROVIDER_FAILURE_CLASSIFICATION_MARKER,
        EXPECTED_KOTLIN_UNCLASSIFIED_VARIANT_MARKER,
        EXPECTED_KOTLIN_SEARCH_ENGINE_MARKER,
        EXPECTED_KOTLIN_ROUTING_ENGINE_MARKER,
        EXPECTED_KOTLIN_OFFLINE_MANAGER_MARKER,
        EXPECTED_KOTLIN_OFFLINE_ROUTE_MARKER,
        EXPECTED_KOTLIN_DIAGNOSTICS_MARKER,
        EXPECTED_KOTLIN_ADD_OVERLAY_MARKER,
        EXPECTED_KOTLIN_UPDATE_OVERLAY_MARKER,
        EXPECTED_KOTLIN_REMOVE_OVERLAY_MARKER,
        EXPECTED_KOTLIN_ADD_NAVIGATION_LISTENER_MARKER,
        EXPECTED_KOTLIN_STOP_NAVIGATION_MARKER,
        EXPECTED_KOTLIN_OFFLINE_TILE_CACHE_TERMS_MARKER,
        EXPECTED_KOTLIN_OFFLINE_TILE_TIME_TO_LIVE_MARKER,
        EXPECTED_KOTLIN_REGION_DEFINITION_MARKER,
        EXPECTED_KOTLIN_REGION_RECORD_MARKER,
        EXPECTED_KOTLIN_REGION_DOWNLOAD_PROGRESS_MARKER,
        EXPECTED_KOTLIN_REGION_DOWNLOAD_PHASE_MARKER,
        EXPECTED_KOTLIN_REGION_REMOVAL_REPORT_MARKER,
        EXPECTED_KOTLIN_REGION_DOWNLOAD_PROGRESS_LISTENER_MARKER,
        EXPECTED_KOTLIN_DOWNLOAD_REGION_MARKER,
        EXPECTED_KOTLIN_CANCEL_REGION_DOWNLOAD_MARKER,
        EXPECTED_KOTLIN_REGION_TILE_COUNT_MARKER,
        EXPECTED_KOTLIN_REGION_RECORDS_MARKER,
        EXPECTED_KOTLIN_READ_REGION_MARKER,
        EXPECTED_KOTLIN_DELETE_REGION_MARKER,
        EXPECTED_KOTLIN_DIAGNOSTICS_ADD_LISTENER_MARKER,
        EXPECTED_KOTLIN_DIAGNOSTICS_FLUSH_RECENT_MARKER,
    ] {
        assert!(
            kotlin_bindings.contains(marker),
            "generated Kotlin bindings missing marker `{marker}`"
        );
    }
}
