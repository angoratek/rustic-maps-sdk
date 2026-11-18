# Offline maps

Keep map imagery and routing working without a network. A disk response
cache holds raster tile imagery under your byte budget, including named
regions you download ahead of time, and a compiled routing archive returns
routes with no network round trip. Downloaded regions also have their own
pack byte budget, separate from the general cache and unbounded until you set
one. Android, iOS and React Native download regions; the browser surface has no
disk store for offline regions and keeps offline imagery in the page's own
storage.

<div class="platform-tabs">
<div class="platform-tab-titles" role="tablist">
<button type="button" data-platform-tab="android">Android</button>
<button type="button" data-platform-tab="ios">iOS</button>
<button type="button" data-platform-tab="react-native">React Native</button>
<button type="button" data-platform-tab="wasm">WebAssembly</button>
</div>

<div class="platform-tab-panel" data-platform-panel="android">

### Register the providers with a tile cache

One registration call installs the map-platform provider set and wraps the
raster-tile contract in the disk response cache:

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

The call opens or creates the SQLite response cache at the path. A refusal,
such as an unusable path or a non-positive budget, returns the reason
verbatim.

### Read and tune the caching terms

The cache starts at the tiles category's default time-to-live of 30 calendar
days, the imagery-provider terms ceiling. Tune the live value and read the
value in force:

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

A request above the provider's declared ceiling is accepted, and the
ceiling clamps what the cache retains; the readback shows the effective
value, so display it in your offline-maps settings screen.

### Set a byte budget for offline regions

A downloaded region has its own payload-byte budget beside the general cache.
Set it before you download, and read back the value in force:

```kotlin
val offlineManager = OfflineManager(mapSdk)
val packByteBudget = 64UL * 1024UL * 1024UL   // 64 MiB
offlineManager.setTilePackByteBudget(packByteBudget)
val effectivePackByteBudget = offlineManager.tilePackByteBudget()
```

Until you set one, packs have no separate byte limit and the only size limit
is the 65,536-tile structural cap. A download that would cross the budget
stops, keeps the tiles already stored, and reports a cache failure. Set a
higher budget and call `downloadRegion` again to resume the remaining tiles. A
zero budget is refused with the reason, and a missing tile cache reports that
the tile cache is not configured.

### Download a region for offline use

A region download writes every tile of a bounding box and zoom range into the
same SQLite cache the live map reads, pins those tiles against ordinary
eviction, and reports progress. Define the region first with these values.

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

`regionTileCount` validates the definition and counts the tiles before
anything is written. A maximum zoom deeper than the provider's declared
ceiling, or a region above the tile limit, throws before the download
starts.

Start the download with a listener. It receives one `Downloading` report
before the first tile, one report per completed tile, and exactly one
terminal `Completed`, `Cancelled` or `Failed` report:

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

The listener runs on an SDK worker thread, so post the UI update to the main
thread; do not call back into the manager from it.

Cancel an in-flight download with the region id:

```kotlin
offlineManager.cancelRegionDownload(region.regionId)
```

Cancellation is a partial success: the terminal phase is `Cancelled`, the
returned record keeps `presentTileCount` below `memberTileCount`, and the next
`downloadRegion` call fetches only the missing tiles.

List the regions stored on disk, read one, and delete it:

```kotlin
val storedRegions: List<RegionRecord> = offlineManager.regionRecords()
val storedRegion: RegionRecord? = offlineManager.regionRecord(region.regionId)
// storedRegion.presentTileCount, memberTileCount, presentPayloadBytes,
// effectiveTimeToLiveSeconds
val removal: RegionRemovalReport = offlineManager.deleteRegion(region.regionId)
```

### Restart without a network

Stop the app, turn off the network, and open it again. The downloaded tiles
render from disk at the zooms you downloaded, and `regionRecords()` still
returns the region with its stored tile count, payload bytes and effective
retention. Deleting the region returns the map to network rendering.

### Pre-warm the tiles you need

Cached tile responses render whenever the network is unavailable: a
cache miss goes to the network and back-fills, and a cache hit comes from
disk. Render the regions your users expect to revisit once while
online, and the cache holds them until the budget or time-to-live says
otherwise.

### Route without the network

The offline routing layer runs inside the SDK: a compiled routing
archive — OpenStreetMap extract in, self-describing archive file out —
returns routes with A\* over the archive's graph, honoring recorded turn
restrictions and deriving maneuver text from the geometry's bearing change.

