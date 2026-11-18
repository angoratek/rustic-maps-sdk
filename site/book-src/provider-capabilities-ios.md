# Provider capabilities on iOS

iOS exposes the full SDK surface: live provider APIs, keyless raster and
vector base maps, the disk tile cache, offline routing and turn-by-turn
navigation. This page lists the providers the shared registry declares, the
public Swift calls each capability uses, and the code that switches a base
map or registers a provider.

## What you need

- The `RusticMapsSDK` bindings product and the `MapNativeCoreUI` product,
  with an iOS 17 deployment target.
- A provider credential only for the live provider APIs. Read it at runtime;
  never place it in `Info.plist`, an xcconfig, or source.

## Providers available on iOS

| Provider | Key | Credential | Form on iOS | Serves |
|---|---|---|---|---|
| MapQuest | `mapquest` | API key | Live provider | Geocoding (forward and reverse), search and suggestions, routing, distance matrix, traffic incidents and flow, raster tiles |
| HERE | `here` | API key | Live provider | Geocoding (forward and reverse), routing, traffic incidents and flow, vector tiles with HERE Base |
| Google Maps | `google` | API key | Live provider | Geocoding (forward and reverse), routing |
| TomTom | `tomtom` | API key | Live provider | Geocoding (forward and reverse), routing, traffic incidents and flow, raster tiles |
| Mapbox | `mapbox` | Access token | Live provider | Geocoding (forward and reverse), routing, vector tiles with Mapbox Streets |
| Radar | `radar` | API key | Live provider | Geocoding (forward and reverse), routing |
| Photon | `photon` | None, or a deployment URL | Selectable search provider | Geocoding (forward and reverse), search and suggestions |
| OpenStreetMap Standard | `openstreetmap` | None | Raster base | Raster tiles |
| OpenStreetMap US | `osmus` | None | Vector base | Vector tiles with Carto Light and Carto Dark |
| OpenFreeMap | `openfreemap` | None | Vector base | Vector tiles with Bright, Fiord, and Positron |
| OSRM | `osrm` | None, or a deployment URL | Selectable routing provider | Routing |
| Valhalla | `valhalla` | None, or a deployment URL | Selectable routing provider | Routing |

MapQuest registers through a runtime credential. Photon, OSRM and Valhalla
are keyless: select one with `configureProvider(contract:providerKey:
credential:)`. Photon reaches its public instance from an empty credential;
OSRM and Valhalla take the deployment's base URL, and an empty one targets the
local default. OpenStreetMap Standard registers through the provider-neutral
URL-template raster call. OpenStreetMap US and OpenFreeMap are served by the
shipped `MapView`'s vector base member and need no registration call.

| Provider | Attribution line | Terms |
|---|---|---|
| MapQuest | Map data © OpenStreetMap contributors (ODbL) — tile imagery © MapQuest | MapQuest terms apply: rate limits and a 30-day raster-tile caching ceiling. |
| HERE | © 20XX HERE, XXX | HERE terms apply: per-plan transaction limits and a caching policy that bounds stored results. |
| Google Maps | Map data © Google | Google Maps Platform terms apply: billing per SKU, caching restrictions, and no permanent storage of most content. |
| TomTom | © TomTom | TomTom terms apply: per-plan request quotas and a caching policy for stored tiles and results. |
| Mapbox | © Mapbox © OpenStreetMap contributors | Mapbox terms apply: usage-based billing and attribution of Mapbox and OpenStreetMap wherever the data is shown. |
| Radar | © Radar | Radar terms apply: per-plan request quotas and no permanent storage of geocoding results. |
| Photon | Map data © OpenStreetMap contributors (ODbL) — search by Photon | The public Photon instance is offered for reasonable, non-commercial use; a self-hosted deployment follows its operator's terms. |
| OpenStreetMap Standard | Map data © OpenStreetMap contributors (ODbL) — raster tiles © OpenStreetMap contributors | OpenStreetMap tile usage policy applies: low-volume use only, with attribution. |
| OpenStreetMap US | Map data © OpenStreetMap contributors (ODbL) — vector tiles © OpenStreetMap US | OpenStreetMap US tileservice terms apply: free for low-volume, non-commercial use with attribution. |
| OpenFreeMap | Map data © OpenStreetMap contributors (ODbL) — vector tiles © OpenFreeMap and OpenMapTiles | OpenFreeMap terms apply: free public use with attribution; bulk downloads are limited to 3,000 tiles or 50 MB per download. |
| OSRM | Map data © OpenStreetMap contributors (ODbL) — routing by OSRM | A self-hosted deployment follows its operator's terms; public demo servers are rate-limited and not for production use. |
| Valhalla | Map data © OpenStreetMap contributors (ODbL) — routing by Valhalla | A self-hosted deployment follows its operator's terms; public demo servers are rate-limited and not for production use. |

