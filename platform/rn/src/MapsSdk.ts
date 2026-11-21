/**
 * The package-owned `MapsSdk` proxy.
 *
 * The generated JSI bindings construct their own native SDK object, and there
 * is no way to import that foreign handle into a native view. A native
 * `<MapView>` therefore has to use the SDK object the package owns and proxy
 * every SDK call through its module — otherwise two provider registries and
 * two SQLite tile caches exist in one process. This module is that proxy:
 * one native `MapsSdk` per process, created by the package's native module,
 * with the contract's provider-credential, search and diagnostics members
 * exposed under typed names.
 *
 * The proxy deliberately does not import the generated bindings at module
 * scope: the package entry's bindings install their own Rust crate and run
 * the generated checksum gate at import time, which the package's native
 * runtime does not need.
 */

import {
  NativeModules,
  DeviceEventEmitter,
  NativeEventEmitter,
  Platform,
} from 'react-native';

import { MapsSdkError, MapsSdkErrorCode, wrapNativeFailures } from './MapsSdkError';

// The package's one typed failure is part of the public surface, beside the
// SDK handle it is thrown from.
export { MapsSdkError, MapsSdkErrorCode } from './MapsSdkError';
export type { MapsSdkErrorCodeValue } from './MapsSdkError';

import type {
  BoundingBox,
  DistanceMatrix,
  GeographicCoordinate,
  Place,
  ProviderContract,
  ProviderFailureDiagnostic,
  ProviderRegistration,
  RasterImageFormat,
  RasterTileResolution,
  Route,
  RouteCapabilities,
  RouteOptions,
  SourceHeader,
  TileCacheTerms,
  TrafficFlowData,
  TrafficIncident,
  TransportationMode,
  VectorTileSourceLayerCount,
} from './generated/rustic_maps';

// The record types the proxy answers are re-exported beside it, so a
// consumer imports one module for the SDK handle and its answer shapes.
export type {
  DistanceMatrix,
  DistanceMatrixEntry,
  ProviderContract,
  ProviderRegistration,
  Route,
  RouteCapabilities,
  RouteOptions,
  TrafficFlowData,
  TrafficIncident,
  TransportationMode,
} from './generated/rustic_maps';

/** One runtime provider credential's honest outcome. */
export interface ProviderCredentialResult {
  /** Whether the credential registered. */
  isApplied: boolean;
  /** The verbatim refusal detail (empty when the credential applied). */
  detailText: string;
}

/** One runtime SDK registration's honest outcome (a credential or a base map). */
export interface SdkRegistrationResult {
  /** Whether the registration applied. */
  isApplied: boolean;
  /** The verbatim refusal detail (empty when the registration applied). */
  detailText: string;
}

/**
 * One device position report on the package's curated surface (the
 * contract's `PositionFix`): the reported coordinate with the source's
 * optional accuracy, course and instant, carrying `null` where the source
 * reported none.
 */
export interface DevicePositionFix {
  /** The reported coordinate on the WGS 84 ellipsoid. */
  coordinate: GeographicCoordinate;
  /** Radius in meters of the 68% horizontal-accuracy envelope. */
  horizontalAccuracyMeters: number | null;
  /** Direction of travel in degrees clockwise from true north. */
  courseHeadingDegrees: number | null;
  /** The fix's instant in seconds since the Unix epoch. */
  timestampSeconds: number | null;
}

/**
 * Why a device positioning source could not produce fixes (the contract's
 * `PositionSourceError`, under its lower-camel JavaScript name): the user
 * denied the permission, location services are disabled, no provider can
 * answer, or the source did not answer inside its timeout.
 *
 * The name is deliberately distinct from the generated contract's
 * `PositionSourceError`: the package's curated surface is a lower-camel
 * string union, while the generated record is the contract's own shape. Two
 * public types with one name would make an import ambiguous.
 */
export type PositionSourceFailureReason =
  | 'permissionDenied'
  | 'serviceDisabled'
  | 'unavailable'
  | 'timeout';

/**
 * A raster image format the base-map contract serves: the generated
 * `RasterImageFormat` enum, whose zero-based values are PNG and JPEG.
 */
export type UrlTemplateRasterImageFormat = RasterImageFormat;

/**
 * A pixel resolution the base-map contract serves: the generated
 * `RasterTileResolution` enum, whose zero-based values are standard and
 * retina.
 */
export type UrlTemplateTileResolution = RasterTileResolution;

/**
 * One provider-neutral base-map tile source (the contract's
 * `UrlTemplateRasterTileSource` shape): a web-mercator XYZ raster tileset
 * addressed by a `{z}/{x}/{y}` URL template. The values come from the
 * integrator's shared provider registry, never from this package.
 *
 * The name is deliberately distinct from the generated contract record of the
 * same concept: this is the plain JSON shape the proxy sends across the
 * bridge, and a public type may not share the generated record's name.
 */
