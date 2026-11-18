# Search and routing

Turn a typed query into places, plan a route between them, and draw it on
the map. This guide continues from [Your first map](first-map.md); every
code block compiles against the [FFI contract](udl-contract.md).

## Choose the transport mode and strategy

`RouteOptions` carries the transport mode and the route strategy for every
plan. The SDK can express Car, Walk and Bicycle, and the Fastest and Shortest
strategies. A provider may not serve all of them, so read
`routeCapabilities()` before you show the control. It answers the supported
modes and whether Shortest is available.

| Provider | Car | Walk | Bicycle | Fastest | Shortest |
|---|---|---|---|---|---|
| MapQuest | yes | yes | yes | yes | automobile only |
| Valhalla | yes | yes | yes | yes | automobile only |
| OSRM | yes | yes | yes | yes | no |

Shortest applies to automobile plans only. A walk plan always sends Fastest.
OSRM has no route-service switch for the ranking, so both strategies produce
the same request and the deployment's profile decides the path.

A choice the provider reports as unsupported is disabled with a reason. A
remembered choice the provider cannot honour falls back without being
rewritten: an unsupported mode falls back to Car, then Walk, and Shortest
falls back to Fastest. Returning to a provider that supports the choice
restores it.

The exact disabled-with-reason sentence each surface shows:

| Surface | Unsupported mode | Unsupported Shortest |
|---|---|---|
| Android | `{mode} directions are not available from this provider.` | `Shortest route strategy is not available from this provider.` |
| iOS | `{mode} is not offered by the serving routing provider.` | `Shortest is not offered by the serving routing provider.` |
| React Native | `The serving provider does not support walking routes.` or `The serving provider does not support car routes.` | `The serving provider does not support shortest routes.` |
| WebAssembly | No reason line. The browser API exposes `routeCapabilities()`; the demo sends the selected pair and the route card states the mode and strategy. | No reason line. The selected strategy is sent, and the provider's documented ranking applies. |

On Android, iOS and React Native, if the capability read fails because no
routing provider is installed, the surface disables both mode choices and
Shortest with the contract's sentence,
`no 'RoutingProvider' provider is configured`.

## The calls

`forwardGeocode` returns every candidate the provider resolved, most likely
first; `suggest` returns completions while the user types, and `search` returns
full results for a settled query, each with an optional `BoundingBox`.
`calculateRoute` takes ordered waypoints and `RouteOptions`, defaulting to Car
and Fastest with metric units and no avoids; the returned `Route` carries
distance, duration, the bounding box, and one leg per waypoint pair with its
steps. `fitBounds` frames that box with the insets your own panels keep clear,
and a refused fit surfaces its reason. Draw one route-polyline overlay from the
flattened step shapes; the overlay fields are in [Your first map](first-map.md).
`SearchEngine` and `RoutingEngine` are thin facades over the same SDK with the
same typed failures.

<div class="platform-tabs">
<div class="platform-tab-titles" role="tablist">
<button type="button" data-platform-tab="android">Android</button>
<button type="button" data-platform-tab="ios">iOS</button>
<button type="button" data-platform-tab="react-native">React Native</button>
<button type="button" data-platform-tab="wasm">WebAssembly</button>
</div>

<div class="platform-tab-panel" data-platform-panel="android">

### Resolve a query into places

Provider calls on Android are Kotlin `suspend fun`s; run them from a coroutine
scope:

<!-- docs-snippet: begin search-and-routing/search-for-places android -->
```kotlin
val originPlace: Place = mapSdk.forwardGeocode(originSearchQuery).firstOrNull()
    ?: return null
val destinationPlace: Place = mapSdk.forwardGeocode(destinationSearchQuery).firstOrNull()
    ?: return null
```
<!-- docs-snippet: end search-and-routing/search-for-places android -->

### Plan the route

<!-- docs-snippet: begin search-and-routing/plan-the-route android -->
```kotlin
val routeOptions = RouteOptions(
    transportationMode = transportationMode,
    routeStrategy = routeStrategy,
    unitSystem = UnitSystem.METRIC,
    avoidOptions = AvoidOptions(
        tollRoads = false,
        limitedAccessHighways = false,
        ferries = false,
        unpavedRoads = false,
    ),
    departureTimestampSeconds = null,
)
val plannedRoute = mapSdk.calculateRoute(
    waypoints = listOf(originPlace.coordinate, destinationPlace.coordinate),
    routeOptions = routeOptions,
)
```
<!-- docs-snippet: end search-and-routing/plan-the-route android -->

### Frame the route preview

<!-- docs-snippet: begin search-and-routing/frame-the-route android -->
```kotlin
import uniffi.rustic_maps.EdgeInsets
import uniffi.rustic_maps.MapCamera
import uniffi.rustic_maps.ViewportSize

val fittedCamera = try {
    MapCamera.fitBounds(
        route.boundingBox,
        ViewportSize(widthPixels, heightPixels),
        EdgeInsets(left = 224.0, top = 48.0, right = 48.0, bottom = 96.0),
        maximumZoomLevel = 22.0,
    )
} catch (sdkException: SdkException) {
    // A refused fit (no positive box area, unusable insets) surfaces
    // verbatim; keep the previous camera.
    null
}
```
<!-- docs-snippet: end search-and-routing/frame-the-route android -->

