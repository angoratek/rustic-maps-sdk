# Provider capabilities on React Native

React Native exposes the SDK through one package-owned SDK handle and one
typed `MapView` component. The handle carries the live provider APIs, the
tile cache, and the URL-template base-map registrations; the package root
exports the navigation and offline-routing classes. The component draws the
surface and switches the vector base. This page lists the registry's
providers, the public calls each capability uses, and the code that switches
a base map or registers a provider.

## What you need

- An initialized bare React Native application on Android arm64-v8a or iOS,
  with the `@angoratek/rustic-maps-sdk` package installed.
- A provider credential only for the live provider APIs. Read it at runtime;
  never place it in the JavaScript bundle or a native build file.

## Providers available on React Native

| Provider | Key | Credential | Form on React Native | Serves |
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
are keyless: select one per capability with `configureProvider`. Photon
reaches its public instance from an empty credential; OSRM and Valhalla take
the deployment's base URL, and an empty one targets the local default.
OpenStreetMap Standard registers through the provider-neutral
URL-template raster call. OpenStreetMap US and OpenFreeMap are served by the
`MapView` component's vector base member and need no registration call.

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

| Capability | Public React Native call |
|---|---|
| Forward and reverse geocoding | `MapsSdk.forwardGeocode(addressText)`, `MapsSdk.reverseGeocode(coordinate)` |
| Suggestions and search | `MapsSdk.suggest(queryText, searchBoundary)`, `MapsSdk.search(queryText, searchBoundary)` |
| Routing with options | `MapsSdk.calculateRoute(waypoints, routeOptions)` |
| Route capabilities | `MapsSdk.routeCapabilities()` |
| Distance matrix | `MapsSdk.calculateDistanceMatrix(origins, destinations, transportationMode)` |
| Traffic incidents and flow | `MapsSdk.fetchTrafficIncidents(boundingBox)`, `MapsSdk.fetchTrafficFlow(boundingBox)` |
| Elevation | `MapsSdk.fetchElevationPoints(coordinates)`; no shipped provider serves it |
| Raster base map | `MapsSdk.registerUrlTemplateRasterTileSource(source)`; the `MapView` component draws the tiles |
| Disk tile cache | `MapsSdk.setProviderCredentialWithTileCache(providerKey, credential)`, `MapsSdk.tileCacheTerms()`, `MapsSdk.setTileCacheTimeToLive(seconds)` |
| Vector tiles and styles | `MapViewHandle.setVectorTileSource(source)`, `MapViewHandle.clearVectorTileSource()`, `MapViewHandle.getVectorTileSourceStatus()` |
| Offline routing | `offlineRouteFromArchiveBytes(...)` from the package root |
| Navigation | `Navigator`, `RouteSimulator`, `defaultStepAdvancePolicy()`, and `defaultDeviationPolicy()` from the package root |
| Provider diagnostics | `MapsSdk.latestProviderFailureDiagnostics()`, `MapsSdk.diagnosticsLines()` |
| Host-implemented provider | Not exposed by the package handle |

The package handle answers promises. A rejected promise carries the SDK's
typed failure; a registration resolves with `isApplied: false` and the
provider's verbatim `detailText`.

## Switch the base map

### A styled vector base

The registry's keyless vector base for OpenFreeMap is Bright. Install it on
the component's handle; every following frame draws the styled base beneath
your overlays.

```ts
const accepted = await mapView.setVectorTileSource({
  urlTemplate: 'https://tiles.openfreemap.org/planet/latest/{z}/{x}/{y}.pbf',
  sourceName: 'openfreemap',
  attributionText:
    'Map data © OpenStreetMap contributors (ODbL) · style Bright from OpenFreeMap styles, design CC BY 4.0',
  styleId: 'bright-light',
  minimumZoomLevel: 0,
  maximumZoomLevel: 14,
});
const status = await mapView.getVectorTileSourceStatus();
```

While a vector base is installed, the raster layer is hidden so its imagery
and labels cannot show through the style's translucent fills. The status
readback reports whether a source is configured, the style id, the source
name, the attribution line, the zoom ceiling, and the last frame's drawn and
failed tile counts. A failed count above zero with a `lastFailureDetail`
means the tileset refused some tiles; the base stays installed and the next
frame retries.