export interface UrlTemplateRasterTileSourceDefinition {
  /** The `{z}/{x}/{y}` URL template the tileset publishes. */
  urlTemplate: string;
  /** The tileset's name, used verbatim in every diagnostic it raises. */
  sourceName: string;
  /** Lowest zoom level the tileset serves. */
  minimumZoomLevel: number;
  /** Highest zoom level the tileset serves. */
  maximumZoomLevel: number;
  /** The image format the tileset serves. */
  imageFormat: UrlTemplateRasterImageFormat;
  /** The pixel resolution the tileset serves. */
  tileResolution: UrlTemplateTileResolution;
  /** The `User-Agent` every tile request carries; empty sends none. */
  userAgent: string;
  /**
   * Immediate tile requests the source admits before pacing starts; omitted
   * or `null` keeps the SDK's default of eight. Must be finite and at least
   * one.
   */
  tokenBucketCapacityTokens?: number | null;
  /**
   * Sustained tile request rate in tokens per second; omitted or `null`
   * keeps the SDK's default of eight. Must be finite and strictly positive.
   */
  refillTokensPerSecond?: number | null;
}

/**
 * One header every vector tile request carries: the contract's generated
 * `SourceHeader` record, re-exported under the curated bridge name.
 */
export type SourceHeaderDefinition = SourceHeader;

/**
 * One provider-neutral vector base-map source (the contract's
 * `UrlTemplateVectorTileSource`): a web-mercator XYZ vector tileset
 * addressed by a `{z}/{x}/{y}` URL template, painted with one of the SDK's
 * committed styles. The values come from the integrator's shared provider
 * registry, never from this package.
 */
export interface UrlTemplateVectorTileSource {
  /** The `{z}/{x}/{y}` URL template the tileset publishes. */
  urlTemplate: string;
  /** The tileset's name, used verbatim in every diagnostic it raises. */
  sourceName: string;
  /** The attribution line the tileset's terms (and its style's design) require. */
  attributionText: string;
  /** The committed style id the base paints with. */
  styleId: string;
  /** Lowest zoom level the tileset serves. */
  minimumZoomLevel: number;
  /** Highest zoom level the tileset serves. */
  maximumZoomLevel: number;
  /**
   * Immediate tile requests the source admits before pacing starts; omitted
   * or `null` keeps the SDK's default of eight. Must be finite and at least
   * one.
   */
  tokenBucketCapacityTokens?: number | null;
  /**
   * Sustained tile request rate in tokens per second; omitted or `null`
   * keeps the SDK's default of eight. Must be finite and strictly positive.
   */
  refillTokensPerSecond?: number | null;
  /**
   * Terms the tileset's usage policy requires alongside the attribution line;
   * omitted or `null` declares none.
   */
  termsText?: string | null;
  /**
   * Values substituted for the template's optional `{s}` placeholder, in the
   * source's preference order; omitted or `null` when the template has no
   * `{s}`.
   */
  subdomains?: string[] | null;
  /**
   * Headers every tile request carries, in the order given; omitted or `null`
   * sends none. A credential a public template needs belongs here or in the
   * template itself.
   */
  headers?: SourceHeaderDefinition[] | null;
  /**
   * Style names or families the tileset is designed to be rendered with, in
   * the provider's preference order; omitted or `null` when it declares none.
   */
  styleHints?: string[] | null;
}

// The vector base's layer accounting is the generated contract record; the
// proxy re-exports it so a status consumer imports every shape from here.
export type { VectorTileSourceLayerCount } from './generated/rustic_maps';

/**
 * The vector base source's status readback (the contract's
 * `VectorTileSourceStatus`): what the view draws a styled base with and
 * what its last resolved frame achieved.
 */
export interface VectorTileSourceStatus {
  isConfigured: boolean;
  styleId: string;
  sourceName: string;
  attributionText: string;
  maximumZoomLevel: number;
  drawnTileCount: number;
  failedTileCount: number;
  lastFailureDetail?: string | null;
  /** Every unsupported-property diagnostic the installed style produced. */
  unsupportedPropertyCount: number;
  /** The distinct unsupported properties, scope-qualified and sorted. */
  unsupportedPropertyNames: string[];
  /** The assets that fell back to a degraded draw, sorted. */
  degradedAssetNames: string[];
  /** Each referenced source's layer accounting for the last resolved frame. */
  sourceLayerCounts: VectorTileSourceLayerCount[];
}

/**
 * The stable rejection code the time-to-live validation carries on both
 * platforms. Callers can match it instead of parsing the message.
 */
export const TILE_CACHE_TIME_TO_LIVE_ERROR_CODE =
  MapsSdkErrorCode.InvalidTimeToLive;

/** The stable refusal for a value that is not a positive whole number. */
export const TILE_CACHE_TIME_TO_LIVE_INVALID_MESSAGE =
  'the tile-cache time-to-live must be a finite, positive whole number of seconds';

/**
 * A refused `setTileCacheTimeToLive` call.
 *
 * The native layers accept a `u64` and would trap (iOS) or wrap (Android) on
 * a non-finite, fractional or out-of-range number. The proxy refuses those
 * before they cross the bridge, and also refuses a value above the installed
 * provider's declared terms ceiling, which the cache may not retain.
 */