Each vector style also carries its own attribution line. The style ids are
`carto-light` and `carto-dark` (OpenStreetMap US), `bright-light`, `fiord-dark`,
and `positron-light` (OpenFreeMap), `omv-light` (HERE), and `streets-light`
(Mapbox). The status readback returns the active style's line, so display it
verbatim.

HERE and Mapbox paint their vector tiles with the style document the provider
publishes. The SDK fetches that document with the registered credential and
keeps it in the installed tile cache for up to thirty days, or for the session
when no tile cache is installed, so no style file ships with your application. The
Mapbox document on Android, iOS and React Native requires a Mapbox Order.

## What you can call

| Capability | Public Swift call |
|---|---|
| Forward and reverse geocoding | `MapSdk.forwardGeocode(addressText:)`, `MapSdk.reverseGeocode(coordinate:)` |
| Suggestions and search | `MapSdk.suggest(queryText:searchBoundary:)`, `MapSdk.search(queryText:searchBoundary:)` |
| Routing with options | `MapSdk.calculateRoute(waypoints:routeOptions:)` |
| Route capabilities | `MapSdk.routeCapabilities()` |
| Distance matrix | `MapSdk.calculateDistanceMatrix(origins:destinations:transportationMode:)` |
| Traffic incidents and flow | `MapSdk.fetchTrafficIncidents(boundingBox:)`, `MapSdk.fetchTrafficFlow(boundingBox:)` |
| Elevation | `MapSdk.fetchElevationPoints(coordinates:)`; no shipped provider serves it |
| Raster tiles | `MapSdk.fetchRasterTile(...)`, `MapSdk.registerUrlTemplateRasterTileSource(source:)`; the `MapView` draws them |
| Disk tile cache | `MapSdk.setProviderCredentialWithTileCache(...)`, `MapSdk.setRasterTileCacheTimeToLive(timeToLiveSeconds:)`, `MapSdk.rasterTileCacheTerms()` |
| Vector tiles and styles | `MapView.setVectorTileSource(_:)`, `MapView.clearVectorTileSource()`, `MapView.vectorTileSourceStatus()` |
| Offline routing | `offlineRouteFromArchiveBytes(archiveBytes:start:end:)` |
| Navigation | `Navigator.startNavigation(...)`, `Navigator.updateLocation(locationUpdate:)`, `Navigator.addNavigationListener(listener:)`, `Navigator.applyReroute(...)`, `Navigator.stopNavigation()`; `RouteSimulator` |
| Host-implemented provider | `MapSdk.registerRoutingProviderFromForeign(routingProvider:)` and the other `register*FromForeign` calls, one per contract |

Every provider call `throws` and awaits its provider. A call into a contract
with no registered provider throws `SdkError.providerNotConfigured` with the
contract name.

## Switch the base map

### A styled vector base

The registry's keyless vector base for OpenFreeMap is Bright. Install it on
the view; every following frame draws the styled base beneath your overlays.
The snippet calls the contract `MapView`; the shipped `MapNativeCoreUI`
view exposes the same calls as `setVectorTileSource(_:)`,
`clearVectorTileSource()`, and `vectorTileSourceStatus()`.

```swift
import RusticMapsSDK

func installOpenFreeMapBright(on mapView: RusticMapsSDK.MapView) throws {
    let source = UrlTemplateVectorTileSource(
        urlTemplate: "https://tiles.openfreemap.org/planet/latest/{z}/{x}/{y}.pbf",
        sourceName: "openfreemap",
        attributionText: "Map data © OpenStreetMap contributors (ODbL) · style Bright from OpenFreeMap styles, design CC BY 4.0",
        styleId: "bright-light",
        minimumZoomLevel: 0,
        maximumZoomLevel: 14
    )
    try mapView.setVectorTileSource(source: source)
    let status = mapView.vectorTileSourceStatus()
}
```

