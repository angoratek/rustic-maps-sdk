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

<div class="platform-tabs">
<div class="platform-tab-titles" role="tablist">
<button type="button" data-platform-tab="android">Android</button>
<button type="button" data-platform-tab="ios">iOS</button>
<button type="button" data-platform-tab="react-native">React Native</button>
<button type="button" data-platform-tab="wasm">WebAssembly</button>
</div>

<div class="platform-tab-panel" data-platform-panel="android">

### Resolve a query into places

Provider calls on Android are Kotlin `suspend fun`s. Run them from a
coroutine scope. Each plan field resolves the text the user typed:

<!-- docs-snippet: begin search-and-routing/search-for-places android -->
```kotlin
val originPlace: Place = mapSdk.forwardGeocode(originSearchQuery).firstOrNull()
    ?: return null
val destinationPlace: Place = mapSdk.forwardGeocode(destinationSearchQuery).firstOrNull()
    ?: return null
```
<!-- docs-snippet: end search-and-routing/search-for-places android -->

`forwardGeocode` returns every candidate the provider resolved, most likely
first. `suggest` returns fast completion candidates while the user types, and
`search` returns full results for a settled query. Both search calls take an
optional `BoundingBox` that restricts the search.

### Plan the route

`calculateRoute` takes the ordered waypoints and the routing options. The
request is the same on every platform: two waypoints and a `RouteOptions`
carrying the transport mode and strategy the application offers — Car and
Fastest by default — with metric units and no avoids. A provider that cannot
honour a choice reports it through `routeCapabilities()`; see
[Mixing providers per capability](mix-providers-per-capability.md).

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

The returned `Route` carries the total distance and duration, the bounding
box, and one leg per waypoint pair with its maneuver steps.

### Frame the route preview

Let the camera fit the route's bounding box through the `fitBounds`
constructor instead of hand-tuning zoom:

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

The insets you pass are the pixels your overlays keep clear, so the
destination never hides under your own panels.

### Draw the route

Build one route-polyline overlay from the planned geometry and hand it to
the map view. The overlay step in [Your first map](first-map.md) covers the
descriptor's other fields.

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

The contract also groups these capabilities into small engine objects built
over the same SDK. Each one delegates to the SDK, with the same typed
failures and detail text as the `MapSdk` member it wraps:

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

`suggest` returns fast completion candidates for a query under construction.
`search` returns full results for a settled query. Both take an optional
`BoundingBox`.

### Plan the route

The `routeOptions` value is the option record the application built from its
Directions control — automobile and fastest by default — with metric units
and no avoids:

<!-- docs-snippet: begin search-and-routing/plan-the-route ios -->
```swift
let plannedRoute = try await mapSdk.calculateRoute(
    waypoints: [originPlace.coordinate, destinationPlace.coordinate],
    routeOptions: routeOptions
)
```
<!-- docs-snippet: end search-and-routing/plan-the-route ios -->

The returned `Route` carries distance, duration, the bounding box,
and one leg per waypoint pair with its steps.

### Frame and draw the preview

Fit the camera to the route's bounding box, as [Your first map](first-map.md)
shows, then hand the route geometry to the map view's overlay list:

```swift
let fittedCamera = try MapCamera.fitBounds(
    boundingBox: plannedRoute.boundingBox,
    viewport: viewport,
    edgeInsets: EdgeInsets(left: 224, top: 48, right: 48, bottom: 96),
    maximumZoomLevel: 22
)
pinnedFitBoundsCamera = fittedCamera
```

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

Each engine wraps the same SDK and keeps the typed failures and detail text:

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

The package's `MapsSdk` handle returns the contract's search and routing
calls as promises. Every record it resolves carries the same fields the other
platforms see, in lower camel case:

<!-- docs-snippet: begin search-and-routing/search-for-places react-native -->
```ts
const originPlace = (await mapSdk.forwardGeocode(originSearchQuery))[0] ?? null;
const destinationPlace =
  (await mapSdk.forwardGeocode(destinationSearchQuery))[0] ?? null;
```
<!-- docs-snippet: end search-and-routing/search-for-places react-native -->