export class InvalidTileCacheTimeToLiveError extends MapsSdkError {
  constructor(message: string = TILE_CACHE_TIME_TO_LIVE_INVALID_MESSAGE) {
    super(MapsSdkErrorCode.InvalidTimeToLive, message);
    this.name = 'InvalidTileCacheTimeToLiveError';
  }
}

/**
 * Refuses a time-to-live that is not a finite, positive whole number of
 * seconds. `Number.isSafeInteger` rejects NaN, infinity, fractions and
 * values beyond the exactly representable integer range.
 */
function assertValidTimeToLive(timeToLiveSeconds: number): void {
  if (
    !Number.isSafeInteger(timeToLiveSeconds) ||
    timeToLiveSeconds <= 0
  ) {
    throw new InvalidTileCacheTimeToLiveError();
  }
}

/** The stable refusal for a value above the provider's terms ceiling. */
function timeToLiveAboveTermsCeilingMessage(ceilingSeconds: bigint): string {
  return (
    "the tile-cache time-to-live exceeds the provider's terms ceiling of " +
    `${ceilingSeconds.toString()} seconds`
  );
}

/**
 * The encoded image format a stored region's tiles carry: the generated
 * `RasterImageFormat` enum, whose zero-based values are PNG and JPEG.
 */
export type RegionImageFormat = RasterImageFormat;

/**
 * The pixel resolution a stored region's tiles carry: the generated
 * `RasterTileResolution` enum, whose zero-based values are standard and
 * retina.
 */
export type RegionTileResolution = RasterTileResolution;

/** The phase of one region download (the contract's lower-camel name). */
export type RegionDownloadPhase =
  | 'downloading'
  | 'completed'
  | 'cancelled'
  | 'failed';

/**
 * One offline region selection: the identity the region is stored under, the
 * area it covers, the zoom range and tile form its stored tiles carry, and the
 * retention the application requests.
 */
export interface RegionDefinition {
  /** Stable identifier the region is stored and managed under. */
  regionId: string;
  /** Human-readable name for region-listing surfaces. */
  displayName: string;
  /** The region's bounding box; boxes never span the antimeridian. */
  boundingBox: BoundingBox;
  /** Lowest zoom level downloaded; ranges download coarse-first. */
  minimumZoomLevel: number;
  /** Highest zoom level downloaded. */
  maximumZoomLevel: number;
  /** Encoded format the stored tile payloads carry. */
  imageFormat: RegionImageFormat;
  /** Pixel resolution the stored tile payloads carry. */
  tileResolution: RegionTileResolution;
  /**
   * Requested retention in seconds; null or omitted takes the raster-tiles
   * category default, clamped to the provider's declared ceiling. Zero is
   * refused.
   */
  timeToLiveSeconds?: number | null;
}

/** One offline region as it stands on disk. */
export interface RegionRecord {
  /** The region's identifier. */
  regionId: string;
  /** The region's display name. */
  displayName: string;
  /** The recorded area the region covers. */
  boundingBox: BoundingBox;
  /** The recorded lowest zoom level. */
  minimumZoomLevel: number;
  /** The recorded highest zoom level. */
  maximumZoomLevel: number;
  /** The recorded encoded format. */
  imageFormat: RegionImageFormat;
  /** The recorded pixel resolution. */
  tileResolution: RegionTileResolution;
  /** The retention the region's entries carry, in seconds. */
  effectiveTimeToLiveSeconds: number;
  /** Tile fingerprints the region claims membership for. */
  memberTileCount: number;
  /** Members whose entries are live on disk right now. */
  presentTileCount: number;
  /** Payload bytes of the live members. */
  presentPayloadBytes: number;
}

/**
 * One progress report of a region download. The counts are the download's
 * own honest totals, distinguishing tiles fetched from the provider from
 * tiles the store already held.
 */
export interface RegionDownloadProgress {
  /** The region this report belongs to. */
  regionId: string;
  /** Tiles the region enumerates. */
  totalTileCount: number;
  /** Tiles downloaded and refreshed so far. */
  completedTileCount: number;
  /** Completed tiles the provider had to fetch. */
  networkFetchedTileCount: number;
  /** Completed tiles the store already held. */
  diskServedTileCount: number;
  /** The phase this report describes. */
  phase: RegionDownloadPhase;
}

/** What a region's explicit deletion removed. */
export interface RegionRemovalReport {
  /** Tile entries deleted; never a tile another live region still claims. */
  removedEntryCount: number;
  /** Payload bytes those entries carried. */
  removedPayloadBytes: number;
}

/**
 * The serving traffic-flow cache's counters beside the budget they obey
 * (`TrafficFlowCacheStatistics` in the contract). The console's cache summary
 * reads them; the hits and misses prove a repeated pane was answered without
 * asking the provider.
 */
