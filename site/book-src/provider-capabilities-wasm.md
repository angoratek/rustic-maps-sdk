# Provider capabilities on WebAssembly

WebAssembly exposes the browser surface: live provider calls over `fetch`, a
styled vector base painted by the canvas, the offline routing archive, and
the navigation driving core. It has no native view and no disk cache. This
page lists the registry's providers, the public JavaScript calls each
capability uses, and the code that switches a base map or registers a
provider.

## What you need

- A Chromium-based browser with WebGPU, and the WebAssembly package served
  from the page's own origin over HTTPS or localhost.
- A provider credential only for the live provider APIs. Read it at runtime;
  never place it in the bundle.

## Providers available on WebAssembly

| Provider | Key | Credential | Form on WebAssembly | Serves |
|---|---|---|---|---|
| MapQuest | `mapquest` | API key | Live provider | Geocoding, reverse geocoding, search and suggestions, routing with its capability readback, distance matrix, traffic incidents and flow, raster tiles |
| HERE | `here` | API key | Live provider | Geocoding, reverse geocoding, routing, traffic incidents and flow, vector tiles with HERE Base |
| Google Maps | `google` | API key | Live provider | Geocoding, reverse geocoding, routing |
| TomTom | `tomtom` | API key | Live provider | Geocoding, reverse geocoding, routing, traffic incidents and flow, raster tiles |
| Mapbox | `mapbox` | Access token | Live provider | Geocoding, reverse geocoding, routing, vector tiles with Mapbox Streets |
| Radar | `radar` | API key | Live provider | Geocoding, reverse geocoding, routing |
| Photon | `photon` | None, or a deployment URL | Selectable search provider | Geocoding, reverse geocoding, search and suggestions |
| OSRM | `osrm` | None, or a deployment URL | Selectable routing provider | Routing with its capability readback |
| Valhalla | `valhalla` | None, or a deployment URL | Selectable routing provider | Routing with its capability readback |
| OpenStreetMap Standard | `openstreetmap` | None | Raster tiles | Raster tiles |
| OpenStreetMap US | `osmus` | None | Vector base | Vector tiles with Carto Light and Carto Dark |
| OpenFreeMap | `openfreemap` | None | Vector base | Vector tiles with Bright, Fiord, and Positron |

MapQuest registers through `configureProvider`, one capability at a time, or
`setProviderCredential` for its whole capability set. Photon accepts an empty
credential and reaches its public instance; OSRM and Valhalla need their
deployment's base URL, and the reference apps pass each descriptor's public
demo address. OpenStreetMap Standard registers through
`registerUrlTemplateRasterTileSource`, and its tiles then feed the canvas's
live raster fallback. OpenStreetMap US and OpenFreeMap are served by
`MapView.setVectorTileSource` and need no registration call.

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

| Capability | Public JavaScript call |
|---|---|
| Geocoding | `forwardGeocode(queryText)`, `reverseGeocode(latitude, longitude)` |
| Suggestions and search | `suggestPlaces(queryText)`, `suggestPlacesInBoundary(...)`, `searchPlaces(queryText)`, `searchPlacesInBoundary(...)` |
| Routing with options | `calculateRoute(originLatitude, originLongitude, destinationLatitude, destinationLongitude)`, `calculateRouteWithOptions(..., options)` |
| Route capabilities | `routeCapabilities()` returns the modes and the strategy the serving provider honours |
| Distance matrix | `calculateDistanceMatrix(originCoordinates, destinationCoordinates, transportationMode)` |
| Traffic incidents and flow | `fetchTrafficIncidents(...)`, `fetchTrafficFlow(...)` |
| Elevation | `fetchElevationPoints(pointCoordinates)`; refuses while no elevation provider is configured |
| Raster tiles | `fetchRasterTile(zoomLevel, tileX, tileY)` through the serving provider; `registerUrlTemplateRasterTileSource(source)` installs a `{z}/{x}/{y}` tileset |
| Vector tiles and styles | `MapView.setVectorTileSource(source)`, `MapView.clearVectorTileSource()`, `MapView.vectorTileSourceStatus()` |
| Offline routing | `offlineRouteFromArchiveBytes(archiveBytes, startLatitude, startLongitude, endLatitude, endLongitude)` |
| Navigation | `Navigator.startNavigation(...)`, `Navigator.updateLocation(...)`, `Navigator.applyReroute(...)`, `Navigator.stopNavigation()`, `Navigator.currentTripState()`, `Navigator.hasActiveTrip()`; `RouteSimulator` |
| Provider registration | `configureProvider(contract, providerKey, credential)` per capability, `setProviderCredential(credential)` for a provider's whole set, `providerRegistrations()` to read back which provider serves each capability, `clearProviderRegistrations()` to empty every slot |