### Draw the route

<!-- docs-snippet: begin search-and-routing/draw-the-route android -->
```kotlin
val routeOverlay = MapOverlay(
    kind = MapOverlayKind.ROUTE_POLYLINE,
    coordinates = plannedRoute.legs
        .flatMap { leg -> leg.steps }
        .flatMap { step -> step.shape },
    rgbaColor = null,
    widthPixels = null,
    ringCoordinates = null,
    pointMarkerPositions = null,
    pointMarkerRadiusPixels = null,
    trafficFlowSegments = null,
)
mapView.setOverlays(listOf(routeOverlay))
```
<!-- docs-snippet: end search-and-routing/draw-the-route android -->

Then start a trip in [Turn-by-turn navigation](navigation-simulation.md).

### Group the calls with engines

```kotlin
import uniffi.rustic_maps.RoutingEngine
import uniffi.rustic_maps.SearchEngine

val searchEngine = SearchEngine(mapSdk)
val settledPlaces = searchEngine.search("Capitol Park", searchBoundary = null)

val routingEngine = RoutingEngine(mapSdk)
val plannedRoute = routingEngine.calculateRoute(waypoints, routeOptions)
```

</div>

<div class="platform-tab-panel" data-platform-panel="ios">

### Resolve a query into places

Provider calls on iOS throw on failure and await the provider:

<!-- docs-snippet: begin search-and-routing/search-for-places ios -->
```swift
let originResults = try await mapSdk.forwardGeocode(addressText: originSearchQuery)
let destinationResults = try await mapSdk.forwardGeocode(
    addressText: destinationSearchQuery
)
guard
    let originPlace = originResults.first,
    let destinationPlace = destinationResults.first
else {
    throw WalkthroughScenarioError.planFieldResolvedNoPlace
}
```
<!-- docs-snippet: end search-and-routing/search-for-places ios -->

### Plan the route

<!-- docs-snippet: begin search-and-routing/plan-the-route ios -->
```swift
let plannedRoute = try await mapSdk.calculateRoute(
    waypoints: [originPlace.coordinate, destinationPlace.coordinate],
    routeOptions: routeOptions
)
```
<!-- docs-snippet: end search-and-routing/plan-the-route ios -->

### Frame and draw the preview

<!-- docs-snippet: begin search-and-routing/frame-the-route ios -->
```swift
let fittedCamera = try MapCamera.fitBounds(
    boundingBox: plannedRoute.boundingBox,
    viewport: viewport,
    edgeInsets: EdgeInsets(left: 224, top: 48, right: 48, bottom: 96),
    maximumZoomLevel: 22
)
pinnedFitBoundsCamera = fittedCamera
```
<!-- docs-snippet: end search-and-routing/frame-the-route ios -->

<!-- docs-snippet: begin search-and-routing/draw-the-route ios -->
```swift
let routeOverlay = MapOverlay(
    kind: .routePolyline,
    coordinates: plannedRoute.legs
        .flatMap { leg in leg.steps }
        .flatMap { step in step.shape },
    rgbaColor: nil,
    widthPixels: nil,
    ringCoordinates: nil,
    pointMarkerPositions: nil,
    pointMarkerRadiusPixels: nil,
    trafficFlowSegments: nil
)
_ = mapView.setOverlays([routeOverlay])
```
<!-- docs-snippet: end search-and-routing/draw-the-route ios -->

### Group the calls with engines

```swift
let searchEngine = SearchEngine(mapSdk: mapSdk)
let settledPlaces = try await searchEngine.search(
    queryText: "Capitol Park",
    searchBoundary: nil
)

let routingEngine = RoutingEngine(mapSdk: mapSdk)
let plannedRoute = try await routingEngine.calculateRoute(
    waypoints: waypoints,
    routeOptions: routeOptions
)
```

</div>

<div class="platform-tab-panel" data-platform-panel="react-native">

### Resolve a query into places

The package's `MapsSdk` handle returns the contract's search and routing calls
as promises:

<!-- docs-snippet: begin search-and-routing/search-for-places react-native -->
```ts
const originPlace = (await mapSdk.forwardGeocode(originSearchQuery))[0] ?? null;
const destinationPlace =
  (await mapSdk.forwardGeocode(destinationSearchQuery))[0] ?? null;
```
<!-- docs-snippet: end search-and-routing/search-for-places react-native -->

A query that resolves to no place leaves the field `null`. The example stops
before planning rather than inventing a coordinate. Pass `undefined` or omit
the boundary to search worldwide.

### Plan the route