export interface TrafficFlowCacheStatistics {
  /** Requests answered entirely from fresh cells. */
  hitCount: number;
  /** Requests that had to ask the provider for at least one cell. */
  missCount: number;
  /** Cells currently held. */
  entryCount: number;
  /** Cells dropped to stay inside the entry budget. */
  evictionCount: number;
  /** Flow segments currently held across every cell. */
  segmentCount: number;
  /** Zoom level the cells are keyed at. */
  gridZoomLevel: number;
  /** Cells the cache may hold before evicting. */
  maximumEntryCount: number;
  /** Seconds a stored cell stays fresh. */
  timeToLiveSeconds: number;
}

/**
 * The installed tile payload cache's exact counters and occupancy
 * (`TileCacheStatistics` in the contract). Both the raster base and the
 * vector base fetch through the same store, so one counter pair covers every
 * payload lookup.
 */
export interface TileCacheStatistics {
  /** Payload lookups the store answered without a provider request. */
  hitCount: number;
  /** Payload lookups the store could not answer. */
  missCount: number;
  /** Bytes currently stored; absent when the store could not be read. */
  storedByteCount?: number;
}

/** Receives one progress report of a region download. */
export type RegionDownloadProgressListener = (
  progress: RegionDownloadProgress,
) => void;

/** The named event the native halves emit region progress on. */
const offlineRegionProgressEventName = 'MapsSdkOfflineRegionEvent';

/** The wire shape the native halves emit for one progress report. */
interface RegionProgressEventPayload {
  regionId: string;
  totalTileCount: number;
  completedTileCount: number;
  networkFetchedTileCount: number;
  diskServedTileCount: number;
  phase: number;
}

/** The generated phase enum's zero-based ordinal, as the contract's name. */
function regionDownloadPhaseFromOrdinal(
  phaseOrdinal: number,
): RegionDownloadPhase {
  switch (phaseOrdinal) {
    case 0:
      return 'downloading';
    case 1:
      return 'completed';
    case 2:
      return 'cancelled';
    case 3:
      return 'failed';
    default:
      throw new Error(
        `unknown region download phase ordinal ${phaseOrdinal}`,
      );
  }
}

/** Decodes one native progress event into the typed report. */
function parseRegionDownloadProgress(
  payload: RegionProgressEventPayload,
): RegionDownloadProgress {
  return {
    regionId: payload.regionId,
    totalTileCount: payload.totalTileCount,
    completedTileCount: payload.completedTileCount,
    networkFetchedTileCount: payload.networkFetchedTileCount,
    diskServedTileCount: payload.diskServedTileCount,
    phase: regionDownloadPhaseFromOrdinal(payload.phase),
  };
}

/**
 * Encodes one region definition as the camel-case JSON the native bridge
 * parses. `timeToLiveSeconds` is sent as null when the caller omits it, so
 * the contract's nullable `u64` is explicit on the wire.
 */
function serializeRegionDefinition(region: RegionDefinition): string {
  return JSON.stringify({
    regionId: region.regionId,
    displayName: region.displayName,
    boundingBox: region.boundingBox,
    minimumZoomLevel: region.minimumZoomLevel,
    maximumZoomLevel: region.maximumZoomLevel,
    imageFormat: region.imageFormat,
    tileResolution: region.tileResolution,
    timeToLiveSeconds: region.timeToLiveSeconds ?? null,
  });
}

/** Decodes one native region record into the typed record. */
function parseRegionRecord(encodedRecord: string): RegionRecord {
  return JSON.parse(encodedRecord) as RegionRecord;
}

/**
 * Subscribes to the package's region-progress event channel.
 *
 * Android's device-event emitter delivers unconditionally; the iOS
 * `RCTEventEmitter` only emits when a JS listener is registered on the module
 * itself, so the subscription goes through `NativeEventEmitter` there (the
 * same platform split the package's map view uses).
 */
function subscribeToRegionProgress(
  handler: (payload: RegionProgressEventPayload) => void,
): () => void {
  const module = nativeModuleOrNull();
  if (Platform.OS === 'ios' && module !== null) {
    const moduleEmitter = new NativeEventEmitter(
      module as unknown as {
        addListener(eventName: string): void;
        removeListeners(count: number): void;
      },
    );
    const subscription = moduleEmitter.addListener(
      offlineRegionProgressEventName,
      handler as (event: unknown) => void,
    );
    return () => subscription.remove();
  }
  const subscription = DeviceEventEmitter.addListener(
    offlineRegionProgressEventName,
    handler,
  );
  return () => subscription.remove();
}

