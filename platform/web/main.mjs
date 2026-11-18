// Browser entry point: opens the generated WebAssembly module, starts the
// SDK, and exposes the compatibility surface on `window.mapTest` for the
// Playwright harness. The `window.mapTestReady` promise resolves only after
// `uniffiInitAsync` opened the module and the Rust-side runtime is ready, so
// tests can await readiness before asserting.
//
// The package is one generated `wasm2` module that carries two surfaces:
//
// - the contract namespace (`./generated/index.js`): the generated `MapSdk`,
//   `SdkError` and the pure projection/offline calls, initialized explicitly
//   by `uniffiInitAsync`;
// - the wasm-bindgen glue (`./generated/rustic_maps_bg.js`): the canvas
//   `MapView` and the browser transports (provider registration and calls,
//   diagnostics, navigation, positioning). Objects never cross the two ABIs,
//   so the page keeps the glue's `MapCamera` for canvas frames and the
//   generated record types never reach the canvas.
//
// The `window.mapTest` names and shapes the harness relies on are preserved
// here; the façade is the only place a generated difference is adapted.
import {
  SdkError_Tags,
  coordinateToNormalizedWorld as generated_coordinate_to_normalized_world,
  getSdkCompiledVersion,
  offlineRouteFromArchiveBytes as generated_offline_route_from_archive_bytes,
  tileKeyForCoordinate as generated_tile_key_for_coordinate,
  tilesCovering as generated_tiles_covering,
  uniffiInitAsync,
} from './generated/index.js';
import {
  MapCamera,
  MapView as BrowserMapView,
  MapsSdk,
  Navigator,
  PositionController,
  SdkLogSeverity,
  TileResolution,
  calculateDistanceMatrix,
  calculateRoute,
  calculateRouteWithOptions,
  clearProviderRegistrations,
  clearUrlTemplateRasterTileSource,
  configureProvider,
  configureProviderSet,
  devicePixelRatioThreshold,
  fetchElevationPoints,
  fetchRasterTile,
  fetchTrafficFlow,
  fetchTrafficIncidents,
  flushRecentDiagnosticEvents,
  forwardGeocode,
  panCameraByScreenPixels,
  providerRegistrations,
  registerUrlTemplateRasterTileSource,
  reverseGeocode,
  rotateCameraByDegreesAroundScreenPoint,
  routeCapabilities,
  searchPlaces,
  searchPlacesInBoundary,
  setProviderBaseUrl,
  setResponseExcerptCapture,
  suggestPlaces,
  suggestPlacesInBoundary,
  subscribeToDiagnosticEvents,
  trafficFlowCacheStatistics,
  zoomCameraByLevelsAroundScreenPoint,
} from './generated/rustic_maps_bg.js';

/** The module the generated entrypoint opens, staged beside it. */
const GENERATED_MODULE_URL = new URL('./generated/rustic_maps.wasm', import.meta.url);

/** Every generated `SdkError` tag; a thrown object with one is a typed SDK failure. */
const GENERATED_SDK_ERROR_TAGS = new Set(Object.values(SdkError_Tags));

/**
 * The message each generated variant answers, reproducing the core SDK error
 * display the page's `{ code, message }` shape has always carried. The
 * generated record stores only the variant's payload (the detail text, or the
 * contract name), so the prefix is restored here; the two offline-routing
 * assertions read the core's `offline graph invalid:` prefix.
 */
const GENERATED_ERROR_MESSAGE_BUILDERS = new Map([
  [
    SdkError_Tags.InvalidCoordinateInput,
    (inner) => `invalid coordinate input: ${inner.detail}`,
  ],
  [
    SdkError_Tags.NetworkRequestFailed,
    (inner) => `network request failed: ${inner.detail}`,
  ],
  [
    SdkError_Tags.ResponseParsingFailure,
    (inner) => `response parsing failure: ${inner.detail}`,
  ],
  [
    SdkError_Tags.ProviderRequestRejected,
    (inner) => `provider rejected request: ${inner.detail}`,
  ],
  [SdkError_Tags.RateLimitExhausted, () => 'rate limit exhausted'],
  [
    SdkError_Tags.ProviderNotConfigured,
    (inner) => `no provider configured for ${inner.contractName}`,
  ],
  [
    SdkError_Tags.InvalidNavigationInput,
    (inner) => `invalid navigation input: ${inner.detail}`,
  ],
  [
    SdkError_Tags.InvalidRenderInput,
    (inner) => `invalid render input: ${inner.detail}`,
  ],
  [
    SdkError_Tags.RenderingSurfaceUnavailable,
    (inner) => `rendering surface unavailable: ${inner.detail}`,
  ],
  [
    SdkError_Tags.CacheOperationFailure,
    (inner) => `cache operation failure: ${inner.detail}`,
  ],
  [
    SdkError_Tags.OfflineRoutingFailure,
    (inner) => `offline graph invalid: ${inner.detail}`,
  ],
]);

/**
 * Reads the generated `SdkError` tag from a thrown value, or null when the
 * value is not a generated SDK failure. The generated error is a tagged class
 * union; `tag` is its variant name.
 * @param {unknown} error
 * @returns {string | null}
 */
function generated_sdk_error_tag(error) {
  if (error === null || typeof error !== 'object') {
    return null;
  }
  const tag = error.tag;
  return typeof tag === 'string' && GENERATED_SDK_ERROR_TAGS.has(tag) ? tag : null;
}

/**
 * The message of a generated `SdkError`: the core display text for its
 * variant, built from the variant's own payload. A generated variant the
 * table does not know falls back to its payload verbatim.
 * @param {object} error
 * @param {string} tag
 * @returns {string}
 */