A query that resolves to no place leaves the field `null`. The example stops
before planning rather than inventing a coordinate.

Pass `undefined` or omit the boundary to search worldwide. Pass a
`BoundingBox` with `southWest` and `northEast` corners to restrict the search.
`suggest` returns completion candidates while the user types; `search`
returns full results for a settled query.

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

The route carries `distanceMeters`, `durationSeconds`, the bounding box and
one leg per waypoint pair with its steps.

A rejected promise carries the typed `SdkError`. Surface it in your error
state rather than retrying blindly; the rate limiter refills on its own.

### Frame the route preview

The map view's `fitBounds` takes the same inset story as the other tabs:
224 px clear on the left for the route card, 48 px top and right, 96 px
bottom, clamped at zoom 22:

```js
mapView.fitBounds(
  plannedRoute.boundingBox,
  { left: 224, top: 48, right: 48, bottom: 96 },
  22,
);
```

### Draw the route

Build one route-polyline overlay from the planned geometry and hand it to the
map view:

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

### Keep one SDK handle

The package handle exposes `search`, `suggest`, `forwardGeocode`, and
`calculateRoute` directly, so a React Native screen needs no extra wrapper.
The generated engine classes wrap a lower-level `MapSdk` instance, which would
be a second SDK — with its own provider registry and tile cache — in the same
process. Keep the single handle from the [first map](first-map.md) guide.

</div>

<div class="platform-tab-panel" data-platform-panel="wasm">

### Register the provider at runtime

A browser page has no settings sheet. Provider registration is one call per
capability, naming the contract, the provider key from the shared registry and
your credential. Take the credential from your own runtime source: a form, a
server-issued token, or a test environment. Keyless, the demo searches
recorded places and plans recorded routes.

```js
import { configureProvider } from './web/rustic_maps_web_bindings.js';

configureProvider('Geocoding', 'mapquest', providerCredential);
configureProvider('Routing', 'mapquest', providerCredential);
```

The calls below reject with the provider's own error text when no provider is
registered or the provider refuses.

### Resolve the queries into places, then plan the route

`forwardGeocode` returns every candidate the provider resolved, most likely
first. Its rows carry `structuredAddressText`, `latitudeDegrees`, and
`longitudeDegrees`. `suggestPlaces` returns completion candidates, and
`searchPlaces` returns full results for a settled query. The `calculateRoute`
call takes two coordinates and uses the default request: automobile, fastest,
metric.

```js
import { forwardGeocode, calculateRoute } from './web/rustic_maps_web_bindings.js';

const originSearchQuery = 'Capitol Park';
const destinationSearchQuery = 'Tower Bridge';

const originPlace = (await forwardGeocode(originSearchQuery))[0] ?? null;
const destinationPlace = (await forwardGeocode(destinationSearchQuery))[0] ?? null;
if (originPlace === null || destinationPlace === null) {
  throw new Error('the provider resolved no place for one of the queries');
}

const plannedRoute = await calculateRoute(
  originPlace.latitudeDegrees,
  originPlace.longitudeDegrees,
  destinationPlace.latitudeDegrees,
  destinationPlace.longitudeDegrees,
);
```

For a pedestrian route or explicit avoids, `calculateRouteWithOptions` takes
the contract's `RouteOptions`. The returned record has the same
`distanceMeters`, `durationSeconds`, `boundingBox`, and `legs`-of-`steps` shape
the other tabs read.

### Frame the route preview

Let the camera fit the route's bounding box through the same fitted constructor
the native tabs use. The arguments are the box's corners, the viewport, the edge
insets in top/right/bottom/left order, and the zoom ceiling:

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

The insets are the pixels your own panels keep clear, so the destination never
hides under them. A refused fit throws the reason; keep the previous camera.

### Draw the route

Build one route-polyline overlay from the planned geometry and hand it to the
view. The browser descriptor spells the color field `color`, not the native
tabs' `rgbaColor`:

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