The contract carries the route call on every surface. Pass the archive bytes,
not a path, so the same call works on Android, iOS, React Native, and the
browser.

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

The request uses the archive profile's default options: automobile, fastest,
metric. A corrupt archive throws `OfflineRoutingFailure`. The SDK does not
fall back to the archive when the online provider fails; catch the online
failure and call the archive route yourself. Maneuver texts stay honest where
the extract carries no street names.

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

A downloaded region has its own payload-byte budget beside the general cache.
Set it before you download, and read back the value in force:

```swift
let offlineManager = OfflineManager(mapSdk: mapSdk)
let packByteBudget: UInt64 = 64 * 1024 * 1024   // 64 MiB
try offlineManager.setTilePackByteBudget(byteBudget: packByteBudget)
let effectivePackByteBudget = try offlineManager.tilePackByteBudget()
```

Until you set one, packs have no separate byte limit and the only size limit
is the 65,536-tile structural cap. A download that would cross the budget
stops, keeps the tiles already stored, and reports a cache failure. Set a
higher budget and call `downloadRegion` again to resume the remaining tiles. A
zero budget is refused with the reason, and a missing tile cache reports that
the tile cache is not configured.

### Download a region for offline use

A region download writes every tile of a bounding box and zoom range into the
same SQLite cache the live map reads, pins those tiles against ordinary
eviction, and reports progress. Define the region first with these values.

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

`regionTileCount` validates the definition and counts the tiles before
anything is written. A maximum zoom deeper than the provider's declared
ceiling, or a region above the tile limit, throws before the download
starts.

Start the download with a listener. It receives one `Downloading` report
before the first tile, one report per completed tile, and exactly one
terminal `Completed`, `Cancelled` or `Failed` report:

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

The listener runs on an SDK worker thread, so hop to the main actor before
touching UI; do not call back into the manager from it.

Cancel an in-flight download with the region id:

```swift
offlineManager.cancelRegionDownload(regionId: region.regionId)
```

Cancellation is a partial success: the terminal phase is `.cancelled`, the
returned record keeps `presentTileCount` below `memberTileCount`, and the next
`downloadRegion` call fetches only the missing tiles.

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

### Restart without a network

Stop the app, turn off the network, and open it again. The downloaded tiles
render from disk at the zooms you downloaded, and `regionRecords()` still
returns the region with its stored tile count, payload bytes and effective
retention. Deleting the region returns the map to network rendering.

### Pre-warm and render offline

Cached tiles render whenever the network is unavailable. Render the regions
your users revisit while online; the cache serves them from disk.

### Route without the network

The offline routing layer runs inside the SDK over a compiled archive:
A\* on the archive's graph, recorded turn restrictions honored, maneuver
text derived from the geometry. The contract call takes the archive bytes,
not a path, so the same call works on every platform:

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

The request uses the archive profile's default options: automobile, fastest,
metric. A corrupt archive throws `OfflineRoutingFailure`. The SDK does not
fall back to the archive when the online provider fails; catch the online
failure and call the archive route yourself.

</div>

<div class="platform-tab-panel" data-platform-panel="react-native">

### Register the providers with a tile cache

The package's SDK handle installs the provider set and wraps the raster-tile
contract in the disk response cache:

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

The package opens (or creates) the cache under the application's caches
directory with a 16 MiB budget; JavaScript passes the provider key and the
credential, never a file path. A refusal resolves with `isApplied: false` and
the provider's verbatim `detailText`. Re-registration replaces the raster-tile
slot, cache included.

### Read and tune the caching terms

The cache starts at the tiles category's default time-to-live of 30 calendar
days, the imagery-provider terms ceiling. Tune the live value and read the
value in force:

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

A request above the provider's declared ceiling is accepted, and the ceiling
clamps what the cache retains; the readback shows the effective value, so
display it in your offline-maps settings screen.

### Set a byte budget for offline regions

A downloaded region has its own payload-byte budget beside the general cache.
Set it before you download, and read back the value in force. The React Native
generated contract binding carries `setTilePackByteBudget` and
`tilePackByteBudget` on its offline manager.