/** The native module surface the proxy calls. */
interface MapsSdkNativeModule {
  initialize(): Promise<void>;
  getCompiledVersion(): Promise<string>;
  defaultProviderKey(): Promise<string>;
  setProviderCredential(
    providerKey: string,
    credential: string,
  ): Promise<ProviderCredentialResult>;
  setProviderCredentialWithTileCache(
    providerKey: string,
    credential: string,
  ): Promise<ProviderCredentialResult>;
  configureProvider(
    contract: number,
    providerKey: string,
    credential: string,
  ): Promise<void>;
  providerRegistrations(): Promise<string>;
  routeCapabilities(): Promise<string>;
  registerUrlTemplateRasterTileSource(
    sourceJson: string,
  ): Promise<SdkRegistrationResult>;
  restoreCredentialedBaseMap(providerKey: string): Promise<SdkRegistrationResult>;
  search(queryText: string, searchBoundaryJson: string | null): Promise<string>;
  suggest(queryText: string, searchBoundaryJson: string | null): Promise<string>;
  forwardGeocode(addressText: string): Promise<string>;
  calculateRoute(
    waypointsJson: string,
    routeOptionsJson: string,
  ): Promise<string>;
  reverseGeocode(coordinateJson: string): Promise<string>;
  calculateDistanceMatrix(
    originsJson: string,
    destinationsJson: string,
    transportationMode: number,
  ): Promise<string>;
  fetchTrafficIncidents(boundingBoxJson: string): Promise<string>;
  fetchTrafficFlow(boundingBoxJson: string): Promise<string>;
  trafficFlowCacheStatistics(): Promise<string>;
  tileCacheStatistics(): Promise<string>;
  tileCacheTerms(): Promise<string>;
  setTileCacheTimeToLive(timeToLiveSeconds: number): Promise<void>;
  latestProviderFailureDiagnostics(): Promise<string>;
  diagnosticsLines(): Promise<string[]>;
  startDeviceLocation(): Promise<boolean>;
  stopDeviceLocation(): Promise<boolean>;
  latestDeviceFix(): Promise<string | null>;
  deviceLocationSmoothedCourseDegrees(): Promise<number | null>;
  deviceLocationSourceStatus(): Promise<PositionSourceFailureReason | null>;
  regionTileCount(regionJson: string): Promise<number>;
  downloadRegion(regionJson: string): Promise<string>;
  cancelRegionDownload(regionId: string): Promise<void>;
  regionRecords(): Promise<string>;
  regionRecord(regionId: string): Promise<string | null>;
  deleteRegion(regionId: string): Promise<string>;
  addListener(eventName: string): void;
  removeListeners(count: number): void;
}

/** The package's native module, or null when the native half is absent. */
function nativeModuleOrNull(): MapsSdkNativeModule | null {
  const module = (
    NativeModules as Record<string, MapsSdkNativeModule | undefined>
  ).MapsSdkNative ?? null;
  return module === null ? null : wrapNativeFailures(module);
}

/**
 * The native module, or the package's typed missing-native refusal.
 *
 * Every operation other than `create` and `isAvailable` requires the native
 * half; a JavaScript-only environment therefore gets one explicit, catchable
 * `MapsSdkError` (`MapsSdkNativeUnavailable`) instead of a different neutral
 * value per method.
 */
function requireNativeModule(): MapsSdkNativeModule {
  const module = nativeModuleOrNull();
  if (module === null) {
    throw MapsSdkError.nativeUnavailable();
  }
  return module;
}

/** Decodes the native JSON answer into typed records. */
function parseRecords<RecordType>(encodedRecords: string): Array<RecordType> {
  return JSON.parse(encodedRecords) as Array<RecordType>;
}

/** The one package-owned SDK handle every `MapsSdk` value proxies. */
let sharedSdk: MapsSdk | null = null;

/**
 * The package-owned SDK root. There is one native instance per process; every
 * `MapsSdk` value is a handle to it, so repeated construction never opens a
 * second provider registry or tile cache.
 */
export class MapsSdk {
  private constructor() {}

  /**
   * Starts the package's native SDK keyless (a no-key state is first-class)
   * and answers its handle. The native start also applies the launch-argument
   * automation credential seam (Android `providerApiKey`/`providerKey`
   * intent extras, iOS `PROVIDER_API_KEY`/`PROVIDER_KEY` environment
   * variables) through the public registration path; the injection is never
   * persisted and never logged.
   *
   * Without the native half it still answers a handle, so a JavaScript-only
   * test can import the surface; every operation that needs the native SDK
   * then throws `MapsSdkError` with code `MapsSdkNativeUnavailable` instead of
   * returning a per-method neutral value.
   */
  static async create(): Promise<MapsSdk> {
    if (sharedSdk !== null) {
      return sharedSdk;
    }
    const module = nativeModuleOrNull();
    if (module !== null) {
      await module.initialize();
    }
    sharedSdk = new MapsSdk();
    return sharedSdk;
  }

  /** Whether the package's native half is present (jest renders without it). */
  static isAvailable(): boolean {
    return nativeModuleOrNull() !== null;
  }

  /** Starts the native SDK if `create` has not run yet; idempotent. */
  async initialize(): Promise<void> {
    await requireNativeModule().initialize();
  }

  /** The compiled SDK version, read through the package's native runtime. */
  async getCompiledVersion(): Promise<string> {
    return requireNativeModule().getCompiledVersion();
  }

  /**
   * The provider key a launch-injected credential targets when the injection
   * names none: the provider the registration dispatch serves.
   */
  async defaultProviderKey(): Promise<string> {
    return requireNativeModule().defaultProviderKey();
  }

  /** Registers one runtime provider credential (no tile cache). */
  async setProviderCredential(
    providerKey: string,
    credential: string,
  ): Promise<ProviderCredentialResult> {
    const module = requireNativeModule();
    return module.setProviderCredential(providerKey, credential);
  }

