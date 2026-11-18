# Provider capabilities on Android

Android exposes the full SDK surface: live provider APIs, keyless raster and vector base
maps, the disk tile cache, offline routing and turn-by-turn navigation. This page lists
the providers this platform can select, what each serves, the credential each needs, and
the Kotlin calls to install a base map or register a provider.

## What you need

- The Android bindings artifact and the `MapView` artifact, with `minSdk = 24`.
- A provider credential only for the live provider APIs. Read it at runtime; never place
  it in a build field, a resource or source.

## Providers available on Android

| Provider | Key | Credential | Form on Android | Serves | Attribution line | Terms |
|---|---|---|---|---|---|---|
| MapQuest | `mapquest` | API key | Live provider | Geocoding (forward and reverse), search and suggestions, routing, distance matrix, traffic incidents and flow, raster tiles | Map data © OpenStreetMap contributors (ODbL) — tile imagery © MapQuest | MapQuest terms apply: rate limits and a 30-day raster-tile caching ceiling. |
| HERE | `here` | API key | Live provider | Geocoding (forward and reverse), routing, traffic incidents and flow, vector tiles with HERE Base | © 20XX HERE, XXX | HERE terms apply: per-plan transaction limits and a caching policy that bounds stored results. |
| Google Maps | `google` | API key | Live provider | Geocoding (forward and reverse), routing | Map data © Google | Google Maps Platform terms apply: billing per SKU, caching restrictions, and no permanent storage of most content. |
| TomTom | `tomtom` | API key | Live provider | Geocoding (forward and reverse), routing, traffic incidents and flow, raster tiles | © TomTom | TomTom terms apply: per-plan request quotas and a caching policy for stored tiles and results. |
| Mapbox | `mapbox` | Access token | Live provider | Geocoding (forward and reverse), routing, vector tiles with Mapbox Streets | © Mapbox © OpenStreetMap contributors | Mapbox terms apply: usage-based billing and attribution of Mapbox and OpenStreetMap wherever the data is shown. |
| Radar | `radar` | API key | Live provider | Geocoding (forward and reverse), routing | © Radar | Radar terms apply: per-plan request quotas and no permanent storage of geocoding results. |
| Photon | `photon` | None, or a deployment URL | Selectable search provider | Geocoding (forward and reverse), search and suggestions | Map data © OpenStreetMap contributors (ODbL) — search by Photon | The public Photon instance is offered for reasonable, non-commercial use; a self-hosted deployment follows its operator's terms. |
| OpenStreetMap Standard | `openstreetmap` | None | Raster base | Raster tiles | Map data © OpenStreetMap contributors (ODbL) — raster tiles © OpenStreetMap contributors | OpenStreetMap tile usage policy applies: low-volume use only, with attribution. |
| OpenStreetMap US | `osmus` | None | Vector base | Vector tiles with Carto Light and Carto Dark | Map data © OpenStreetMap contributors (ODbL) — vector tiles © OpenStreetMap US | OpenStreetMap US tileservice terms apply: free for low-volume, non-commercial use with attribution. |
| OpenFreeMap | `openfreemap` | None | Vector base | Vector tiles with Bright, Fiord, and Positron | Map data © OpenStreetMap contributors (ODbL) — vector tiles © OpenFreeMap and OpenMapTiles | OpenFreeMap terms apply: free public use with attribution; bulk downloads are limited to 3,000 tiles or 50 MB per download. |
| OSRM | `osrm` | None, or a deployment URL | Selectable routing provider | Routing | Map data © OpenStreetMap contributors (ODbL) — routing by OSRM | A self-hosted deployment follows its operator's terms; public demo servers are rate-limited and not for production use. |
| Valhalla | `valhalla` | None, or a deployment URL | Selectable routing provider | Routing | Map data © OpenStreetMap contributors (ODbL) — routing by Valhalla | A self-hosted deployment follows its operator's terms; public demo servers are rate-limited and not for production use. |

MapQuest registers through a runtime credential. Photon, OSRM and Valhalla are keyless:
select one with `configureProvider(contract, providerKey, credential)`. Photon reaches
its public instance from an empty credential; OSRM and Valhalla take the deployment's
base URL, and an empty one targets the local default. OpenStreetMap Standard registers
through the provider-neutral URL-template raster call. OpenStreetMap US and OpenFreeMap
are served by the shipped `MapView`'s vector base member and need no registration call.