<!-- docs-snippet: begin search-and-routing/plan-the-route react-native -->
```ts
const routeOptions: RouteOptions = {
  transportationMode: TransportationMode.Pedestrian,
  routeStrategy: RouteStrategy.Fastest,
  unitSystem: UnitSystem.Metric,
  avoidOptions: {
    tollRoads: false,
    limitedAccessHighways: false,
    ferries: false,
    unpavedRoads: false,
  },
  departureTimestampSeconds: undefined,
};
const plannedRoute = await mapSdk.calculateRoute(
  [originPlace.coordinate, destinationPlace.coordinate],
  routeOptions,
);
```
<!-- docs-snippet: end search-and-routing/plan-the-route react-native -->

A rejected promise carries the typed `SdkError`. Surface it in your error
state rather than retrying blindly; the rate limiter refills on its own.

### Frame the route preview

```js
mapView.fitBounds(
  plannedRoute.boundingBox,
  { left: 224, top: 48, right: 48, bottom: 96 },
  22,
);
```

### Draw the route

<!-- docs-snippet: begin search-and-routing/draw-the-route react-native -->
```ts
const routeOverlay: MapOverlay = {
  kind: MapOverlayKind.RoutePolyline,
  coordinates: plannedRoute.legs.flatMap(leg =>
    leg.steps.flatMap(step => step.shape),
  ),
};
await mapView.setOverlays([routeOverlay]);
```
<!-- docs-snippet: end search-and-routing/draw-the-route react-native -->

Keep the single package handle from the [first map](first-map.md) guide. The
generated engine classes wrap a lower-level `MapSdk` instance, which would be
a second SDK with its own provider registry and tile cache in the same
process.

</div>

<div class="platform-tab-panel" data-platform-panel="wasm">

### Register the provider at runtime

A browser page has no settings sheet. Provider registration is one method per
capability on the `MapSdk` root, naming the contract member, the provider key
from the shared registry and your credential. Take the credential from a form,
a server-issued token, or a test environment. Keyless, the demo searches
recorded places and plans recorded routes.

```js
import { ProviderContract } from './web/rustic_maps.js';

mapSdk.configureProvider(ProviderContract.Geocoding, 'mapquest', providerCredential);
mapSdk.configureProvider(ProviderContract.Routing, 'mapquest', providerCredential);
```

The calls below reject with the provider's own error text when no provider is
registered or the provider refuses.

### Resolve the queries into places, then plan the route

```js
import { RouteStrategy, TransportationMode, UnitSystem } from './web/rustic_maps.js';

const originSearchQuery = 'Capitol Park';
const destinationSearchQuery = 'Tower Bridge';

const originPlace = (await mapSdk.forwardGeocode(originSearchQuery))[0] ?? null;
const destinationPlace = (await mapSdk.forwardGeocode(destinationSearchQuery))[0] ?? null;
if (originPlace === null || destinationPlace === null) {
  throw new Error('the provider resolved no place for one of the queries');
}

const plannedRoute = await mapSdk.calculateRoute(
  [originPlace.coordinate, destinationPlace.coordinate],
  {
    transportationMode: TransportationMode.Automobile,
    routeStrategy: RouteStrategy.Fastest,
    unitSystem: UnitSystem.Metric,
    avoidOptions: {
      tollRoads: false,
      limitedAccessHighways: false,
      ferries: false,
      unpavedRoads: false,
    },
  },
);
```

The records carry `structuredAddressText`, a nested `coordinate` and
`geocodeQuality`; `suggest` and `search` are the completion and search calls.
`calculateRoute` takes the contract's `RouteOptions` record: transport mode,
strategy, unit system and the features to avoid.

### Frame the route preview

```js
/** @type {any} */
const plannedRoute = route;
const viewportWidthPixels = window.innerWidth;
const viewportHeightPixels = window.innerHeight;
const topInsetPixels = 48.0;
const rightInsetPixels = 48.0;
const bottomInsetPixels = 96.0;
const leftInsetPixels = 224.0;
const maximumZoomLevel = 22.0;

const fittedCamera = MapCamera.fittingBounds(
  plannedRoute.boundingBox.southWest.latitude,
  plannedRoute.boundingBox.southWest.longitude,
  plannedRoute.boundingBox.northEast.latitude,
  plannedRoute.boundingBox.northEast.longitude,
  viewportWidthPixels,
  viewportHeightPixels,
  topInsetPixels,
  rightInsetPixels,
  bottomInsetPixels,
  leftInsetPixels,
  maximumZoomLevel,
);
```

A refused fit throws the reason; keep the previous camera.

### Draw the route

Build one route-polyline overlay from the planned geometry. The browser
descriptor spells the color field `color`.

```js
const plannedRoute = route;
const routeOverlay = {
  kind: 'routePolyline',
  coordinates: plannedRoute.legs.flatMap(
    (leg) => leg.steps.flatMap((step) => step.shape),
  ),
  color: { red: 0.16, green: 0.47, blue: 1.0, alpha: 0.95 },
  widthPixels: 6.0,
};
mapView.setOverlays([routeOverlay]);
```

Then start a trip in [Turn-by-turn navigation](navigation-simulation.md).

</div>

</div>