  /**
   * Registers one runtime provider credential with the tile cache wrapped
   * around the raster-tile contract (the offline-maps demo shape).
   */
  async setProviderCredentialWithTileCache(
    providerKey: string,
    credential: string,
  ): Promise<ProviderCredentialResult> {
    const module = requireNativeModule();
    return module.setProviderCredentialWithTileCache(providerKey, credential);
  }

  /**
   * Chooses the provider that serves one capability, leaving every other
   * capability's slot untouched (`MapSdk.configureProvider`). A credentialed
   * provider takes the credential the session already holds; a keyless one
   * takes an empty string. A refusal rejects with the SDK's own message
   * verbatim and leaves the previously serving provider in place.
   */
  async configureProvider(
    contract: ProviderContract,
    providerKey: string,
    credential: string,
  ): Promise<void> {
    const module = requireNativeModule();
    await module.configureProvider(contract, providerKey, credential);
  }

  /**
   * Reports which provider serves each capability, in the SDK's stable order.
   * Every capability appears, including the ones with no provider installed
   * (an empty key and display name with `registered` false).
   */
  async providerRegistrations(): Promise<ProviderRegistration[]> {
    const module = requireNativeModule();
    return parseRecords<ProviderRegistration>(await module.providerRegistrations());
  }

  /**
   * Reports which transport modes and which route strategy the serving
   * routing provider can honour. Rejects with `ProviderNotConfigured` when no
   * routing provider is installed.
   */
  async routeCapabilities(): Promise<RouteCapabilities> {
    const module = requireNativeModule();
    return JSON.parse(await module.routeCapabilities()) as RouteCapabilities;
  }

  /**
   * Registers one provider-neutral base map (M9's native base-map switch):
   * the raster-tile slot is replaced — a tile-cache installation with it —
   * and the credential-registered contracts (geocoding, routing, search,
   * traffic) keep serving whichever base map is active. A malformed
   * template, a blank source name or an inverted zoom range is refused
   * verbatim; the previously serving source stays installed.
   */
  async registerUrlTemplateRasterTileSource(
    source: UrlTemplateRasterTileSourceDefinition,
  ): Promise<SdkRegistrationResult> {
    const module = requireNativeModule();
    return module.registerUrlTemplateRasterTileSource(JSON.stringify(source));
  }

  /**
   * Re-registers the applied provider credential so its raster-tile slot —
   * and the tile cache installed with it — serves the base map again: the
   * switch back from a credential-less source. The credential never leaves
   * the native runtime; JavaScript only names the provider. Refuses honestly
   * when no credential has been applied in this session.
   */
  async restoreCredentialedBaseMap(
    providerKey: string,
  ): Promise<SdkRegistrationResult> {
    const module = requireNativeModule();
    return module.restoreCredentialedBaseMap(providerKey);
  }

  /** Full provider search results for a settled query. */
  async search(
    queryText: string,
    searchBoundary?: BoundingBox | null,
  ): Promise<Place[]> {
    const module = requireNativeModule();
    return parseRecords<Place>(
      await module.search(
        queryText,
        searchBoundary === undefined || searchBoundary === null
          ? null
          : JSON.stringify(searchBoundary),
      ),
    );
  }

  /** Fast completion candidates while the user types. */
  async suggest(
    queryText: string,
    searchBoundary?: BoundingBox | null,
  ): Promise<Place[]> {
    const module = requireNativeModule();
    return parseRecords<Place>(
      await module.suggest(
        queryText,
        searchBoundary === undefined || searchBoundary === null
          ? null
          : JSON.stringify(searchBoundary),
      ),
    );
  }

  /** Resolves a free-form address text into candidate places. */
  async forwardGeocode(addressText: string): Promise<Place[]> {
    const module = requireNativeModule();
    return parseRecords<Place>(await module.forwardGeocode(addressText));
  }

  /**
   * Calculates a route through the ordered waypoints with the given options;
   * the route answers the total distance and duration, the geometry's
   * bounding box and one leg per waypoint pair.
   */
  async calculateRoute(
    waypoints: ReadonlyArray<GeographicCoordinate>,
    routeOptions: RouteOptions,
  ): Promise<Route> {
    const module = requireNativeModule();
    return JSON.parse(
      await module.calculateRoute(
        JSON.stringify(waypoints),
        JSON.stringify(routeOptions),
      ),
    ) as Route;
  }

  /**
   * Calculates the contract's travel-time matrix for every origin-destination
   * pair. The native side parses the JSON and answers the generated
   * `DistanceMatrix` record.
   */
  async calculateDistanceMatrix(
    origins: ReadonlyArray<GeographicCoordinate>,
    destinations: ReadonlyArray<GeographicCoordinate>,
    transportationMode: TransportationMode,
  ): Promise<DistanceMatrix> {
    const module = requireNativeModule();
    return JSON.parse(
      await module.calculateDistanceMatrix(
        JSON.stringify(origins),
        JSON.stringify(destinations),
        transportationMode,
      ),
    ) as DistanceMatrix;
  }

