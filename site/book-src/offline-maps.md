# Offline maps

Keep map imagery and routing working without a network. A disk response cache
holds raster tiles under your byte budget, including named regions you
download ahead of time, and a compiled routing archive returns routes with no
network request. Regions have their own pack byte budget, separate from the
general cache. Android, iOS and React Native download regions; the browser has
no SDK-owned disk store and keeps offline imagery in the page's own storage.

## Rules that apply on the native platforms

- `setProviderCredentialWithTileCache` installs the provider set and wraps the
  raster-tile contract in a disk response cache at a path you own, under an
  absolute byte budget. A refusal, such as an unusable path or a non-positive
  budget, returns the reason verbatim.
- The cache starts at the tiles category's 30-calendar-day default time-to-live,
  the imagery-provider terms ceiling. `setRasterTileCacheTimeToLive` tunes it;
  `rasterTileCacheTerms` reads the effective value, the provider's declared
  ceiling and the category default. A higher request is accepted, and the
  ceiling clamps what the cache retains.
- A region pack has its own byte budget beside the general cache. Until you set
  one, packs have no separate byte limit and the only size cap is the
  65,536-tile structural limit. A download that would cross the budget stops,
  keeps the tiles already stored and reports a cache failure; raise the budget
  and call `downloadRegion` again to resume. A zero budget is refused with the
  reason, and a missing tile cache reports that the cache is not configured.
- A download writes every tile of a bounding box and zoom range into the same
  SQLite cache the live map reads, pins those tiles against ordinary eviction,
  and reports progress. `regionTileCount` validates the definition and counts
  tiles before anything is written; a zoom deeper than the provider's declared
  ceiling, or a region above the tile limit, throws before the download starts.
- The progress listener receives one `Downloading` report before the first
  tile, one per completed tile, and exactly one terminal `Completed`,
  `Cancelled` or `Failed` report. It runs on an SDK worker thread: post the UI
  update to the main thread and do not call back into the manager.
- Cancellation is a partial success: the terminal phase is `Cancelled`, the
  returned record keeps `presentTileCount` below `memberTileCount`, and the
  next `downloadRegion` fetches only the missing tiles.
- Stop the app, turn off the network and open it again: downloaded tiles
  render from disk at the downloaded zooms, and `regionRecords()` still returns
  the region with its stored tile count, payload bytes and effective retention.
  Deleting the region returns the map to network rendering. Cached responses
  also render whenever the network is unavailable: a miss goes to the network
  and back-fills, a hit comes from disk. Render the regions your users expect
  to revisit once while online.
- The offline routing layer runs inside the SDK over a compiled RMOG archive:
  an OpenStreetMap extract in, a self-describing archive out, A\* over the
  graph, turn restrictions honored, and maneuver text derived from the
  geometry. Pass the archive bytes, not a path, so the call works on every
  platform. The request uses the archive profile defaults: automobile,
  fastest, metric. A corrupt archive fails with `OfflineRoutingFailure`. The
  SDK does not fall back to the archive when the online provider fails; catch
  that failure and call the archive route yourself. Maneuver texts stay honest
  where the extract carries no street names.

<div class="platform-tabs">
<div class="platform-tab-titles" role="tablist">
<button type="button" data-platform-tab="android">Android</button>
<button type="button" data-platform-tab="ios">iOS</button>
<button type="button" data-platform-tab="react-native">React Native</button>
<button type="button" data-platform-tab="wasm">WebAssembly</button>
</div>

<div class="platform-tab-panel" data-platform-panel="android">

### Register the providers with a tile cache

```kotlin
val providerKey = "mapquest"
val tileCacheDatabaseFile = File(context.cacheDir, "reference-tile-cache.sqlite")
val tileCacheBudgetBytes = 16777216UL   // 16 MiB

mapSdk.setProviderCredentialWithTileCache(
    providerKey,
    providerApiKey,
    tileCacheDatabaseFile.absolutePath,
    tileCacheBudgetBytes,
)
```

### Read and tune the caching terms