While a vector base is installed, the raster layer is hidden so its imagery
and labels cannot show through the style's translucent fills. The status
readback reports `isConfigured`, the style id, the source name, the
attribution line, the zoom ceiling, and the last frame's drawn and failed
tile counts. A failed count above zero with a `lastFailureDetail` means the
tileset refused some tiles; the base stays installed and the next frame
retries.

Return to the registered raster source with `clearVectorTileSource()`; the
shipped view answers whether it accepted the clear.

```swift
import RusticMapsSDK

func clearVectorBase(on mapView: RusticMapsSDK.MapView) throws {
    try mapView.clearVectorTileSource()
}
```

### A URL-template raster base

Any web-mercator XYZ raster tileset with a `{z}/{x}/{y}` template serves
without provider code. OpenStreetMap Standard is the registry's keyless
raster source.

```swift
import RusticMapsSDK

let mapSdk = MapSdk()
try mapSdk.registerUrlTemplateRasterTileSource(
    source: UrlTemplateRasterTileSource(
        urlTemplate: "https://tile.openstreetmap.org/{z}/{x}/{y}.png",
        sourceName: "openstreetmap",
        minimumZoomLevel: 0,
        maximumZoomLevel: 19,
        imageFormat: .png,
        tileResolution: .standard,
        userAgent: "MyMapApp/1.0 (+https://example.com)"
    )
)
```

The registration replaces the raster-tile slot only; geocoding, routing,
search, and traffic keep serving. It drops an installed tile cache, because
the cache terms belong to the provider that installed it. The shipped
`MapNativeCoreUI` view resolves its raster provider when it attaches and
keeps tiles by coordinate, so call `reloadTileSource()` after the
registration to make the new source serve the next frame.

## Register a provider

MapQuest registers from one runtime credential. The contract call installs
the shipped provider set with the SDK's rate-limit and retry defaults.

```swift
import RusticMapsSDK

let mapSdk = MapSdk()
try mapSdk.configureProviderSet(providerKey: "mapquest", credential: providerApiKey)
```

A refused key throws `SdkError` with the provider's verbatim detail. Apply
the credential through your settings sheet or a launch-environment
automation seam; never compile it into the app.

`setProviderCredential(credential:)` fills every capability the full-service
provider serves. Per-capability selection takes the provider key explicitly,
so one application can search with one provider and route with another. A
keyed vendor registers one capability per call for exactly the contracts its
descriptor declares.

```swift
import RusticMapsSDK

let mapSdk = MapSdk()
for contract in [ProviderContract.geocoding, .routing] {
    try mapSdk.configureProvider(
        contract: contract,
        providerKey: "here",
        credential: providerApiKey
    )
}
```

A keyless provider takes its deployment address in the credential slot; an
empty address targets the crate's local default, so a shared deployment must
be named. Photon is the exception: its empty address is the public instance.

```swift
import RusticMapsSDK

let mapSdk = MapSdk()
try mapSdk.configureProvider(
    contract: .routing,
    providerKey: "osrm",
    credential: "https://router.project-osrm.org"
)
```

A provider key with no dispatch arm is refused by name rather than pretending
to work. Adding an arm is covered in [Add a provider](adding-a-provider.md).

## What you should see

After the vector install, `vectorTileSourceStatus()` reports a configured
source with the `bright-light` style id and the OpenFreeMap source name.
After the raster registration and the view reload, the next frame draws the
OpenStreetMap tiles. After a credential registration, search, routing, and
traffic answer; without one, those calls throw
`SdkError.providerNotConfigured` while the keyless base still draws.

## Constraints

- `setProviderCredential` fills every capability the full-service provider
  serves in one call; `configureProvider(contract:providerKey:credential:)`
  selects one capability and leaves the rest serving.
- Every contract has a host-implemented interface: `GeocodingProvider`,
  `RoutingProvider`, `DistanceMatrixProvider`, `SearchPlaceProvider`,
  `TrafficIncidentProvider`, `TrafficFlowProvider`, `ElevationProvider` and
  `RasterTileProvider`, each with its `register*FromForeign` call. Vector tiles
  and offline routing are set through the map and offline surfaces instead.
- Re-registration replaces every slot on the `MapSdk` object. A second
  `MapSdk` needs its own registration.
- Tests never reach the network. Provider responses in tests come from
  committed sample data served under the provider's key.

## Where to go next

- [Add a provider](adding-a-provider.md) for the two implementation routes.
- [Search and routing](search-and-routing.md) for the provider calls in a
  complete flow.
- [Offline maps](offline-maps.md) for the tile cache and archive routing.