  /** Fetches traffic incidents intersecting the bounding box. */
  async fetchTrafficIncidents(boundingBox: BoundingBox): Promise<TrafficIncident[]> {
    const module = requireNativeModule();
    return parseRecords<TrafficIncident>(
      await module.fetchTrafficIncidents(JSON.stringify(boundingBox)),
    );
  }

  /** Fetches per-segment traffic flow inside the bounding box. */
  async fetchTrafficFlow(boundingBox: BoundingBox): Promise<TrafficFlowData[]> {
    const module = requireNativeModule();
    return parseRecords<TrafficFlowData>(
      await module.fetchTrafficFlow(JSON.stringify(boundingBox)),
    );
  }

  /**
   * Reads the serving traffic-flow cache's counters, or null when the serving
   * provider reports no cache (a foreign provider serves uncached).
   */
  async trafficFlowCacheStatistics(): Promise<TrafficFlowCacheStatistics | null> {
    const module = requireNativeModule();
    const encodedStatistics = await module.trafficFlowCacheStatistics();
    if (encodedStatistics === 'null' || encodedStatistics.trim() === '') {
      return null;
    }
    return JSON.parse(encodedStatistics) as TrafficFlowCacheStatistics;
  }

  /**
   * Reads the installed tile payload cache's exact counters, or null when no
   * tile cache is installed (a keyless session). Both the raster base and the
   * vector base fetch through the same store, so the counters cover every
   * payload lookup.
   */
  async tileCacheStatistics(): Promise<TileCacheStatistics | null> {
    const module = requireNativeModule();
    const encodedStatistics = await module.tileCacheStatistics();
    if (encodedStatistics === 'null' || encodedStatistics.trim() === '') {
      return null;
    }
    return JSON.parse(encodedStatistics) as TileCacheStatistics;
  }

  /** Resolves a coordinate into candidate places describing that position. */
  async reverseGeocode(coordinate: {
    latitude: number;
    longitude: number;
  }): Promise<Place[]> {
    const module = requireNativeModule();
    return parseRecords<Place>(
      await module.reverseGeocode(JSON.stringify(coordinate)),
    );
  }

  /** The installed raster-tile cache's effective terms. */
  async tileCacheTerms(): Promise<TileCacheTerms | null> {
    const module = requireNativeModule();
    // JSON carries the u64 fields as numbers; the contract type keeps bigint.
    const parsedTerms = JSON.parse(await module.tileCacheTerms()) as {
      effectiveTimeToLiveSeconds: number;
      providerTermsCeilingSeconds?: number;
      defaultTimeToLiveSeconds: number;
    };
    const providerTermsCeilingSeconds =
      parsedTerms.providerTermsCeilingSeconds;
    return {
      effectiveTimeToLiveSeconds: BigInt(parsedTerms.effectiveTimeToLiveSeconds),
      providerTermsCeilingSeconds:
        providerTermsCeilingSeconds === undefined
          ? undefined
          : BigInt(providerTermsCeilingSeconds),
      defaultTimeToLiveSeconds: BigInt(parsedTerms.defaultTimeToLiveSeconds),
    };
  }

  /**
   * Overrides the installed tile cache's time-to-live, in seconds.
   *
   * Rejects with {@link InvalidTileCacheTimeToLiveError} when the value is
   * not a finite, positive whole number of seconds, or when it exceeds the
   * installed provider's declared terms ceiling. The readback of the terms
   * requires an installed tile cache, exactly like the override itself, so a
   * keyless session rejects with the SDK's honest not-configured failure.
   */
  async setTileCacheTimeToLive(timeToLiveSeconds: number): Promise<void> {
    assertValidTimeToLive(timeToLiveSeconds);
    const module = requireNativeModule();
    const terms = await this.tileCacheTerms();
    const ceiling = terms?.providerTermsCeilingSeconds;
    if (ceiling !== undefined && BigInt(timeToLiveSeconds) > ceiling) {
      throw new InvalidTileCacheTimeToLiveError(
        timeToLiveAboveTermsCeilingMessage(ceiling),
      );
    }
    await module.setTileCacheTimeToLive(timeToLiveSeconds);
  }

  /**
   * Answers how many tiles the region enumerates, so a settings surface can
   * warn about a large download before starting it. Nothing is written.
   * Rejects with the SDK's own `TileCache` not-configured failure when no
   * tile cache is installed, and with `InvalidCoordinateInput` for an invalid
   * definition.
   */
  async regionTileCount(region: RegionDefinition): Promise<number> {
    const module = requireNativeModule();
    return module.regionTileCount(serializeRegionDefinition(region));
  }