Each vector style also carries its own attribution line. The style ids are `carto-light`
and `carto-dark` (OpenStreetMap US), `bright-light`, `fiord-dark`, and `positron-light`
(OpenFreeMap), `omv-light` (HERE), and `streets-light` (Mapbox). The status readback
returns the active style's line, so display it verbatim.

HERE and Mapbox paint their vector tiles with the style document the provider publishes.
The SDK fetches that document with the registered credential and keeps it in the
installed tile cache for up to thirty days, or for the session when no tile cache is
installed, so no style file ships with your application. The Mapbox document on Android,
iOS and React Native requires a Mapbox Order.

## What you can call

| Capability | Public Kotlin call |
|---|---|
| Forward and reverse geocoding | `MapSdk.forwardGeocode(addressText)`, `MapSdk.reverseGeocode(coordinate)` |
| Suggestions and search | `MapSdk.suggest(queryText, searchBoundary)`, `MapSdk.search(queryText, searchBoundary)` |
| Routing with options | `MapSdk.calculateRoute(waypoints, routeOptions)` |
| Route capabilities | `MapSdk.routeCapabilities()` |
| Distance matrix | `MapSdk.calculateDistanceMatrix(origins, destinations, transportationMode)` |
| Traffic incidents and flow | `MapSdk.fetchTrafficIncidents(boundingBox)`, `MapSdk.fetchTrafficFlow(boundingBox)` |
| Elevation | `MapSdk.fetchElevationPoints(coordinates)`; no shipped provider serves it |
| Raster tiles | `MapSdk.fetchRasterTile(...)`, `MapSdk.registerUrlTemplateRasterTileSource(source)`; the `MapView` draws them |
| Disk tile cache | `MapSdk.setProviderCredentialWithTileCache(...)`, `MapSdk.setRasterTileCacheTimeToLive(...)`, `MapSdk.rasterTileCacheTerms()` |
| Vector tiles and styles | `MapView.setVectorTileSource(source)`, `MapView.clearVectorTileSource()`, `MapView.vectorTileSourceStatus()` |
| Offline routing | `offlineRouteFromArchiveBytes(archiveBytes, start, end)` |
| Navigation | `Navigator.startNavigation(...)`, `Navigator.updateLocation(...)`, `Navigator.addNavigationListener(...)`, `Navigator.applyReroute(...)`, `Navigator.stopNavigation()`; `RouteSimulator` |
| Host-implemented provider | `MapSdk.registerRoutingProviderFromForeign(...)` and the other `register*FromForeign` calls, one per contract |

Every provider query on `MapSdk` is a `suspend fun`; run it from a coroutine scope.
Calls into a contract with no registered provider throw `SdkException` with
`ProviderNotConfigured` and the contract name.

## Switch the base map

### A styled vector base

The registry's keyless vector base for OpenFreeMap is Bright. Install it on the shipped
view; every following frame draws the styled base beneath your overlays.

```kotlin
import uniffi.rustic_maps.UrlTemplateVectorTileSource

val openFreeMapBright = UrlTemplateVectorTileSource(
    urlTemplate = "https://tiles.openfreemap.org/planet/latest/{z}/{x}/{y}.pbf",
    sourceName = "openfreemap",
    attributionText = "Map data © OpenStreetMap contributors (ODbL) · style Bright from OpenFreeMap styles, design CC BY 4.0",
    styleId = "bright-light",
    minimumZoomLevel = 0u,
    maximumZoomLevel = 14u,
)
if (mapView.setVectorTileSource(openFreeMapBright)) {
    val status = mapView.vectorTileSourceStatus()
}
```

While a vector base is installed, the raster layer is hidden so its imagery and labels
cannot show through the style's translucent fills. The status readback reports
`isConfigured`, the style id, the source name, the attribution line, the zoom ceiling,
and the last frame's drawn and failed tile counts. A failed count above zero with a
`lastFailureDetail` means the tileset refused some tiles; the base stays installed and
the next frame retries.

