// Browser entry point: opens the generated WebAssembly module, starts the
// SDK, and exposes the compatibility surface on `window.mapTest` for the
// Playwright harness. The `window.mapTestReady` promise resolves only after
// `uniffiInitAsync` opened the module and the Rust-side runtime is ready, so
// tests can await readiness before asserting.
//
// The package is one generated `wasm2` module that carries two surfaces:
//
// - the contract namespace (`./generated/index.js`): the generated `MapSdk`
//   and its provider, search, plan, traffic and diagnostics members, plus
//   the `SdkError` typed failures and the pure projection/offline calls,
//   initialized explicitly by `uniffiInitAsync`;
// - the wasm-bindgen glue (`./generated/rustic_maps_bg.js`): the canvas
//   `MapView`, its camera, the keyless URL-template raster fallback,
//   navigation and positioning. Objects never cross the two ABIs, so the
//   page keeps the glue's `MapCamera` for canvas frames and the generated
//   record types never reach the canvas.
//
// The `window.mapTest` names and shapes the harness relies on are preserved
// here; the façade is the only place a generated difference is adapted.
import {
  GeocodeQuality,
  MapSdk as GeneratedMapSdk,
  ProviderContract,
  RasterImageFormat,
  RasterTileResolution,
  RouteStrategy,
  SdkError_Tags,
  TrafficFlowCongestionLevel,
  TrafficIncidentSeverity,
  TrafficIncidentType,
  TransportationMode,
  UnitSystem,
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
  clearProviderRegistrations as canvas_clear_provider_registrations,
  clearSharedProviderSlots,
  clearUrlTemplateRasterTileSource,
  devicePixelRatioThreshold,
  flushRecentDiagnosticEvents,
  panCameraByScreenPixels,
  registerUrlTemplateRasterTileSource,
  rotateCameraByDegreesAroundScreenPoint,
  setProviderBaseUrl,
  setResponseExcerptCapture,
  subscribeToDiagnosticEvents,
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
  // An empty `name` keeps `Error.prototype.toString` at the message alone, so
  // `String(error)` stays the verbatim detail every surface reported.
  adapted_error.name = '';
  adapted_error.code = legacy_failure_code(tag);
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

/**
 * The page's established failure codes are the generated `SdkError` variant
 * names in lower camel case (`ProviderNotConfigured` →
 * `providerNotConfigured`), the spelling every surface has always reported.
 * @param {string} tag
 * @returns {string}
 */
function legacy_failure_code(tag) {
  return tag.charAt(0).toLowerCase() + tag.slice(1);
}

/**
 * The contract spelling of a provider capability the page names. The page
 * spells capabilities as the contract enum (`Routing`, `RasterTiles`), the
 * registry's `…Provider` form, or the lowercase capability name; every form
 * reaches the same generated `ProviderContract` member.
 * @param {string} contract_name_text
 * @returns {number}
 */
function provider_contract_from_name(contract_name_text) {
  const normalized = String(contract_name_text)
    .replace(/[^0-9A-Za-z]/g, '')
    .toLowerCase()
    .replace(/provider$/, '');
  switch (normalized) {
    case 'geocoding':
      return ProviderContract.Geocoding;
    case 'routing':
      return ProviderContract.Routing;
    case 'distancematrix':
      return ProviderContract.DistanceMatrix;
    case 'placesearch':
    case 'searchplace':
      return ProviderContract.PlaceSearch;
    case 'trafficincidents':
    case 'trafficincident':
      return ProviderContract.TrafficIncidents;
    case 'trafficflow':
      return ProviderContract.TrafficFlow;
    case 'elevation':
      return ProviderContract.Elevation;
    case 'rastertiles':
    case 'rastertile':
      return ProviderContract.RasterTiles;
    case 'vectortiles':
    case 'vectortile':
      return ProviderContract.VectorTiles;
    case 'offlinerouting':
      return ProviderContract.OfflineRouting;
    default:
      throw new Error(`"${contract_name_text}" is not a provider contract`);
  }
}

/**
 * The contract name a generated numeric enum member carries (`Geocoding`),
 * the text every readback the page consumes has always reported.
 * @param {object} enum_type
 * @param {number} value
 * @returns {string}
 */
function enum_member_name(enum_type, value) {
  const member_name = enum_type[value];
  return typeof member_name === 'string' ? member_name : String(value);
}

/** One generated `Place` as the flat row the page and its specs read. */
function place_row_from_generated(place) {
  return {
    structuredAddressText: place.structuredAddressText,
    latitudeDegrees: place.coordinate.latitude,
    longitudeDegrees: place.coordinate.longitude,
    qualityText: enum_member_name(GeocodeQuality, place.geocodeQuality),
  };
}

/** The generated place list as the flat rows every geocoding/search call answers. */
function place_rows_from_generated(places) {
  return places.map(place_row_from_generated);
}

/**
 * Reads a flat `[latitude, longitude, …]` array into the contract's
 * coordinates, refusing the same malformed shapes the browser seam refused.
 * @param {unknown} flat_values
 * @param {string} label
 * @returns {Array<{latitude: number, longitude: number}>}
 */
function coordinate_pairs_from_flat(flat_values, label) {
  if (
    !Array.isArray(flat_values)
    || flat_values.length === 0
    || flat_values.length % 2 !== 0
  ) {
    throw new Error(
      `${label} coordinates must be a non-empty, even-length [latitude, longitude, …] array`,
    );
  }
  const coordinates = [];
  for (let index = 0; index < flat_values.length; index += 2) {
    coordinates.push(geographic_coordinate(flat_values[index], flat_values[index + 1]));
  }
  return coordinates;
}

/**
 * Builds a contract `BoundingBox` from the page's four corners, refusing an
 * inverted box with the core's own wording before the SDK is asked.
 */
function bounding_box_from_corners(
  south_west_latitude,
  south_west_longitude,
  north_east_latitude,
  north_east_longitude,
) {
  const south_west = geographic_coordinate(south_west_latitude, south_west_longitude);
  const north_east = geographic_coordinate(north_east_latitude, north_east_longitude);
  if (
    south_west.latitude > north_east.latitude
    || south_west.longitude > north_east.longitude
  ) {
    throw new Error('bounding box corners are inverted');
  }
  return { southWest: south_west, northEast: north_east };
}

/** The contract's transportation mode for the page's name spelling. */
function transportation_mode_from_name(transportation_mode_name) {
  switch (String(transportation_mode_name).trim().toLowerCase()) {
    case 'automobile':
    case 'car':
      return TransportationMode.Automobile;
    case 'bicycle':
      return TransportationMode.Bicycle;
    case 'pedestrian':
    case 'walking':
      return TransportationMode.Pedestrian;
    default:
      throw new Error('the transportation mode must be automobile, bicycle or pedestrian');
  }
}

/** Reads one optional route-option text field, refusing a non-string value. */
function route_option_text(options, field_name, default_text) {
  const field_value = options[field_name];
  if (field_value === undefined || field_value === null) {
    return default_text;
  }
  if (typeof field_value !== 'string') {
    throw new Error(`the route option ${field_name} must be a string`);
  }
  return field_value;
}

/** Reads one optional route-option boolean field, defaulting to false. */
function route_option_boolean(options, field_name) {
  const field_value = options[field_name];
  if (field_value === undefined || field_value === null) {
    return false;
  }
  if (typeof field_value !== 'boolean') {
    throw new Error(`the route option ${field_name} must be a boolean`);
  }
  return field_value;
}

/**
 * The contract route options the SDK defaults to: automobile, fastest,
 * metric, no avoids, departing now.
 */
function default_route_options() {
  return {
    transportationMode: TransportationMode.Automobile,
    routeStrategy: RouteStrategy.Fastest,
    unitSystem: UnitSystem.Metric,
    avoidOptions: {
      tollRoads: false,
      limitedAccessHighways: false,
      ferries: false,
      unpavedRoads: false,
    },
  };
}

/**
 * The deepest departure instant a contract route request accepts: the last
 * exactly representable integer second, the browser seam's own bound.
 */
const MAXIMUM_DEPARTURE_TIMESTAMP_SECONDS = Number.MAX_SAFE_INTEGER;

/**
 * Reads the page's plain route-options object into the contract record,
 * refusing unknown enum names and malformed fields before a provider is
 * asked, with the same details the browser seam always answered.
 * @param {unknown} options
 */
function route_options_from_object(options) {
  if (options === null || typeof options !== 'object' || Array.isArray(options)) {
    throw new Error('the route options argument must be an object');
  }
  const transportationMode = transportation_mode_from_name(
    route_option_text(options, 'transportationMode', 'automobile'),
  );
  const route_strategy_text = route_option_text(
    options,
    'routeStrategy',
    'fastest',
  ).trim().toLowerCase();
  let routeStrategy;
  if (route_strategy_text === 'fastest') {
    routeStrategy = RouteStrategy.Fastest;
  } else if (route_strategy_text === 'shortest') {
    routeStrategy = RouteStrategy.Shortest;
  } else {
    throw new Error('the route strategy must be fastest or shortest');
  }
  const unit_system_text = route_option_text(
    options,
    'unitSystem',
    'metric',
  ).trim().toLowerCase();
  let unitSystem;
  if (unit_system_text === 'metric') {
    unitSystem = UnitSystem.Metric;
  } else if (unit_system_text === 'imperial') {
    unitSystem = UnitSystem.Imperial;
  } else {
    throw new Error('the unit system must be metric or imperial');
  }
  const avoid_field = options.avoidOptions;
  let avoidOptions = {
    tollRoads: false,
    limitedAccessHighways: false,
    ferries: false,
    unpavedRoads: false,
  };
  if (avoid_field !== undefined && avoid_field !== null) {
    if (typeof avoid_field !== 'object' || Array.isArray(avoid_field)) {
      throw new Error('the avoidOptions field must be an object');
    }
    avoidOptions = {
      tollRoads: route_option_boolean(avoid_field, 'tollRoads'),
      limitedAccessHighways: route_option_boolean(avoid_field, 'limitedAccessHighways'),
      ferries: route_option_boolean(avoid_field, 'ferries'),
      unpavedRoads: route_option_boolean(avoid_field, 'unpavedRoads'),
    };
  }
  const departure_field = options.departureTimestampSeconds;
  let departureTimestampSeconds;
  if (departure_field !== undefined && departure_field !== null) {
    if (typeof departure_field !== 'number') {
      throw new Error('the departure timestamp must be a number of seconds');
    }
    if (
      !Number.isFinite(departure_field)
      || departure_field < 0
      || departure_field > MAXIMUM_DEPARTURE_TIMESTAMP_SECONDS
    ) {
      throw new Error(
        'the departure timestamp must be finite, non-negative and within the u64 second range',
      );
    }
    departureTimestampSeconds = BigInt(departure_field);
  }
  return {
    transportationMode,
    routeStrategy,
    unitSystem,
    avoidOptions,
    departureTimestampSeconds,
  };
}

/** One generated `TrafficIncident` in the page's plain row shape. */
function traffic_incident_row_from_generated(incident) {
  return {
    identifier: incident.identifier,
    incidentType: enum_member_name(TrafficIncidentType, incident.incidentType),
    severity: enum_member_name(TrafficIncidentSeverity, incident.severity),
    shortDescriptionText: incident.shortDescriptionText,
    startCoordinate: incident.startCoordinate,
    endCoordinate: incident.endCoordinate ?? null,
  };
}

/** One generated `TrafficFlowData` in the page's plain row shape. */
function traffic_flow_row_from_generated(flow_row) {
  return {
    shapeCoordinates: flow_row.shapeCoordinates,
    currentSpeedKilometersPerHour: flow_row.currentSpeedKilometersPerHour ?? null,
    freeFlowSpeedKilometersPerHour: flow_row.freeFlowSpeedKilometersPerHour ?? null,
    congestionLevel: enum_member_name(
      TrafficFlowCongestionLevel,
      flow_row.congestionLevel,
    ),
  };
}

/**
 * The generated traffic-flow counters as the plain numbers the page compares
 * against: the contract carries them as 64-bit integers, which reach
 * JavaScript as `bigint`.
 */
function traffic_flow_cache_statistics_from_generated(statistics) {
  if (statistics === undefined || statistics === null) {
    return null;
  }
  return {
    hitCount: Number(statistics.hitCount),
    missCount: Number(statistics.missCount),
    entryCount: Number(statistics.entryCount),
    evictionCount: Number(statistics.evictionCount),
    segmentCount: Number(statistics.segmentCount),
    gridZoomLevel: Number(statistics.gridZoomLevel),
    maximumEntryCount: Number(statistics.maximumEntryCount),
    timeToLiveSeconds: Number(statistics.timeToLiveSeconds),
  };
}

window.mapTestReady = (async () => {
  await uniffiInitAsync(GENERATED_MODULE_URL);
  // The contract SDK every provider, search, plan and diagnostics call
  // resolves through. It is created before the canvas root because its
  // constructor installs the process-wide diagnostics subscriber the
  // readout below reads; a second constructor is the page's Clear act.
  let generated_map_sdk = new GeneratedMapSdk();
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
    // Providers, search, plans, traffic and raster tiles resolve through the
    // generated contract registry. The page's positional signatures and row
    // shapes are preserved at this boundary; the generated records, enums
    // and 64-bit counters are adapted here.
    configureProvider: (contract, provider_key, credential) =>
      with_adapted_sync_failure(() =>
        generated_map_sdk.configureProvider(
          provider_contract_from_name(contract),
          provider_key,
          credential,
        ),
      ),
    configureProviderSet: (provider_key, credential) =>
      with_adapted_sync_failure(() =>
        generated_map_sdk.configureProviderSet(provider_key, credential),
      ),
    setProviderBaseUrl,
    // The contract has no clear member: drop the provider mirrors the
    // generated registry published to the canvas, clear the canvas' own
    // transport slots, and replace the SDK instance so its native registry
    // goes with it. The keyless URL-template fallback survives, exactly as it
    // always has.
    clearProviderRegistrations: () => {
      canvas_clear_provider_registrations();
      clearSharedProviderSlots();
      generated_map_sdk = new GeneratedMapSdk();
    },
    providerRegistrations: () =>
      generated_map_sdk.providerRegistrations().map((registration) => ({
        contract: enum_member_name(ProviderContract, registration.contract),
        providerKey: registration.providerKey,
        displayName: registration.displayName,
        registered: registration.registered,
      })),
    routeCapabilities: () =>
      with_adapted_sync_failure(() => {
        const capabilities = generated_map_sdk.routeCapabilities();
        return {
          supportedTransportationModes: capabilities.supportedTransportationModes.map(
            (transportation_mode) =>
              enum_member_name(TransportationMode, transportation_mode),
          ),
          supportsShortestRouteStrategy: capabilities.supportsShortestRouteStrategy,
        };
      }),
    forwardGeocode: (query_text) =>
      with_adapted_failure(async () =>
        place_rows_from_generated(await generated_map_sdk.forwardGeocode(query_text)),
      ),
    reverseGeocode: (latitude, longitude) =>
      with_adapted_failure(async () =>
        place_rows_from_generated(
          await generated_map_sdk.reverseGeocode(
            geographic_coordinate(latitude, longitude),
          ),
        ),
      ),
    fetchElevationPoints: (point_coordinates) =>
      with_adapted_failure(async () => {
        const elevation_points = await generated_map_sdk.fetchElevationPoints(
          coordinate_pairs_from_flat(point_coordinates, 'elevation point'),
        );
        return elevation_points.map((elevation_point) => ({
          latitude: elevation_point.coordinate.latitude,
          longitude: elevation_point.coordinate.longitude,
          elevationMeters: elevation_point.elevationMeters,
        }));
      }),
    // The URL-template raster source is the canvas' keyless fallback, not a
    // configured provider: the generated registry deliberately does not see
    // it, so its two members stay on the browser transport.
    registerUrlTemplateRasterTileSource,
    clearUrlTemplateRasterTileSource,
    suggestPlaces: (query_text) =>
      with_adapted_failure(async () =>
        place_rows_from_generated(
          await generated_map_sdk.suggest(query_text, undefined),
        ),
      ),
    suggestPlacesInBoundary: (
      query_text,
      south_west_latitude,
      south_west_longitude,
      north_east_latitude,
      north_east_longitude,
    ) =>
      with_adapted_failure(async () =>
        place_rows_from_generated(
          await generated_map_sdk.suggest(
            query_text,
            bounding_box_from_corners(
              south_west_latitude,
              south_west_longitude,
              north_east_latitude,
              north_east_longitude,
            ),
          ),
        ),
      ),
    searchPlaces: (query_text) =>
      with_adapted_failure(async () =>
        place_rows_from_generated(await generated_map_sdk.search(query_text, undefined)),
      ),
    searchPlacesInBoundary: (
      query_text,
      south_west_latitude,
      south_west_longitude,
      north_east_latitude,
      north_east_longitude,
    ) =>
      with_adapted_failure(async () =>
        place_rows_from_generated(
          await generated_map_sdk.search(
            query_text,
            bounding_box_from_corners(
              south_west_latitude,
              south_west_longitude,
              north_east_latitude,
              north_east_longitude,
            ),
          ),
        ),
      ),
    calculateRoute: (
      origin_latitude,
      origin_longitude,
      destination_latitude,
      destination_longitude,
    ) =>
      with_adapted_failure(() =>
        generated_map_sdk.calculateRoute(
          [
            geographic_coordinate(origin_latitude, origin_longitude),
            geographic_coordinate(destination_latitude, destination_longitude),
          ],
          default_route_options(),
        ),
      ),
    calculateRouteWithOptions: (
      origin_latitude,
      origin_longitude,
      destination_latitude,
      destination_longitude,
      options,
    ) =>
      with_adapted_failure(() =>
        generated_map_sdk.calculateRoute(
          [
            geographic_coordinate(origin_latitude, origin_longitude),
            geographic_coordinate(destination_latitude, destination_longitude),
          ],
          route_options_from_object(options),
        ),
      ),
    calculateDistanceMatrix: (
      origin_coordinates,
      destination_coordinates,
      transportation_mode,
    ) =>
      with_adapted_failure(async () => {
        const distance_matrix = await generated_map_sdk.calculateDistanceMatrix(
          coordinate_pairs_from_flat(origin_coordinates, 'origin'),
          coordinate_pairs_from_flat(destination_coordinates, 'destination'),
          transportation_mode_from_name(transportation_mode),
        );
        return distance_matrix.entries;
      }),
    fetchTrafficIncidents: (
      south_west_latitude,
      south_west_longitude,
      north_east_latitude,
      north_east_longitude,
    ) =>
      with_adapted_failure(async () => {
        const traffic_incidents = await generated_map_sdk.fetchTrafficIncidents(
          bounding_box_from_corners(
            south_west_latitude,
            south_west_longitude,
            north_east_latitude,
            north_east_longitude,
          ),
        );
        return traffic_incidents.map(traffic_incident_row_from_generated);
      }),
    fetchTrafficFlow: (
      south_west_latitude,
      south_west_longitude,
      north_east_latitude,
      north_east_longitude,
    ) =>
      with_adapted_failure(async () => {
        const traffic_flow_rows = await generated_map_sdk.fetchTrafficFlow(
          bounding_box_from_corners(
            south_west_latitude,
            south_west_longitude,
            north_east_latitude,
            north_east_longitude,
          ),
        );
        return traffic_flow_rows.map(traffic_flow_row_from_generated);
      }),
    trafficFlowCacheStatistics: () =>
      traffic_flow_cache_statistics_from_generated(
        generated_map_sdk.trafficFlowCacheStatistics(),
      ),
    fetchRasterTile: (zoom_level, tile_x, tile_y) =>
      with_adapted_failure(async () => {
        const raster_tile_data = await generated_map_sdk.fetchRasterTile(
          tile_x,
          tile_y,
          zoom_level,
          RasterImageFormat.Png,
          RasterTileResolution.Standard,
        );
        return new Uint8Array(raster_tile_data.imageBytes);
      }),
    // The browser transport's diagnostics ring stays on the wasm-bindgen
    // glue for now; the generated listener migration follows separately.
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