Until you set one, packs have no separate byte limit and the only size limit
is the 65,536-tile structural cap. A download that would cross the budget
stops, keeps the tiles already stored, and reports a cache failure. Set a
higher budget and call `downloadRegion` again to resume the remaining tiles. A
zero budget is rejected with the reason, and a missing tile cache reports that
the tile cache is not configured.

### Download a region for offline use

A region download writes every tile of a bounding box and zoom range into the
same SQLite cache the live map reads, pins those tiles against ordinary
eviction, and reports progress. Define the region first with these values.

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

`regionTileCount` validates the definition and counts the tiles before
anything is written. A maximum zoom deeper than the provider's declared
ceiling, or a region above the tile limit, rejects before the download
starts.

Start the download with a listener. It receives one `Downloading` report
before the first tile, one report per completed tile, and exactly one
terminal `Completed`, `Cancelled` or `Failed` report:

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

The listener runs on an SDK worker thread, so marshal the UI update in your
shell; do not call back into the manager from it.

Cancel an in-flight download with the region id:

```js
await offlineManager.cancelRegionDownload(region.regionId);
```

Cancellation is a partial success: the terminal phase is `Cancelled`, the
returned record keeps `presentTileCount` below `memberTileCount`, and the next
`downloadRegion` call fetches only the missing tiles.

List the regions stored on disk, read one, and delete it:

```js
const storedRegions = await offlineManager.regionRecords();
const storedRegion = await offlineManager.regionRecord(region.regionId);
// storedRegion.presentTileCount, memberTileCount, presentPayloadBytes,
// effectiveTimeToLiveSeconds
const removal = await offlineManager.deleteRegion(region.regionId);
```

### Restart without a network

Stop the app, turn off the network, and open it again. The downloaded tiles
render from disk at the zooms you downloaded, and `regionRecords()` still
returns the region with its stored tile count, payload bytes and effective
retention. Deleting the region returns the map to network rendering.

### Pre-warm the tiles you need

Cached tile responses render whenever the network is unavailable: a cache miss
goes to the network and back-fills, and a cache hit answers from disk. Render
the regions your users expect to revisit once while online, and the cache
holds them until the budget or time-to-live says otherwise.

### Route without the network

The compiled routing archive returns routes with A\* over its graph, honoring
recorded turn restrictions and deriving maneuver text from the geometry. The
generated bindings carry the contract's archive-route call; pass the bytes,
not a path:

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

The request uses the archive profile's default options: automobile, fastest,
metric. A corrupt archive rejects with `OfflineRoutingFailure`. The SDK does
not fall back to the archive when an online route fails; handle that failure
in your own code.

</div>

<div class="platform-tab-panel" data-platform-panel="wasm">

### What the browser surface carries

The browser surface has no SDK-owned disk store: there is no SQLite tile
cache, no byte budget, and no offline-region store. The region download,
progress, listing and deletion steps of the native tabs have no counterpart
here. Offline region downloads are not available in the browser, and the
WebAssembly TypeScript surface neither declares nor serves those members.

What the WebAssembly build carries is the routing archive: archive bytes in,
route records out, with no provider and no network round trip. The same
compiled archive serves the native platforms, so one compile feeds every
target.

A page that needs offline imagery keeps the tiles it fetched in the browser's
own storage, such as Cache Storage or IndexedDB, and serves them to the SDK
through a URL-template raster source it controls. The SDK does not download,
pin, count or delete those tiles; your page owns that lifecycle, including
when the stored tiles expire.

### Route over the archive

```js
import { offlineRouteFromArchiveBytes } from './web/rustic_maps_web_bindings.js';

const archiveResponse = await fetch('/offline-graph/routing-grid.rmog');
const archiveBytes = new Uint8Array(await archiveResponse.arrayBuffer());

const plannedRoute = await offlineRouteFromArchiveBytes(
  archiveBytes,
  originLatitude,
  originLongitude,
  endLatitude,
  endLongitude,
);
```

The result is the same `Route`/`RouteLeg`/`RouteStep` record the live route
call returns, so the route overlay and framing steps from [Search and
routing](search-and-routing.md) apply unchanged. The call rejects with the
reason when the bytes fail archive validation, a coordinate falls outside
every node's snap tolerance, or the two points are not connected in the
graph.

Where the bytes come from is the page's decision: ship the archive with the
app, fetch it once and keep it in the browser's own storage, or read it from
`<input type="file">` for a local preview.

</div>

</div>