Return to the registered raster source with `clearVectorTileSource()`.

```kotlin
mapView.clearVectorTileSource()
```

### A URL-template raster base

Any web-mercator XYZ raster tileset with a `{z}/{x}/{y}` template serves without
provider code. OpenStreetMap Standard is the registry's keyless raster source.

```kotlin
import uniffi.rustic_maps.RasterImageFormat
import uniffi.rustic_maps.RasterTileResolution
import uniffi.rustic_maps.UrlTemplateRasterTileSource

mapSdk.registerUrlTemplateRasterTileSource(
    UrlTemplateRasterTileSource(
        urlTemplate = "https://tile.openstreetmap.org/{z}/{x}/{y}.png",
        sourceName = "openstreetmap",
        minimumZoomLevel = 0u,
        maximumZoomLevel = 19u,
        imageFormat = RasterImageFormat.PNG,
        tileResolution = RasterTileResolution.STANDARD,
        userAgent = "MyMapApp/1.0 (+https://example.com)",
    ),
)
mapView.reloadTileSource()
```

The registration replaces the raster-tile slot only; geocoding, routing, search and
traffic keep serving. It drops an installed tile cache, because the cache terms belong
to the provider that installed it. `MapView` resolves its raster provider when it
attaches and keeps tiles by coordinate, so call `reloadTileSource()` after the
registration to make the new source serve the next frame.

## Register a provider

MapQuest registers from one runtime credential. The contract call installs the shipped
provider set with the SDK's rate-limit and retry defaults.

```kotlin
import uniffi.rustic_maps.MapSdk

val mapSdk = MapSdk()
mapSdk.configureProviderSet("mapquest", providerApiKey)
```

A refused key throws `SdkException` with the provider's verbatim detail. Apply the
credential through your settings surface or a launch-time automation seam; never compile
it into the app.

To keep registration generic over provider keys, the shipped facade takes the shared
registry's provider descriptors and registers every capability a descriptor declares.
The facade's `nativeSdk` is the one object the map view and every provider call use.

```kotlin
import com.rusticmaps.sdk.MapsSdkFacade
import com.rusticmaps.sdk.demo.ProviderRegistryLoader

val providerRegistry = ProviderRegistryLoader.load(assets)
val facade = MapsSdkFacade(providerRegistry.providers)
facade.applyProviderCredential("mapquest", providerApiKey)
val mapSdk = facade.nativeSdk
```

A keyless provider registers one capability at a time. The credential slot carries the
deployment address; an empty one targets the crate's local default, so a shared
deployment must be named.

```kotlin
mapSdk.configureProvider(
    ProviderContract.Routing,
    "osrm",
    "https://router.project-osrm.org",
)
```

A provider key the registry does not declare, or one this platform cannot register, is
refused by name rather than pretending to work. Adding a provider is covered in [Add a
provider](adding-a-provider.md).

## What you should see

After the vector install, `vectorTileSourceStatus()` reports `isConfigured = true`, the
`bright-light` style id and the OpenFreeMap source name. After the raster registration
and the view reload, the next frame draws the OpenStreetMap tiles. After a credential
registration, search, routing and traffic answer; without one, those calls throw
`SdkException` with `ProviderNotConfigured` while the keyless base still draws.

## Constraints

- `setProviderCredential` installs the shipped provider set; the provider key in the
  facade selects the registration arm, not a provider inside the SDK.
- Every contract has a host-implemented interface: `GeocodingProvider`,
  `RoutingProvider`, `DistanceMatrixProvider`, `SearchPlaceProvider`,
  `TrafficIncidentProvider`, `TrafficFlowProvider`, `ElevationProvider` and
  `RasterTileProvider`, each with its `register*FromForeign` call. Vector tiles and
  offline routing are set through the map and offline surfaces instead.
- Re-registration replaces every slot on the `MapSdk` object. A second `MapSdk` needs
  its own registration.
- Tests never reach the network. Provider responses come from committed sample data
  served under the provider's key.

## Where to go next

- [Add a provider](adding-a-provider.md) for the two implementation routes.
- [Search and routing](search-and-routing.md) for the provider calls in a complete flow.
- [Offline maps](offline-maps.md) for the tile cache and archive routing.