```kotlin
mapSdk.setRasterTileCacheTimeToLive(timeToLiveSeconds)
val terms = mapSdk.rasterTileCacheTerms()
// terms.effectiveTimeToLiveSeconds
// terms.providerTermsCeilingSeconds
// terms.defaultTimeToLiveSeconds

// The same calls through the offline facade:
// val offlineManager = OfflineManager(mapSdk)
// offlineManager.setTileTimeToLive(timeToLiveSeconds)
// val terms = offlineManager.tileCacheTerms()
```

### Set a byte budget for offline regions

```kotlin
val offlineManager = OfflineManager(mapSdk)
val packByteBudget = 64UL * 1024UL * 1024UL   // 64 MiB
offlineManager.setTilePackByteBudget(packByteBudget)
val effectivePackByteBudget = offlineManager.tilePackByteBudget()
```

### Download a region for offline use

```kotlin
val region = RegionDefinition(
    regionId = "downtown-sacramento",
    displayName = "Downtown Sacramento",
    boundingBox = BoundingBox(
        southWest = GeographicCoordinate(latitude = 38.566, longitude = -121.508),
        northEast = GeographicCoordinate(latitude = 38.582, longitude = -121.488),
    ),
    minimumZoomLevel = 12.toUByte(),
    maximumZoomLevel = 16.toUByte(),
    imageFormat = RasterImageFormat.Png,
    tileResolution = RasterTileResolution.Standard,
    timeToLiveSeconds = 2_592_000UL,
)

val offlineManager = OfflineManager(mapSdk)
val totalTileCount = offlineManager.regionTileCount(region) // 23 tiles
```

Start the download with a listener:

```kotlin
val progressListener = object : RegionDownloadProgressListener {
    override fun onRegionDownloadProgress(progress: RegionDownloadProgress) {
        runOnUiThread {
            // progress.phase: Downloading, Completed, Cancelled or Failed
            // progress.completedTileCount / progress.totalTileCount
            // progress.networkFetchedTileCount / progress.diskServedTileCount
        }
    }
}
val record: RegionRecord = offlineManager.downloadRegion(region, progressListener)
```

Cancel an in-flight download with the region id:

```kotlin
offlineManager.cancelRegionDownload(region.regionId)
```

List the regions stored on disk, read one, and delete it:

```kotlin
val storedRegions: List<RegionRecord> = offlineManager.regionRecords()
val storedRegion: RegionRecord? = offlineManager.regionRecord(region.regionId)
// storedRegion.presentTileCount, memberTileCount, presentPayloadBytes,
// effectiveTimeToLiveSeconds
val removal: RegionRemovalReport = offlineManager.deleteRegion(region.regionId)
```

### Route without the network

```kotlin
import uniffi.rustic_maps.GeographicCoordinate
import uniffi.rustic_maps.offlineRouteFromArchiveBytes

// The compiled RMOG archive's bytes, bundled with the app or downloaded.
val archiveBytes: ByteArray = archiveFile.readBytes()
val plannedRoute = offlineRouteFromArchiveBytes(
    archiveBytes,
    GeographicCoordinate(latitude = 0.001, longitude = 0.0),
    GeographicCoordinate(latitude = 0.001, longitude = 0.002),
)
```

</div>

<div class="platform-tab-panel" data-platform-panel="ios">

### Register the providers with a tile cache

```swift
let providerKey = "mapquest"
let tileCacheDatabasePath = FileManager.default
    .urls(for: .cachesDirectory, in: .userDomainMask)[0]
    .appendingPathComponent("reference-tile-cache.sqlite")
    .path
let tileCacheBudgetBytes: UInt64 = 16_777_216   // 16 MiB

try mapSdk.setProviderCredentialWithTileCache(
    providerKey: providerKey,
    credential: providerApiKey,
    tileCacheDatabasePath: tileCacheDatabasePath,
    tileCacheAbsoluteMaximumBytes: tileCacheBudgetBytes
)
```

### Read and tune the caching terms

```swift
try mapSdk.setRasterTileCacheTimeToLive(timeToLiveSeconds: timeToLiveSeconds)
let terms = try mapSdk.rasterTileCacheTerms()
// terms.effectiveTimeToLiveSeconds
// terms.providerTermsCeilingSeconds
// terms.defaultTimeToLiveSeconds

// The same calls through the offline facade:
// let offlineManager = OfflineManager(mapSdk: mapSdk)
// try offlineManager.setTileTimeToLive(timeToLiveSeconds: timeToLiveSeconds)
// let terms = try offlineManager.tileCacheTerms()
```