function generated_sdk_error_message(error, tag) {
  const inner = error.inner ?? {};
  const message_builder = GENERATED_ERROR_MESSAGE_BUILDERS.get(tag);
  if (message_builder !== undefined) {
    return message_builder(inner);
  }
  if (typeof inner.detail === 'string') {
    return inner.detail;
  }
  if (typeof inner.contractName === 'string') {
    return `no provider configured for ${inner.contractName}`;
  }
  return tag;
}

/**
 * Adapts a generated `SdkError` back to the `{ code, message }` failure shape
 * the page and its specs read (`error.code`, `error.message`, and
 * `String(error)` carrying the detail). A non-generated failure passes through
 * untouched, so a page-side `TypeError` keeps its own identity.
 * @param {unknown} error
 * @returns {unknown}
 */
function adapted_failure(error) {
  const tag = generated_sdk_error_tag(error);
  if (tag === null) {
    return error;
  }
  const adapted_error = new Error(generated_sdk_error_message(error, tag));
  adapted_error.code = tag;
  return adapted_error;
}

/**
 * Runs one generated call and adapts its typed rejection.
 * @template T
 * @param {() => Promise<T> | T} call
 * @returns {Promise<T>}
 */
async function with_adapted_failure(call) {
  try {
    return await call();
  } catch (error) {
    throw adapted_failure(error);
  }
}

/**
 * Runs one synchronous generated call and adapts its typed rejection, keeping
 * the synchronous result the page's positional helpers have always returned.
 * @template T
 * @param {() => T} call
 * @returns {T}
 */
function with_adapted_sync_failure(call) {
  try {
    return call();
  } catch (error) {
    throw adapted_failure(error);
  }
}

/** Builds a contract `GeographicCoordinate` from the page's positional degrees. */
function geographic_coordinate(latitude, longitude) {
  return { latitude, longitude };
}

window.mapTestReady = (async () => {
  await uniffiInitAsync(GENERATED_MODULE_URL);
  // The SDK root the page creates once and hands to its MapViews
  // (`new MapView(canvas, maps)`). The generated contract `MapSdk` is not
  // this root: the canvas classes come from the wasm-bindgen glue and only
  // that root's constructor accepts them.
  const maps = await MapsSdk.create();
  const compiled_version = maps.compiledVersion;
  window.mapTest = {
    // The canvas surface keeps the wasm-bindgen classes. The generated
    // contract `MapView` is stripped from the browser contract and must never
    // reach the page; the alias names the one canvas class the page owns.
    MapCamera,
    MapView: BrowserMapView,
    MapsSdk,
    Navigator,
    PositionController,
    maps,
    TileResolution,
    SdkLogSeverity,
    // The contract-only pure calls run on the generated surface. Their
    // positional page signatures are preserved here, and the record shapes
    // the generated contract takes are built at this boundary.
    coordinateToNormalizedWorld: (latitude, longitude) =>
      with_adapted_sync_failure(() =>
        generated_coordinate_to_normalized_world(geographic_coordinate(latitude, longitude)),
      ),
    tileKeyForCoordinate: (latitude, longitude, zoom_level) =>
      with_adapted_sync_failure(() =>
        generated_tile_key_for_coordinate(
          geographic_coordinate(latitude, longitude),
          zoom_level,
        ),
      ),
    tilesCovering: (
      south_west_latitude,
      south_west_longitude,
      north_east_latitude,
      north_east_longitude,
      zoom_level,
    ) =>
      with_adapted_sync_failure(() =>
        generated_tiles_covering(
          {
            southWest: geographic_coordinate(south_west_latitude, south_west_longitude),
            northEast: geographic_coordinate(north_east_latitude, north_east_longitude),
          },
          zoom_level,
        ),
      ),
    getSdkCompiledVersion,
    devicePixelRatioThreshold,
    panCameraByScreenPixels,
    zoomCameraByLevelsAroundScreenPoint,
    rotateCameraByDegreesAroundScreenPoint,
    offlineRouteFromArchiveBytes: (
      archive_bytes,
      start_latitude,
      start_longitude,
      end_latitude,
      end_longitude,
    ) =>
      with_adapted_failure(() =>
        generated_offline_route_from_archive_bytes(
          archive_bytes,
          geographic_coordinate(start_latitude, start_longitude),
          geographic_coordinate(end_latitude, end_longitude),
        ),
      ),
    // The browser transport surface stays on the wasm-bindgen glue: the
    // provider calls read the fixture-origin override and the browser fetch
    // path that the generated catalog has no member for, and the canvas'
    // raster/vector slots are mirrored from this registry.
    configureProvider,
    configureProviderSet,
    setProviderBaseUrl,
    clearProviderRegistrations,
    providerRegistrations,
    routeCapabilities,
    forwardGeocode,
    reverseGeocode,
    fetchElevationPoints,
    registerUrlTemplateRasterTileSource,
    clearUrlTemplateRasterTileSource,
    suggestPlaces,
    suggestPlacesInBoundary,
    searchPlaces,
    searchPlacesInBoundary,
    calculateRoute,
    calculateRouteWithOptions,
    calculateDistanceMatrix,
    fetchTrafficIncidents,
    fetchTrafficFlow,
    trafficFlowCacheStatistics,
    fetchRasterTile,
    subscribeToDiagnosticEvents,
    flushRecentDiagnosticEvents,
    setResponseExcerptCapture,
  };
  const status_element = document.getElementById('harness-status');
  if (status_element) {
    status_element.textContent = `Map ready (version ${compiled_version})`;
  }
  return compiled_version;
})();