There is no disk tile cache and no host-implemented provider registration on
this surface. Calls reject with the provider's verbatim message when no
provider is registered or the provider refuses.

## Switch the base map

### A styled vector base

The registry's keyless vector base for OpenFreeMap is Bright. Install it on
the canvas; every following frame draws the styled base beneath your
overlays.

<!-- data-platform-panel="wasm" -->
```js
mapView.setVectorTileSource({
  urlTemplate: 'https://tiles.openfreemap.org/planet/latest/{z}/{x}/{y}.pbf',
  sourceName: 'openfreemap',
  attributionText:
    'Map data © OpenStreetMap contributors (ODbL) · style Bright from OpenFreeMap styles, design CC BY 4.0',
  styleId: 'bright-light',
  minimumZoomLevel: 0,
  maximumZoomLevel: 14,
});
const status = mapView.vectorTileSourceStatus();
```

While a vector source is installed, the canvas hides the raster layer and
paints the style's own background. The status readback reports whether a
source is configured, the style id, the source name, the attribution line,
the zoom ceiling, and the last frame's drawn and failed tile counts. A failed
count above zero with a `lastFailureDetail` means the tileset refused some
tiles; the base stays installed and the next frame retries.

The install is refused while a frame is resolving. Call it outside
`renderFrame`, or retry after the frame settles. Return to the raster layer
with `clearVectorTileSource()`.

```js
mapView.clearVectorTileSource();
```

### The raster fallback

The canvas draws its same-origin recorded tiles first. The serving raster-tile
provider answers for addresses those tiles do not cover, so panning anywhere
still draws imagery. Register a `{z}/{x}/{y}` tileset by declaration:

```js
import { registerUrlTemplateRasterTileSource } from './web/rustic_maps_web_bindings.js';

registerUrlTemplateRasterTileSource({
  urlTemplate: 'https://tile.openstreetmap.org/{z}/{x}/{y}.png',
  sourceName: 'OpenStreetMap Standard',
  minimumZoomLevel: 0,
  maximumZoomLevel: 19,
  imageFormat: 'png',
  tileResolution: 'standard',
  userAgent: navigator.userAgent,
});
```

A declaration whose template lacks `{z}`, `{x}`, or `{y}`, whose name is
blank, or whose zoom range is inverted throws with a precise detail, and the
previous source keeps serving. The canvas fetches at the ceiling the source
declares. Registering a provider that carries a credential replaces it.

## Register a provider

Registration is one call per capability: name the capability from the
contract, the provider key from the shared registry, and the credential. The
credential lives in module memory only; it is never persisted or logged.

```js
import init, {
  MapsSdk,
  configureProvider,
  providerRegistrations,
} from './web/rustic_maps_web_bindings.js';

await init();
const mapSdk = await MapsSdk.create();

configureProvider('Geocoding', 'mapquest', providerCredential);
configureProvider('Routing', 'mapquest', providerCredential);

const registrations = providerRegistrations();
```

`providerRegistrations()` answers one row per capability, in the contract's
order, with the provider key, its display name and whether a provider is
installed. Each slot is independent, so a page may serve geocoding from one
provider and raster tiles from another. `setProviderCredential(credential)`
registers the full-service provider for every capability it serves, and
`clearProviderRegistrations()` empties every slot. Photon accepts an empty
credential; OSRM and Valhalla take the deployment's base URL.

```js
import { configureProvider } from './web/rustic_maps_web_bindings.js';

configureProvider('Routing', 'osrm', 'https://router.project-osrm.org');
```

A key with no compiled provider is refused with a message naming the key, and
a blank credential for a provider that requires one is refused before any
slot changes. Apply the credential through a settings form or a test's
runtime environment; never compile it into the bundle.

## What you should see

After the vector install, `vectorTileSourceStatus()` reports a configured
source with the `bright-light` style id and the OpenFreeMap source name.
After `configureProvider`, the capability's calls resolve and
`providerRegistrations()` names the provider; before it, they reject with the
registration refusal. The canvas keeps drawing the keyless base and its
recorded tiles in either case.

## Constraints

- The module must be initialized and `MapsSdk.create()` resolved before a
  `MapView` is constructed.
- The browser surface has no disk tile cache and no host-implemented provider
  registration. A provider that needs those contracts is a compiled provider
  crate, covered in [Add a provider](adding-a-provider.md).
- `MapView.setVectorTileSource` refuses while a frame is resolving; retry
  after the frame settles.
- Tests never reach the network. Recorded tiles and provider responses are
  served from the page's own origin.

## Where to go next

- [Add a provider](adding-a-provider.md) for the two implementation routes.
- [Search and routing](search-and-routing.md) for the provider calls in a
  complete flow.
- [Offline maps](offline-maps.md) for archive routing in the browser.