### Set a byte budget for offline regions

```swift
let offlineManager = OfflineManager(mapSdk: mapSdk)
let packByteBudget: UInt64 = 64 * 1024 * 1024   // 64 MiB
try offlineManager.setTilePackByteBudget(byteBudget: packByteBudget)
let effectivePackByteBudget = try offlineManager.tilePackByteBudget()
```

### Download a region for offline use

```swift
let region = RegionDefinition(
    regionId: "downtown-sacramento",
    displayName: "Downtown Sacramento",
    boundingBox: BoundingBox(
        southWest: GeographicCoordinate(latitude: 38.566, longitude: -121.508),
        northEast: GeographicCoordinate(latitude: 38.582, longitude: -121.488)
    ),
    minimumZoomLevel: 12,
    maximumZoomLevel: 16,
    imageFormat: .png,
    tileResolution: .standard,
    timeToLiveSeconds: 2_592_000
)

let offlineManager = OfflineManager(mapSdk: mapSdk)
let totalTileCount = try offlineManager.regionTileCount(region: region) // 23 tiles
```

Start the download with a listener:

```swift
final class RegionProgressListener: RegionDownloadProgressListener {
    func onRegionDownloadProgress(progress: RegionDownloadProgress) {
        Task { @MainActor in
            // progress.phase: .downloading, .completed, .cancelled or .failed
            // progress.completedTileCount / progress.totalTileCount
            // progress.networkFetchedTileCount / progress.diskServedTileCount
        }
    }
}

let progressListener = RegionProgressListener()
let record: RegionRecord = try await offlineManager.downloadRegion(
    region: region,
    progressListener: progressListener
)
```

Cancel an in-flight download with the region id:

```swift
offlineManager.cancelRegionDownload(regionId: region.regionId)
```

List the regions stored on disk, read one, and delete it:

```swift
let storedRegions: [RegionRecord] = try await offlineManager.regionRecords()
let storedRegion: RegionRecord? = try await offlineManager.regionRecord(
    regionId: region.regionId
)
// storedRegion.presentTileCount, memberTileCount, presentPayloadBytes,
// effectiveTimeToLiveSeconds
let removal: RegionRemovalReport = try await offlineManager.deleteRegion(
    regionId: region.regionId
)
```

### Route without the network

```swift
// The compiled RMOG archive's bytes, bundled with the app or downloaded.
let archiveURL = URL(fileURLWithPath: "routing_grid.rmog")
let archiveBytes = try Data(contentsOf: archiveURL)
let plannedRoute = try await offlineRouteFromArchiveBytes(
    archiveBytes: archiveBytes,
    start: GeographicCoordinate(latitude: 0.001, longitude: 0.0),
    end: GeographicCoordinate(latitude: 0.001, longitude: 0.002)
)
```

</div>

<div class="platform-tab-panel" data-platform-panel="react-native">

### Register the providers with a tile cache

The package opens or creates the cache under the application's caches
directory with a 16 MiB budget; JavaScript passes the provider key and the
credential, never a file path. Re-registration replaces the raster-tile slot,
cache included.

```js
import { MapsSdk } from '@angoratek/rustic-maps-sdk/MapsSdk';

async function createMapSdkWithTileCache(providerKey, providerApiKey) {
  const mapSdk = await MapsSdk.create();
  const registration = await mapSdk.setProviderCredentialWithTileCache(
    providerKey,
    providerApiKey,
  );
  if (!registration.isApplied) {
    throw new Error(registration.detailText);
  }
  return mapSdk;
}
```

### Read and tune the caching terms

```js
import { MapsSdk } from '@angoratek/rustic-maps-sdk/MapsSdk';

const mapSdk = await MapsSdk.create();
const oneWeekInSeconds = 7 * 24 * 60 * 60;
await mapSdk.setTileCacheTimeToLive(oneWeekInSeconds);
const terms = await mapSdk.tileCacheTerms();
if (terms !== null) {
  // terms.effectiveTimeToLiveSeconds
  // terms.providerTermsCeilingSeconds
  // terms.defaultTimeToLiveSeconds
}
```