Return to the registered raster source with `clearVectorTileSource()`.

```ts
await mapView.clearVectorTileSource();
```

### A URL-template raster base

Any web-mercator XYZ raster tileset with a `{z}/{x}/{y}` template serves
without provider code. OpenStreetMap Standard is the registry's keyless
raster source.

```ts
const registration = await mapSdk.registerUrlTemplateRasterTileSource({
  urlTemplate: 'https://tile.openstreetmap.org/{z}/{x}/{y}.png',
  sourceName: 'openstreetmap',
  minimumZoomLevel: 0,
  maximumZoomLevel: 19,
  imageFormat: 0,
  tileResolution: 0,
  userAgent: 'MyMapApp/1.0 (+https://example.com)',
});
if (!registration.isApplied) {
  throw new Error(registration.detailText);
}
mapView.reloadTileSource();
```

The `imageFormat` and `tileResolution` values are the generated enum
ordinals: `imageFormat: 0` is PNG and `tileResolution: 0` is standard
resolution. The registration replaces the raster-tile slot only; geocoding,
routing, search, and traffic keep serving. It drops an installed tile cache,
because the cache terms belong to the provider that installed it. The
component resolves its raster provider when it attaches, so call
`reloadTileSource()` after the registration to make the new source
serve the next frame.

## Register a provider

The package owns one native SDK per process. Create it with
`MapsSdk.create()` and register the provider's credential.

```ts
import { MapsSdk } from '@angoratek/rustic-maps-sdk/MapsSdk';

const mapSdk = await MapsSdk.create();
const registration = await mapSdk.setProviderCredential(
  'mapquest',
  providerApiKey,
);
if (!registration.isApplied) {
  throw new Error(registration.detailText);
}
```

A refused credential resolves with `isApplied: false` and the provider's
verbatim `detailText`. Apply the credential through your settings sheet or a
launch-time automation seam; never compile it into the bundle.

The package handle's `setProviderCredential(providerKey, credential)` applies
a credential to the full-service provider's set. Every other vendor registers
one capability per call through `configureProvider`, for exactly the
contracts its descriptor declares.

```ts
import { MapsSdk, ProviderContract } from '@angoratek/rustic-maps-sdk';

const mapSdk = await MapsSdk.create();
for (const contract of [ProviderContract.Geocoding, ProviderContract.Routing]) {
  await mapSdk.configureProvider(contract, 'here', providerApiKey);
}
```

A keyless provider takes its deployment address in the credential slot; an
empty address targets the crate's local default, so a shared deployment must
be named. Photon is the exception: its empty address is the public instance.

```ts
import { MapsSdk, ProviderContract } from '@angoratek/rustic-maps-sdk';

const mapSdk = await MapsSdk.create();
await mapSdk.configureProvider(
  ProviderContract.Routing,
  'osrm',
  'https://router.project-osrm.org',
);
```

Adding an arm is covered in [Add a provider](adding-a-provider.md).

## What you should see

After the vector install, `getVectorTileSourceStatus()` reports a configured
source with the `bright-light` style id and the OpenFreeMap source name.
After the raster registration and the view reload, the next frame draws the
OpenStreetMap tiles. After a credential registration, search, routing, and
traffic answer; without one, those promises reject with the typed
`ProviderNotConfigured` failure while the keyless base still draws.

## Constraints

- Keep the package handle. Constructing the generated root's `MapsSdk`
  creates a second native SDK with its own provider registry and tile cache.
- The package handle does not expose host-implemented providers. Geocoding
  and raster tiles can be implemented in Kotlin or Swift on the native
  platforms; on React Native, use a compiled provider crate or a
  URL-template source.
- Re-registration replaces every slot on the native SDK object.
- Tests never reach the network. Provider responses in tests come from
  committed sample data served under the provider's key.

## Where to go next

- [Add a provider](adding-a-provider.md) for the two implementation routes.
- [Search and routing](search-and-routing.md) for the provider calls in a
  complete flow.
- [Offline maps](offline-maps.md) for the tile cache and archive routing.