  /**
   * Downloads the region's tiles into the installed cache and answers the
   * region's record as it stands on disk afterwards. Progress reports reach
   * `progressListener` while the download runs; the listener is detached when
   * the download reports a terminal phase or the promise rejects.
   *
   * The download is resumable: a failure or a cancellation keeps the tiles
   * that did arrive, and the next call fetches only the missing ones. A
   * cancelled download resolves with its partial record and a terminal
   * `cancelled` progress report, not an error.
   */
  async downloadRegion(
    region: RegionDefinition,
    progressListener?: RegionDownloadProgressListener,
  ): Promise<RegionRecord> {
    const module = requireNativeModule();
    let isListenerDetached = false;
    let unsubscribe: () => void = () => undefined;
    const detachListener = () => {
      if (!isListenerDetached) {
        isListenerDetached = true;
        unsubscribe();
      }
    };
    unsubscribe = subscribeToRegionProgress(payload => {
      if (payload.regionId !== region.regionId) {
        return;
      }
      const progress = parseRegionDownloadProgress(payload);
      progressListener?.(progress);
      if (progress.phase !== 'downloading') {
        detachListener();
      }
    });
    try {
      return parseRegionRecord(
        await module.downloadRegion(serializeRegionDefinition(region)),
      );
    } catch (downloadFailure) {
      detachListener();
      throw downloadFailure;
    }
  }

  /**
   * Cancels the region's in-flight download if one exists. Cancellation is
   * idempotent and a successful partial result; the partial region is kept
   * and resumable. A no-op for an unknown, finished or already cancelled
   * region.
   */
  async cancelRegionDownload(regionId: string): Promise<void> {
    const module = requireNativeModule();
    await module.cancelRegionDownload(regionId);
  }

  /**
   * Lists every region stored on disk, in the store's listing order. Rejects
   * with the SDK's `TileCache` not-configured failure when no tile cache is
   * installed.
   */
  async regionRecords(): Promise<RegionRecord[]> {
    const module = requireNativeModule();
    return JSON.parse(await module.regionRecords()) as RegionRecord[];
  }

  /**
   * Reads one stored region's record, or null when no region is stored under
   * the identifier.
   */
  async regionRecord(regionId: string): Promise<RegionRecord | null> {
    const module = requireNativeModule();
    const encodedRecord = await module.regionRecord(regionId);
    return encodedRecord === null ? null : parseRegionRecord(encodedRecord);
  }

  /**
   * Deletes the region's stored tiles and its membership, and answers what
   * was removed. A tile another live region still claims is kept.
   */
  async deleteRegion(regionId: string): Promise<RegionRemovalReport> {
    const module = requireNativeModule();
    return JSON.parse(
      await module.deleteRegion(regionId),
    ) as RegionRemovalReport;
  }

  /** The retained provider-failure diagnostics, oldest first. */
  async latestProviderFailureDiagnostics(): Promise<
    ProviderFailureDiagnostic[]
  > {
    const module = requireNativeModule();
    const parsedDiagnostics = parseRecords<{
      timestampMillisecondsSinceEpoch: number;
      failureClassification: ProviderFailureDiagnostic['failureClassification'];
      providerContractName: string;
      providerMethodName: string;
      failureDetail: string;
    }>(await module.latestProviderFailureDiagnostics());
    return parsedDiagnostics.map(diagnostic => ({
      ...diagnostic,
      timestampMillisecondsSinceEpoch: BigInt(
        diagnostic.timestampMillisecondsSinceEpoch,
      ),
    }));
  }

  /** The SDK's read-only diagnostics lines (version, cache, provider state). */
  async diagnosticsLines(): Promise<string[]> {
    return requireNativeModule().diagnosticsLines();
  }

  /**
   * Starts the host's device-location source, which feeds the contract's
   * position controller. Answers false when the source cannot start (a
   * missing permission, disabled location services or no available
   * provider), and the host records the matching source error. The platform
   * permission prompt is the JavaScript caller's responsibility:
   * `PermissionsAndroid` on Android and the host's authorization request on
   * iOS.
   */
  async startDeviceLocation(): Promise<boolean> {
    return requireNativeModule().startDeviceLocation();
  }

  /**
   * Stops the host's device-location source and clears it. Answers true when
   * a source was running; the controller's latest fix and source status stay
   * readable.
   */
  async stopDeviceLocation(): Promise<boolean> {
    return requireNativeModule().stopDeviceLocation();
  }

  /** The controller's latest accepted fix, or null before the first one. */
  async latestDeviceFix(): Promise<DevicePositionFix | null> {
    const encodedFix = await requireNativeModule().latestDeviceFix();
    if (encodedFix === undefined || encodedFix === null) {
      return null;
    }
    return JSON.parse(encodedFix) as DevicePositionFix;
  }

  /**
   * The smoothed course across the most recent fixes that reported one, in
   * degrees clockwise from true north, or null when none carried a course.
   */
  async deviceLocationSmoothedCourseDegrees(): Promise<number | null> {
    return requireNativeModule().deviceLocationSmoothedCourseDegrees();
  }

  /**
   * The last device-location source failure, or null when the last event was
   * a fix (or no source has run yet).
   */
  async deviceLocationSourceStatus(): Promise<PositionSourceFailureReason | null> {
    return requireNativeModule().deviceLocationSourceStatus();
  }
}

/** The shared package SDK handle (created lazily by `MapsSdk.create`). */
export const mapsSdk = {
  create: MapsSdk.create,
  isAvailable: MapsSdk.isAvailable,
};