### Set a byte budget for offline regions

The generated contract binding carries `setTilePackByteBudget` and
`tilePackByteBudget` on its offline manager.

### Download a region for offline use

```js
import {
  RasterImageFormat,
  RasterTileResolution,
  RegionDefinition,
} from '@angoratek/rustic-maps-sdk';

const region = RegionDefinition.create({
  regionId: 'downtown-sacramento',
  displayName: 'Downtown Sacramento',
  boundingBox: {
    southWest: { latitude: 38.566, longitude: -121.508 },
    northEast: { latitude: 38.582, longitude: -121.488 },
  },
  minimumZoomLevel: 12,
  maximumZoomLevel: 16,
  imageFormat: RasterImageFormat.Png,
  tileResolution: RasterTileResolution.Standard,
  timeToLiveSeconds: 2_592_000n,
});

const totalTileCount = offlineManager.regionTileCount(region); // 23 tiles
```

Start the download with a listener:

```js
const progressListener = {
  onRegionDownloadProgress(progress) {
    // progress.phase: 'Downloading', 'Completed', 'Cancelled' or 'Failed'
    // progress.completedTileCount / progress.totalTileCount
    // progress.networkFetchedTileCount / progress.diskServedTileCount
  },
};

const record = await offlineManager.downloadRegion(region, progressListener);
```

Cancel an in-flight download with the region id:

```js
await offlineManager.cancelRegionDownload(region.regionId);
```

List the regions stored on disk, read one, and delete it:

```js
const storedRegions = await offlineManager.regionRecords();
const storedRegion = await offlineManager.regionRecord(region.regionId);
// storedRegion.presentTileCount, memberTileCount, presentPayloadBytes,
// effectiveTimeToLiveSeconds
const removal = await offlineManager.deleteRegion(region.regionId);
```

### Route without the network

```js
import { offlineRouteFromArchiveBytes } from '@angoratek/rustic-maps-sdk';

// The compiled RMOG archive's bytes, bundled with the app or downloaded.
const archiveResponse = await fetch('routing_grid.rmog');
const archiveBytes = await archiveResponse.arrayBuffer();
const plannedOfflineRoute = await offlineRouteFromArchiveBytes(
  archiveBytes,
  { latitude: 0.001, longitude: 0.0 },
  { latitude: 0.001, longitude: 0.002 },
);
```

</div>

<div class="platform-tab-panel" data-platform-panel="wasm">

### What the browser surface carries

The browser surface has no SDK-owned disk store: no SQLite tile cache, no byte
budget and no offline-region store. The region download, progress, listing and
deletion steps of the native tabs have no counterpart, and the WebAssembly
TypeScript surface neither declares nor serves those members.

The build carries the routing archive: archive bytes in, route records out,
with no provider and no network request. A page that needs offline imagery
keeps fetched tiles in the browser's own storage, such as Cache Storage or
IndexedDB, and serves them through a URL-template raster source it controls.
The SDK does not download, pin, count or delete those tiles; your page owns
that lifecycle, including when they expire.

### Route over the archive

```js
import { offlineRouteFromArchiveBytes } from './web/rustic_maps.js';

const archiveResponse = await fetch('/offline-graph/routing-grid.rmog');
const archiveBytes = await archiveResponse.arrayBuffer();

const plannedRoute = await offlineRouteFromArchiveBytes(
  archiveBytes,
  { latitude: originLatitude, longitude: originLongitude },
  { latitude: endLatitude, longitude: endLongitude },
);
```

The result is the same `Route`/`RouteLeg`/`RouteStep` record the live route
call returns, so the overlay and framing steps from [Search and
routing](search-and-routing.md) apply unchanged. The call rejects with the
reason when the bytes fail archive validation, a coordinate falls outside
every node's snap tolerance, or the two points are not connected in the graph.
Ship the archive with the app, fetch it once and keep it in browser storage,
or read it from `<input type="file">` for a local preview.

</div>

</div>
