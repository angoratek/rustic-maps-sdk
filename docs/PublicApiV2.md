# Public API v2 — naming, `MapView` and the one-contract goal

**Status:** the SDK's live developer-facing API reference. The surface below is
what the SDK exposes today; the naming rules and the `MapView` shape are the
ones the bindings and the reference applications implement.
**Scope:** every developer-facing surface — the UDL contract, the Kotlin, Swift,
React Native and WebAssembly bindings, and the reference applications.

The document is the live developer-facing naming reference: the master mapping a
consumer sees, the naming rules, the exact `MapView` surface, and the
WebAssembly and native construction decisions. The site's tutorials are written
from it.

## 1. Master mapping table

Spelling shown is as a developer types it in Kotlin / Swift / TypeScript. UniFFI
camelCases UDL `snake_case` automatically, so the UDL keeps snake_case; the column is
the *consumer-facing* name.

| Concept | HERE | Mapbox v11 | Google | OSM / MapLibre | Before M17 | Adopted in M17 |
|---|---|---|---|---|---|---|
| SDK root object | `SDKNativeEngine` (shared instance) | `MapboxMapsOptions` + `MapView` | `MapsInitializer` | `MapLibre.getInstance` | `MapSdk` | **`MapsSdk`** (Rust type stays `MapSdk`) |
| Embeddable map surface | `MapView` (`FrameLayout`) | `MapView` | `MapView` / `GMSMapView` | `MapView` / `FlutterMap` | `MapSurfaceView` (UDL), `GpuMapCanvas` (wasm) | **`MapView`** |
| Platform surface handle | — (internal) | `MapSurface` (`TextureView`/`SurfaceView`) | — | `NativeMapView` | `MapSurfaceTargetHandle` | **`SurfaceHandle`** (internal, hidden) |
| Lifecycle | `onCreate/onResume/onPause/onDestroy` | `onStart/onStop/onLowMemory/onDestroy` | `onCreate…onDestroy` + `onSaveInstanceState` | `onCreate…onDestroy` + `onSaveInstanceState` | `attach` / `resize` / `detach` / `draw` | **`onStart/onStop/onDestroy`**; `attach/detach` stay internal |
| Readiness | `MapView.OnReadyListener.onMapViewReady()` | `MapLoadedCallback` | `OnMapReadyCallback.onMapReady` | `OnMapReadyCallback.onMapReady` | none (only `frame_settled`) | **`MapView.OnReadyListener.onMapReady(mapView)`** |
| Camera value | `MapCamera` | `CameraOptions` / `CameraState` | `CameraPosition` | `CameraPosition` | `MapCamera` | **`MapCamera`** (keep) |
| Apply camera | `mapView.camera.lookAt(...)` | `mapboxMap.setCamera(CameraOptions)` | `moveCamera(CameraUpdate)` | `map.animateCamera(CameraUpdate)` | `new MapCamera(...)` then `draw(camera)` | **`mapView.setCamera(camera)` / `animateCamera(camera, durationMs)`** |
| Fit bounds | `MapCameraUpdateFactory.newLookAt(GeoBoundingBox)` | `cameraForCoordinateBounds(...)` | `CameraUpdateFactory.newLatLngBounds` | `getCameraForLatLngBounds` | `MapCamera(bounds, viewport, insets, maxZoom)` constructor | **`mapView.fitBounds(bounds, edgeInsets, maximumZoom)`** |
| Camera getters | `mapView.camera` | `mapboxMap.cameraState` | `map.getCameraPosition()` | `map.getCameraPosition()` | `MapCamera.center()/zoomLevel()/bearingDegrees()/pitchDegrees()` | **`mapView.camera` (value) with those getters** |
| Marker | `MapMarker` | `PointAnnotationManager` | `Marker` (`GMSMarker`) | `PointAnnotationManager` | `MapOverlayDescriptor(kind = PointMarkers)` | **`Marker`** |
| Polyline | `MapPolyline` | `PolylineAnnotationManager` | `Polyline` | `LineAnnotationManager` | `MapOverlayDescriptor(kind = RoutePolyline)` | **`Polyline`** |
| Polygon | `MapPolygon` | `PolygonAnnotationManager` | `Polygon` | `FillAnnotationManager` | `MapOverlayDescriptor(kind = FilledPolygon)` | **`Polygon`** |
| Overlay collection | `mapView.mapScene.addMapPolyline(...)` | `mapboxMap.annotations` | `googleMap.addMarker(...)` | `map.addMarkers(...)` | `setOverlays(list)` / `clearOverlays()` | **`mapView.addOverlay(...)`, `removeOverlay(handle)`, `clearOverlays()`** |
| Location puck | `LocationIndicator` | `LocationPuck2D/3D` | `MyLocation` layer | `LocationComponent` | `set_location_indicator` / `clear_location_indicator` | **`mapView.locationIndicator.show(...)` / `.hide()`** |
| Search | `SearchEngine` | `SearchEngine` (`mapbox-search-android`) | Places SDK (`PlacesClient`) | Places plugin | `MapSdk.forwardGeocode` / `searchPlaces` / `searchAutocomplete` | **`SearchEngine.search(query, boundary)` / `suggest(query)` / `reverseGeocode(coordinate)`** |
| Routing | `RoutingEngine.calculateRoute(with:options:)` | `MapboxDirections` | Directions API | — (none on-device) | `MapSdk.calculateRoute(waypoints, options)` | **`RoutingEngine.calculateRoute(waypoints, options)`** |
| Distance matrix | `RoutingEngine.calculateRoute` overload | Matrix API | Distance Matrix API | — | `MapSdk.calculateDistanceMatrix` | **`RoutingEngine.calculateDistanceMatrix(origins, destinations, mode)`** |
| Traffic | `TrafficEngine` | — | — | — | `MapSdk.fetchTrafficIncidents` / `fetchTrafficFlow` | **`TrafficEngine.incidents(bounds)` / `.flow(bounds)`** |
| Navigation | `Navigator` / `VisualNavigator` | `MapboxNavigation` / `NavigationMapView` | Navigation SDK (`NavigationView`) | — | `MapNavigationController` | **`Navigator`** |
| Start navigation | `navigator.startRoute(...)` | `navigation.startTrip(...)` | `NavigationLauncher` | — | `start_trip(route, ...)` | **`navigator.startNavigation(route, options)`** |
| Stop navigation | `navigator.stop()` | `navigation.stop()` | — | — | (none) | **`navigator.stopNavigation()`** |
| Location feed | `LocationEngine` → updates | `location` provider | — | `LocationComponent` | `provide_location_update(update)` | **`navigator.updateLocation(update)`** |
| Reroute | `RoutingEngine` + `Navigator` callbacks | `NavigationRoute` refresh | — | — | `provide_reroute` / `begin_reroute_calculation` | **`navigator.applyReroute(route)`**; gate stays internal |
| Navigation events | `NavigatorListener` | `NavigationListener` | `NavigationListener` | — | `NavigationStateEventListener` + 4 `subscribe_to_*` | **`NavigationListener` + `navigator.addNavigationListener { }`** |
| Subscription handle | `NavigatorListener` remove | `Cancelable` | `ListenerRegistration` | `Cancelable` | `NavigationTopicSubscription.cancel()` | **`Subscription.cancel()`** (keep shape) |
| Offline downloads | `MapDownloader` / `MapUpdater` / `OfflineSearchEngine` | `OfflineManager` / `OfflineRegion` | — | `OfflineManager.createOfflineRegion` | `register_mapquest_providers_with_tile_cache` + `set_raster_tile_cache_time_to_live` | **`OfflineManager.tileCacheTerms()` / `.setTileTimeToLive(seconds)`** (landed), **`.regionTileCount(region)` / `.downloadRegion(region, listener)` / `.cancelRegionDownload(id)` / `.regionRecords()` / `.regionRecord(id)` / `.deleteRegion(id)`** (contract names `region_tile_count`, `download_region`, `cancel_region_download`, `region_records`, `region_record`, `delete_region`; planned) |
| Offline progress | `MapDownloader` progress | `OfflineRegionObserver.onStatus` | — | `OfflineRegionObserver` | `TileFrameStatistics` only | **`RegionDownloadProgressListener.onRegionDownloadProgress(progress)`** (`completedTileCount` / `totalTileCount` / `networkFetchedTileCount` / `diskServedTileCount`; planned) |
| Provider credential | `SDKOptions(AuthenticationMode.withKeySecret)` | `MapboxOptions.accessToken` | manifest API key | style URL | `register_mapquest_providers(key)` | **`maps.configureProviderSet(providerKey, credential)`** (every capability one provider serves) or **`maps.configureProvider(contract, providerKey, credential)`** (one capability) |
| Provider instance | engine constructor injection | `MapboxOptions` | `MapsInitializer` | `MapLibre.setHttpClient` | `register_*_provider_from_foreign(...)` | **`maps.configureProvider(contract, providerKey, credential)`** for a built-in key; a foreign object uses the matching `register_*_from_foreign` call. The `maps.providers.register(provider)` façade is **not landed** |
| Base-map / style | `mapScene.loadScene(MapScheme)` | `loadStyle(StyleURI)` | `GoogleMap.setMapType` | `setStyle(url)` | (raster/vector tile source on the canvas) | **`mapView.setStyle(MapStyle)` + `maps.tileProviders.setRasterSource(source)`** |
| Diagnostics | — | `MapboxMaps` logging | — | — | `subscribe_to_diagnostic_events` / `flush_recent_diagnostic_events` | **`maps.diagnostics.addListener(listener, minimumSeverity)` / `.flushRecent(maximumCount)`** |
| Errors | typed `SDKError` | `MapboxMap` throws | logs + callbacks | throws | `SdkError` | **`SdkException`** (keep the UDL type name `SdkError`) |

**Verdict on our vocabulary:** coordinates, tiles, camera value and progress records are
already better named than most vendors' (`GeographicCoordinate` beats `LatLng`, `Point`,
`CLLocationCoordinate2D`). Everything *above* the record layer — the root object, the
view, the navigation controller and the wasm names — is where the drift was;
M17 adopted the names in the right-hand column.

---

## 2. Naming rules (adopt these as review criteria)

- **R1 — Noun-first types.** `MapView`, `MapCamera`, `MapStyle`, `SearchEngine`,
  `RoutingEngine`, `Navigator`, `OfflineManager`. Never `MapViewHost`, `MapSurfaceHost`,
  `CanvasManager`.
- **R2 — One concept, one word, everywhere.** Pick *view*, *camera*, *marker*,
  *polyline*, *polygon*, *route*, *navigation*, *navigator*, *region*, *provider*,
  *credential*, *listener*, and never alternate with *surface*, *scene*, *puck*, *trip*,
  *subscription* (as a noun for the thing you listen to).
- **R3 — Engine = noun + verb methods.** `SearchEngine.search`, `RoutingEngine.calculateRoute`,
  `Navigator.startNavigation`, `OfflineManager.downloadRegion`.
- **R4 — Approved verbs.** `search`, `suggest`, `reverseGeocode`, `calculateRoute`,
  `calculateDistanceMatrix`, `startNavigation`, `stopNavigation`, `updateLocation`,
  `setCamera`, `animateCamera`, `fitBounds`, `addOverlay`, `removeOverlay`,
  `clearOverlays`, `configureProviderSet`, `configureProvider`,
  `providerRegistrations`, `register`, `downloadRegion`,
  `deleteRegion`, `addListener`, `removeListener`, `cancel`.
- **R5 — Booleans read as questions.** `isFrameSettled`, `isNavigationActive`,
  `isOffline`. Never `frame_settled` on a foreign surface.
- **R6 — Events are `on<PastParticiple>` on a listener interface.** `onMapReady`,
  `onCameraChanged`, `onInstruction`, `onSpeechInstruction`, `onDeviationChanged`,
  `onPhaseChanged`, `onArrival`, `onError`.
- **R7 — Units live in the name, spelled out.** `_meters`, `_seconds`, `_pixels`,
  `_degrees`, `_bytes`. Keep the current discipline (`distanceRemainingMeters`,
  `durationRemainingSeconds`); it is a strength most vendors lack. Never `ms`, `px`,
  `deg` in identifiers (prose may abbreviate).
- **R8 — No vendor in a public name.** `registerMapquestProviders` is the single
  worst name in the contract: it bakes MapQuest into every integrator's call site and
  contradicts the provider-genericity north star. Vendor names belong in provider
  crates and the registry descriptor only.
- **R9 — No platform/internal leakage.** `Surface`, `Gpu`, `Canvas`, `NativeWindow`,
  `Pointer`, `Descriptor` never appear in a name a developer types. `MapSurfaceTargetHandle`
  exists to carry `ANativeWindow*` / `UIView*`; it should be internal to the wrapper.
- **R10 — No transport-mode suffixes.** `_live` (`forwardGeocodeLive`,
  `calculateRouteLive`, `fetchRasterTileLive`) describes the harness's transport choice,
  not the SDK operation. Live/recorded is a *provider registration*, never a method name.
- **R11 — snake_case only inside Rust/UDL.** UniFFI camelCases UDL members for Kotlin,
  Swift and RN for free. The wasm surface does **not**: wasm-bindgen preserves Rust
  names unless each export carries `#[wasm_bindgen(js_name = ...)]`. Every wasm export
  must set `js_name`.
- **R12 — Abbreviations.** `SDK`, `URL`, `HTTP`, `GPS`, `MVT` allowed in prose, never as
  identifier shorthand (`docs/DocsStyleGuide.md` §11 already rules this). `Gpu` is an
  implementation word, not an abbreviation we need — drop it.
- **R13 — Values are immutable, verbs live on the owner.** `MapCamera` is a value
  (matches Mapbox `CameraOptions`); you change the camera by *telling the view*
  (`setCamera`), not by mutating the camera. This is already our UDL shape — keep it.
- **R14 — Prefix scope by plane, not by vendor.** `Map*` for things drawn on the map
  (`MapView`, `MapCamera`, `MapStyle`, `Marker`, `Polyline`, `Polygon`),
  `Navigation*` for trip/session data (`NavigationState`, `NavigationListener`,
  `NavigationPhase`). Do not mix (`MapNavigationController` is both).

---

## 3. Public `MapView` surface

The intent: one object a developer holds per screen; engines hold the SDK root; the
wasm harness gets the same names.

### (a) Create + lifecycle

```kotlin
// Kotlin (Android)
val maps = MapsSdk()
// The whole-set call registers every capability the named provider serves from
// the one credential it issued. To choose one capability independently, call
// `configureProvider(contract, providerKey, credential)` instead.
maps.configureProviderSet("mapquest", credential)  // throws SdkException verbatim
val mapView = MapView(context, maps)                  // FrameLayout
mapView.onStart(); mapView.onStop(); mapView.onDestroy()
mapView.setOnReadyListener(object : MapView.OnReadyListener {
    override fun onMapReady(mapView: MapView) { mapView.setCamera(initialCamera) }
})
```
```swift
// Swift (iOS) — no lifecycle forwarding; readiness is the only gate
let maps = MapsSdk()
try maps.configureProviderSet("mapquest", credential)
let mapView = MapView(frame: .zero, maps: maps)
mapView.onReadyListener = { mapView in mapView.setCamera(initialCamera) }
```
```ts
// React Native (generated from the UDL, camelCase)
const maps = new MapsSdk();
maps.configureProviderSet("mapquest", credential);
const mapView = new MapView(maps);   // native host view binds to this object
mapView.onReadyListener = { onMapReady: (view) => view.setCamera(initialCamera) };
```
```ts
// wasm mirror (hand-written; needs js_name on every export)
await init();
const maps = await MapsSdk.create();                 // was today's `start_sdk()`
const mapView = new MapView(canvas, maps);           // async init inside; was `GpuMapCanvas`
await mapView.ready;
mapView.setCamera(initialCamera);
```

The wasm entry keeps the product name with an async factory (`MapsSdk.create()`),
while the contract interface is the direct constructor `MapSdk()` that Kotlin and
Swift bind. The shapes differ on purpose — a browser page awaits its module and
then a factory; native hosts construct the interface — and `contract_sync.mjs`
records that one-name tolerance rather than mirroring an async factory the UDL
does not carry.

#### Choosing a provider

`configureProvider(contract, providerKey, credential)` is provider-neutral by
design: the contract name selects the capability slot, the key names a provider
the registry knows, and the credential is whatever that provider issued.
`configureProviderSet(providerKey, credential)` fills every capability the
named provider serves from a single credential. What ships today:

| `providerKey` | Serves | Credential |
|---|---|---|
| `mapquest` | geocoding, routing, distance matrix, place search, traffic, raster tiles | a MapQuest API key |
| `here` | geocoding, routing, traffic | a HERE API key |
| `google` | geocoding, routing | a Google Maps API key |
| `tomtom` | geocoding, routing, traffic, raster tiles | a TomTom API key |
| `mapbox` | geocoding, routing | a Mapbox access token |
| `radar` | geocoding, routing | a Radar API key |
| `osrm` | routing | the deployment's base URL (empty for a local server) |
| `valhalla` | routing | the deployment's base URL (empty for a local server) |
| `photon` | geocoding, place search, type-ahead | the deployment's base URL (empty for the public instance) |
| `openstreetmap` | raster tiles | none (some sources want a User-Agent) |
| `osmus`, `openfreemap` | a styled vector base with labels | none |

A keyless base map draws, and the keyless providers answer their contracts
without a credential. Adding a provider is a provider crate, a registry
descriptor and a dispatch row; the calls above do not change. `providerRegistrations()` reports, in a stable order, which key
serves each capability. The settings surface of every reference app renders its
rows from the same registry, so a new provider appears there without a UI
change.

**Landed (2026-10-30).** The wasm seam now serves the construction half of
this shape: `await MapsSdk.create()` resolves to the SDK root
(`maps.compiledVersion`), `new MapView(canvas, maps)` binds the canvas to
that root (`mapView.sdkCompiledVersion`), and the optional third argument
(`new MapView(canvas, maps, TileResolution.Retina)`) pins the tile
resolution for capture harnesses — omitted, it follows the same device
pixel-ratio rule as `devicePixelRatioThreshold()`. `initializeSdk()` stays
as the readiness alias for existing harness code. The camera verbs and
`ready` promise on this class remain open.

### (b) Camera control / animation / fit-bounds

```kotlin
val camera: MapCameraPose = mapView.camera
mapView.setCamera(MapCameraPose(38.58, -121.49, zoomLevel = 14.0, bearingDegrees = 0.0, pitchDegrees = 0.0))
mapView.animateCamera(target, durationMillis = 500)                 // a gesture cancel stops it
mapView.fitBounds(bounds, edgeInsets = EdgeInsets(top = 64.0, left = 16.0, bottom = 300.0, right = 16.0), maximumZoomLevel = 17.0)
mapView.listener = object : MapViewListener {
    override fun onCameraChanged(camera: MapCameraPose, isGestureDriven: Boolean) {}
}
```
```swift
let camera: MapCameraPose = mapView.camera
mapView.setCamera(MapCameraPose(latitudeDegrees: 38.58, longitudeDegrees: -121.49, zoomLevel: 14, bearingDegrees: 0, pitchDegrees: 0))
mapView.animateCamera(target, durationMilliseconds: 500)
mapView.fitBounds(boundingBox: bounds, edgeInsets: EdgeInsets(top: 64, left: 16, bottom: 300, right: 16), maximumZoomLevel: 17)
// A MapViewDelegate receives mapView(_:didChangeCamera:isGestureDriven:).
// TS: mapView.setCamera(camera); mapView.animateCamera(camera, 500);
//     mapView.fitBounds(bounds, { left: 16, top: 64, right: 16, bottom: 300 }, 17);
```

**Camera contract status: readback, set, fit and interpolation (landed 2026-11-17, addition-only).**
The UDL carries four `MapView` camera members and one `MapCamera`
constructor. They are additive: no existing camera, gesture or draw member
changes, and the generated Kotlin, Swift and React Native bindings carry them.
The Android, iOS and React Native views hold the contract `MapCamera` as their
only camera state: a set, a gesture, a fit and every animation step hand the
contract value to the native view and report its readback, and an animation
frame is one `MapCamera.interpolate` call. The wrapper's camera value type
(`MapCameraPose` on Kotlin and Swift, the `MapCamera` record on TypeScript) is
a projection of that readback, not a second state store. The hand-maintained
WebAssembly TypeScript façade carries the same members; its browser shape is
described below. The camera-change notification remains a callback
(`onCameraChanged` on Android and React Native,
`mapView(_:didChangeCamera:isGestureDriven:)` on iOS) rather than the
subscription sketched above.

| Member | Returns | Behavior and failure modes |
| --- | --- | --- |
| `MapView.camera()` | `MapCamera` | The camera pose the view holds. It starts at latitude 0, longitude 0, zoom 2, north-up and top-down, and every `set_camera`, `fit_bounds` or accepted gesture transform updates it. The view stores its own pose so a wrapper reports the same value it hands to `draw`; drawing still receives the pose as an argument. Cannot fail. |
| `MapView.set_camera(camera)` | nothing | Adopts a validated pose and cannot fail. The camera constructor clamps zoom and pitch and wraps the bearing, so adopting never changes an overlay. |
| `MapView.fit_bounds(bounding_box, edge_insets, maximum_zoom_level)` | `MapCamera?` | Frames the box inside the attached viewport's inset region, top-down and north-up by construction, at the largest zoom that keeps the whole box inside the insets and clamped to the maximum. Anchors the center on the inset region's center. Answers the adopted camera, or null when no surface viewport is attached; nothing changes in that case. Throws `InvalidCoordinateInput` when the box is not canonical, the insets are negative or leave no inset area, the maximum zoom is not finite, or the box spans no positive world area. A refused fit keeps the previous camera. Unlike the sketch above, this member uses the attached viewport instead of taking one. |
| `MapView.camera_geographic_bounds()` | `BoundingBox?` | Unprojects the four viewport corners through the current camera. Answers null when no surface viewport is attached or the projection cannot answer. At a steep pitch, when fewer than three corners reach the ground, the camera's own ground scale bounds a box around its center instead. |
| `MapCamera.interpolate(start_camera, target_camera, fraction)` | `MapCamera` | The SDK's one camera-interpolation curve. Clamps `fraction` into `[0, 1]` and eases with smoothstep, moves the center along the shortest longitude path in Mercator space, interpolates zoom and pitch linearly and takes the shortest bearing arc. Fractions at or below zero answer `start_camera` verbatim; fractions at or above one answer `target_camera` verbatim, so an animation's endpoints are exact. Throws `InvalidCoordinateInput` when `fraction` is not finite. |

The UDL spells these in snake_case; UniFFI generates `camera`, `setCamera`,
`fitBounds` and `cameraGeographicBounds` on the native surfaces, and
`MapCamera.interpolate` keeps its name.

#### WebAssembly camera members

The browser façade maps the same members onto the canvas view. The browser has
no attached platform surface, so the viewport and the vertical field of view
are explicit arguments, exactly as in `MapCamera.groundMetersPerScreenPixel`
and the projection methods.

| Member | Signature | Behavior |
|---|---|---|
| `mapView.camera()` | `(): MapCamera` | The one pose the view holds. It starts at latitude 0, longitude 0, zoom 2, north-up and top-down; `setCamera`, `fitBounds` and every rendered frame update it. |
| `mapView.setCamera(camera)` | `(camera: MapCamera): void` | Adopts a validated pose and cannot fail. The binding borrows the camera, so the caller keeps using its instance. |
| `mapView.fitBounds(...)` | `(southWestLatitude, southWestLongitude, northEastLatitude, northEastLongitude, widthPixels, heightPixels, insetTopPixels, insetRightPixels, insetBottomPixels, insetLeftPixels, maximumZoomLevel): MapCamera \| null` | Frames the box in the given viewport, adopts the fitted camera and answers it. Answers null before the view initializes; a refused fit keeps the previous camera and throws `InvalidCoordinateInput`. |
| `mapView.cameraGeographicBounds(widthPixels, heightPixels, fieldOfViewDegrees)` | `: { southWestLatitude, southWestLongitude, northEastLatitude, northEastLongitude } \| null` | Reads the stored pose's geographic box, with the ground-scale fallback at a steep pitch. Answers null when the projection cannot answer. |
| `MapCamera.interpolate(startCamera, targetCamera, fraction)` | static `(startCamera: MapCamera, targetCamera: MapCamera, fraction: number): MapCamera` | The contract's interpolation curve, with exact endpoints and the shortest bearing arc. Throws `InvalidCoordinateInput` when `fraction` is not finite. |

`renderFrame(camera, fieldOfViewDegrees)` keeps its per-frame camera argument.
A rendered frame is authoritative: it updates the stored pose `camera()`
reports.

### (c) Overlays

```kotlin
val marker = mapView.addMarker(Marker(coordinate, anchor = MarkerAnchor.BottomCenter))
val polyline = mapView.addPolyline(Polyline(coordinates, widthPixels = 6.0, color = RouteColor))
val polygon = mapView.addPolygon(Polygon(ringCoordinates, fillColor = ParkColor))
mapView.removeOverlay(marker); mapView.clearOverlays()
```
`Marker` / `Polyline` / `Polygon` are binding-layer types converting to the existing
`MapOverlayDescriptor` list in one `setOverlays` call — the single-descriptor-with-
nullable-payload record stays inside the FFI, never in a developer's hands.

**Contract status (M17 contract slice, 2026-10-30):** the id-addressed members are
on the UDL contract as `MapView.add_overlay` / `update_overlay` / `remove_overlay`
(Kotlin `addOverlay` / `updateOverlay` / `removeOverlay`). `add_overlay(overlay)`
answers a generated opaque identifier (`overlay-1`, a monotonic per-view sequence
that is never reused, even across `clear_overlays`/`set_overlays`); validation is
`set_overlays`' all-or-nothing validation for the single descriptor, so a refused
descriptor throws and adds nothing. `update_overlay(id, overlay)` replaces the
addressed overlay in its draw position; an invalid descriptor throws and keeps the
previous overlay in force, and an unknown id throws a typed `SdkError` with a
precise detail. `remove_overlay(id)` answers whether the id was present (an
unknown id is not an error), and `clear_overlays()` clears both the bulk list and
every addressed overlay. `set_overlays` stays the documented bulk call: it
replaces the whole drawn list — including addressed overlays — and drops the
addressed bookkeeping, so an id returned before a `set_overlays` call no longer
addresses anything. Overlay state, the addressed set and the identifier sequence
all outlive attach/detach.

### (d) Gestures

```kotlin
mapView.gestures.isPanEnabled = true
mapView.gestures.isZoomEnabled = true
mapView.gestures.isRotateEnabled = true
mapView.gestures.isTiltEnabled = false
mapView.addInteractionListener(object : MapView.InteractionListener {
    override fun onPanBegin() {}; override fun onPan() {}; override fun onPanEnd() {}
})
```
The three `MapCamera` gesture members (`pan_by_screen_pixels`,
`zoom_by_levels_around_screen_point`, `rotate_by_degrees_around_screen_point`) carry
this algebra in the contract, so all four shells compute identical results; the
reference apps route their pan/pinch/rotate math through them.

### (e) Search

```kotlin
val search = SearchEngine(maps)
suspend fun places(q: String) = search.search(q, boundary = null)      // was searchPlaces
suspend fun suggestions(q: String) = search.suggest(q, boundary = null) // was searchAutocomplete
suspend fun at(c: GeographicCoordinate) = search.reverseGeocode(c)
```

### (f) Route

```kotlin
val routing = RoutingEngine(maps)
val route: Route = routing.calculateRoute(waypoints, RouteOptions(...))
val matrix = routing.calculateDistanceMatrix(origins, destinations, TransportationMode.DRIVING)
```

### (g) Navigation with listeners

```kotlin
val navigator = Navigator(maps)
val subscription = navigator.addNavigationListener(object : NavigationListener {
    override fun onInstruction(instruction: NavigationInstruction) {}
    override fun onSpeechInstruction(speech: SpeechInstruction) {}
    override fun onDeviationChanged(change: DeviationStatusChange) {}
    override fun onPhaseChanged(change: NavigationPhaseChange) {}
})
navigator.startNavigation(route, NavigationOptions(unitSystem = UnitSystem.METRIC))
navigator.updateLocation(locationUpdate)
navigator.applyReroute(replacementRoute)
navigator.stopNavigation()
subscription.cancel()
```
One listener with four methods replaces the four `subscribe_to_*` calls (and matches
HERE's `NavigatorListener` / Mapbox's `NavigationListener`). The four topics remain
inside the implementation for honesty — a `NavigationListener` that implements only
`onInstruction` still must not receive speech on that method.

**Contract status (M17 contract slice, 2026-10-30):** `Navigator.add_navigation_listener`
and `Navigator.stop_navigation` are on the UDL contract. The listener is subscribed
to all four existing topics in one call, and the returned composite `Subscription`'s
`cancel()` removes all four registrations (idempotent). `stop_navigation()` ends the
active trip and clears trip state — `current_trip_state`/`current_instructions` answer
null, and later fixes, reroutes and reroute-request signals are dropped until
`start_navigation` begins a new trip. It resets the speech-trigger band memory and
the reroute-recalculation gate with the trip (utterance ids stay monotonic; the
configured reroute policy persists) and leaves listener subscriptions untouched.
No stop event is dispatched: the native reducer and the frozen phase channel have no
stop transition, so inventing one would be a semantics the reducer does not own.

### (h) Offline downloads

The caching-terms members are on the contract today and stay unchanged:

```kotlin
val offline = OfflineManager(maps)
offline.setTileTimeToLive(timeToLiveSeconds = 2_592_000) // ceiling still clamps; readback is the truth
val terms: TileCacheTerms = offline.tileCacheTerms()
if (terms.effectiveTimeToLiveSeconds != requestedSeconds) { /* show clamped value */ }
```

**Planned: region downloads.** The offline-region members are specified here
and are not on the published UDL contract yet. The contract names are
snake_case; the Kotlin, Swift and React Native bindings derive their spelling
from them. The addition-only wave is:

| Member | Shape |
|---|---|
| `OfflineManager.region_tile_count(region)` | synchronous count of the tiles a definition covers, so a settings surface can warn before a large download |
| `OfflineManager.download_region(region, progress_listener)` | async; returns the final `RegionRecord` |
| `OfflineManager.cancel_region_download(region_id)` | signals an in-flight download; idempotent and a no-op for an unknown or finished region |
| `OfflineManager.region_records()` | async; every region stored on disk |
| `OfflineManager.region_record(region_id)` | async; one region, or `null` when the id is unknown |
| `OfflineManager.delete_region(region_id)` | async; removes the region and returns a `RegionRemovalReport` |

The records are `RegionDefinition` (region id, display name, bounding box,
minimum and maximum zoom level, image format, tile resolution, optional
requested retention), `RegionRecord` (the same shape plus the effective
retention, member tile count, present tile count and present payload bytes),
`RegionDownloadProgress` (region id, total and completed tile counts, the
network-fetched and disk-served split, and a `RegionDownloadPhase`),
`RegionDownloadPhase` (`Downloading`, `Completed`, `Cancelled`, `Failed`) and
`RegionRemovalReport` (removed entry count and removed payload bytes).
`RegionDownloadProgressListener` follows the existing [`Trait, WithForeign]`
pattern; its `on_region_download_progress` runs on an SDK worker thread and
must marshal to the platform UI thread. A cancelled download is a successful
partial result: the terminal phase is `Cancelled` and the returned record's
present count is below its member count, so a later download resumes from
the tiles already on disk. A region whose maximum zoom level is deeper than
the installed provider's declared raster ceiling is refused with
`InvalidCoordinateInput`; the contract adds no cancellation token and no new
`SdkError` variant.

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
val tileCount = offline.regionTileCount(region)
val listener = object : RegionDownloadProgressListener {
    override fun onRegionDownloadProgress(progress: RegionDownloadProgress) {
        // progress.completedTileCount / progress.totalTileCount
    }
}
val record: RegionRecord = offline.downloadRegion(region, listener)
```

The WebAssembly surface has no disk pack store: the browser binds no
`TileCacheInstallation`, so the hand-maintained TypeScript façade neither
declares nor serves these members. Its offline surface stays the routing
archive below.

Routing from a compiled archive is a contract function on every native surface today —
archive bytes in, a `Route` out — sharing one `rustic-maps-core` implementation with the wasm
`offlineRouteFromArchiveBytes` export:

```kotlin
val plannedRoute: Route = offlineRouteFromArchiveBytes(
    archiveBytes = compiledArchiveBytes,          // ByteArray from the RMOG file
    start = GeographicCoordinate(latitude = 0.001, longitude = 0.0),
    end = GeographicCoordinate(latitude = 0.001, longitude = 0.002),
)
```
The request uses the archive profile's default options (automobile, fastest, metric);
a corrupt archive throws `OfflineRoutingFailure`. The engine-shaped façade
(`OfflineRoutingEngine.fromArchiveBytes`) remains the v2 target, not a requirement for
the capability.

### (i) Tile / base-map provider registration

```kotlin
maps.configureProvider(ProviderContract.RasterTiles, "openstreetmap", "")  // one capability
maps.registerUrlTemplateRasterTileSource(rasterSource)   // or declare the source here
mapView.setVectorTileSource(vectorSource)                // styled vector base with labels
mapView.reloadTileSource()                               // re-resolve the raster provider, drop pooled tiles
mapView.setStyle(MapStyle.VECTOR_LIGHT)                  // base-map switch
```
`configureProviderSet(providerKey, credential)` registers a provider's whole
set in one call; `configureProvider(contract, providerKey, credential)`
replaces exactly one slot, and `providerRegistrations()` reads back what serves
each one. A raster
provider or credential change reaches the map through `reloadTileSource()`,
which re-resolves the provider and drops the scheduler's pooled tiles without
an attach/detach cycle; the camera, overlays and location layer keep their
state. The browser target serves the same per-capability registry through
`configureProvider`, `providerRegistrations`, `configureProviderSet(providerKey, credential)`
and `clearProviderRegistrations`; the older `registerProvider` / `clearProvider`
names are gone.

A vector base can also be served by application code. Implement
`VectorTileProvider` — `sourceDescriptor()` describes the tileset and
`fetchVectorTile(tileX, tileY, zoomLevel)` returns its bytes — and register it
with `registerVectorTileProviderFromForeign(provider)`. The SDK selects that
provider under the application key `application`, so a provider-backed base
names it in `ProviderVectorTileSource(providerKey = "application", styleId = …)`.
`vectorTileProviderIsRegistered()` reports whether the slot is filled. The
URL-template source record carries the tileset's terms text, `{s}` subdomains,
per-request headers and style hints beside the template; null or empty fields
declare none.

`vectorTileSourceStatus()` reports what the base is drawing with and what its
last resolved frame achieved. The same record crosses every target; the
WebAssembly surface spells each field in lower camel case.

| Record field | WebAssembly field | Meaning |
|---|---|---|
| `is_configured` | `isConfigured` | Whether a vector base is installed. |
| `style_id` | `styleId` | The installed style id. |
| `source_name` | `sourceName` | The served tileset's name. |
| `attribution_text` | `attributionText` | The attribution line the terms require. |
| `maximum_zoom_level` | `maximumZoomLevel` | The highest zoom the tileset serves. |
| `drawn_tile_count` | `drawnTileCount` | Tile addresses the last frame drew. |
| `failed_tile_count` | `failedTileCount` | Tile addresses whose fetch or decode failed. |
| `last_failure_detail` | `lastFailureDetail` | The last tile failure's detail; null when none. |
| `unsupported_property_count` | `unsupportedPropertyCount` | Every style property or layer kind the renderer could not apply. |
| `unsupported_property_names` | `unsupportedPropertyNames` | The distinct unsupported properties, scope-qualified and sorted; at most 32 names. |
| `degraded_asset_names` | `degradedAssetNames` | Assets that fell back (a declared sprite, glyph endpoint or undrawable source), sorted; at most 16 names. |
| `source_layer_counts` | `sourceLayerCounts` | Per-source `matched_layer_count` and `drawn_layer_count` for the last frame, sorted by source id. |

The two name lists are bounded so a large vendor document cannot grow the
status without bound; the count still reports every diagnostic. A document
whose declared vector source names a tileset the provider does not serve is
refused at install, naming the declared source and the served tileset, rather
than painting fragments.

### (j) Readiness and errors

```kotlin
mapView.setOnReadyListener(...)                                    // map can now draw & project
maps.diagnostics.addListener(listener, SdkLogSeverity.WARNING)     // replaces subscribe_to_diagnostic_events
maps.diagnostics.flushRecent(listener, SdkLogSeverity.WARNING, maximumCount = 100)
try { search.search(q) } catch (error: SdkException) { /* typed, verbatim detail */ }
```
Keep the typed-exception model (Mapbox does the same); do not invent string-based error
callbacks (Google's model). Readiness is the only callback.

---

## 4. Naming verdict

The v2 names below are the shipped surface on every binding. The WebAssembly
exports use the same camelCase names as Kotlin, Swift and React Native, and the
implementation adjectives and snake_case exports the first draft carried were
retired (the current surface is in the UDL and the generated bindings).

**Never rename:** `GeographicCoordinate`, `BoundingBox`, `RouteStep`, `RouteLeg`,
`RouteOptions`, `AvoidOptions`, `NavigationPhase`, `SdkError`, `MapCamera`.

---

## 5. WebAssembly: one contract, four surfaces

The browser cannot get its view class from the UDL: a DOM `HTMLCanvasElement`
and a `GPUDevice` are not expressible in UniFFI types. A thin **wasm-bindgen**
canvas adapter therefore stays hand-written, and it is the only hand-maintained
piece of the wasm surface. Everything else is generated from the UDL: record
shapes, enum spellings, method and class names and error variants come from the
one contract, so the browser writes the same camelCase names as Kotlin, Swift
and React Native. The page uses one wasm module for both rendering and the
contract surface, with one initialization and one provider registry
(`PRD.md` §6 items 27 and 30).

The frame pump stays outside the contract: there is no SDK-internal render loop.
The platform wrapper owns the pump and the canvas adapter reports its own device
pixel ratio.

---

## 6. `MapView` ownership and the frame pump

### Ownership split (extends `docs/ReferenceAppUX.md` §5.1, which currently stops at "platform owns gestures")

SDK owns: surface acquisition & lifetime, the frame pump, resize, camera value store + animation,
gesture→camera deltas, overlay bookkeeping by id, readiness/error derivation, pointer→coordinate for
taps. Platform app owns: widgets, sheets, navigation UI, TTS, provider credentials UI.

### Frame pump ownership

**Lives in the platform wrapper (Kotlin/Swift), never in Rust.** The contract is explicit that there is
no SDK render loop (`rustic_maps.udl:1257-1268`, "never an SDK-internal loop"); moving it into Rust
would require a platform callback into FFI, i.e. a new foreign-callback interface and a threading story
for UIKit/Choreographer — the opposite of a simplification. The wrapper is also the only place that can
observe window attachment and pause.

**Android** — `Choreographer.postFrameCallback` + a `Channel<Long>(CONFLATED)` + one consumer coroutine;
copy the proven RN shape (`ReferenceMapController.kt:744-778`) rather than the Compose
`withFrameNanos` one, so the same code serves XML and Compose hosts. Post from
`onAttachedToWindow`, stop on `onDetachedFromWindow` / `onWindowVisibilityChanged != VISIBLE` /
`setRenderingPaused(true)`.

**iOS** — `CADisplayLink` on `.main`/`.common`, started in `didMoveToWindow` when `window != nil`,
invalidated when `nil`, plus `setRenderingPaused`.

**Per tick, in order** (all already validated in-tree):
1. extent check → `resize` when the *capped* pixel extent changed (never every frame);
2. `frame_settled(camera)` — pure query; throws only when detached (`udl:1468-1477`);
3. if settled → **pause the pump** (see below); else `draw` with at most one in flight
   (`SdkException.RenderingSurfaceUnavailable` → retry next tick);
4. report statistics / ready / error.

**Not burning a core when idle.** Because `draw` performs its fetch phase inside the call (the await
spans network), a `frame_settled == true` answer means nothing is pending — so the pump may stop
entirely rather than keep ticking. On settle: `Choreographer.removeFrameCallback` /
`displayLink.isPaused = true`. Resume on: camera set/animate, gesture delta, overlay mutation,
location-indicator change, surface created/changed, window attach / visibility / `setRenderingPaused(false)`,
`retryAttach`, and a ~1 s watchdog tick (cheap insurance for anything not observable). Keep ticking
while `frame_settled == false`; if statistics report `deferred == 0` and `unavailable > 0` with an
unchanged pose for N consecutive frames, drop to a 1 s retry cadence — otherwise a permanently failing
provider turns into a 60 fps spin against the network (the scheduler retries `Unavailable` tiles every
frame: `rustic-maps-rendering/src/tile_scheduler.rs:11-12, :34-35`).

**What the app must still do: nothing.** Optional only: `setRenderingPaused(true)` in
`onStop`/`scenePhase == .background` if it wants to release the GPU surface early; `retryAttach()` after
an error surfaced by `onError`.

### Device pixel ratio

The committed styles declare point/CSS sizes while the surface draws device pixels. The wrapper reports
the drawing buffer's device pixels per point with the contract's `set_device_pixel_ratio`
(`MapView.set_device_pixel_ratio`, M30) whenever it applies the drawable scale — at attach, after a
resize and on a rebuilt native view. The SDK multiplies the placed glyph sizes and the overlay line
widths and marker/pin radii by that ratio exactly once, so labels and overlays keep their declared point
size on a 1×, 2× or 3× surface. Android reports `resources.displayMetrics.density` (falling back to 1.0
until the display resolves); iOS reports its capped drawable scale (`drawable pixels / point`). The wasm
canvas derives its own ratio from `canvas.width / clientWidth` and deliberately carries no setter, so the
member stays native-only.

---

## 7. Native view construction — implementation notes

These are the measured facts behind the M17 native slices; they keep the
work from re-discovering the same constraints.

### Android: the surface pointer shim ships inside the AAR

Android exposes no public Kotlin API for a `Surface`'s `ANativeWindow*`, so
today every integrator hand-writes a three-line C shim and a JNI declaration.
The SDK can ship it: the crate already builds the Android `cdylib`
(`librustic_maps.so`), so a `cfg(target_os = "android")` module with
`jni` and `ndk-sys` dependencies exports

```rust
#[no_mangle]
pub extern "system" fn Java_com_angoratek_rusticmaps_map_NativeWindowShim_acquireNativeWindow(
    environment: JNIEnv,
    _shim_class: JClass,
    surface: JObject,
) -> jlong
```

and the AAR's Kotlin object loads it with `System.loadLibrary("rustic_maps")`
(the same library the UniFFI JNA bindings load). A fixed class/package name
makes the mangled symbol a compile-time constant; `RegisterNatives`/
`JNI_OnLoad` buys nothing here, and a consumer ProGuard keep rule covers
obfuscation. Releasing the window on every failure path matters: both current
app-side hosts leak one `ANativeWindow*` reference per failed attach.

### iOS: the view owns its own layer pointer

`MapView: UIView` with `layerClass == CAMetalLayer` hands Rust
`Unmanaged.passUnretained(self).toOpaque()` from inside the view, so user code
never does pointer math. It must also:
- implement `init?(coder:)` (the current app-side view `fatalError`s, so it
  cannot be used from a Storyboard or XIB);
- cap the drawable at the platform's texture limit (`max_texture_dimension_2d`
  is 2048 on downlevel adapters — an unguarded full-screen buffer on a tall
  phone is rejected);
- detach in `dismantleUIView` / `didMoveToWindow(nil)`, never in `deinit`
  (the Rust side must release its surface while the layer still exists).

### React Native: one SDK object, owned by the package

The generated JSI bindings construct their own `MapSdk`, and there is no way
to import a foreign handle into them. A native `<MapView>` therefore has to
use the SDK object the package owns and proxy the map calls through its
module — otherwise two registries (and two SQLite tile caches) exist in one
process. The app shell already works this way; the M17 slice moves that
pattern into the shipped package.

Navigation does not need that package-owned handle: the generated `Navigator`
creates its own native navigation object and exposes the contract's
`NavigationListener` callback surface, so JavaScript can drive a trip and
receive the instruction/phase/deviation/speech topics directly:

```ts
import { Navigator } from '@angoratek/rustic-maps-sdk';

const navigator = new Navigator();
navigator.addNavigationListener({
  onNavigationInstruction: instruction => renderBanner(instruction),
  onNavigationPhaseChange: change => renderPhase(change),
});
navigator.startNavigation(route, 'metric');
navigator.updateLocation(fix);
```

The curated `MapsSdk` proxy deliberately does not duplicate this surface; its
provider-scoped members (distance matrix, traffic, credentials) stay on the
proxy because they need the shared provider registry. The owner recorded the
split in `PRD.md` §6 item 22.

## 8. Device positioning

The contract carries one positioning controller and its records (added in
0.1.0; see `docs/history/PositioningResearch.md` for the platform research):

```text
dictionary PositionFix {
    GeographicCoordinate coordinate;
    double? horizontal_accuracy_meters;
    double? course_heading_degrees;
    double? timestamp_seconds;
};

enum PositionSourceError { "PermissionDenied", "ServiceDisabled", "Unavailable", "Timeout" };

interface PositionController {
    constructor();
    [Throws=SdkError] void feed_fix(PositionFix fix);
    void source_failed(PositionSourceError error);
    PositionFix? latest_fix();
    double? smoothed_course_degrees();
    boolean is_stale(double now_seconds);
    PositionSourceError? source_status();
};
```

The host owns the operating system's location API and its permission prompt,
and pushes fixes and failures into the controller. The controller validates
each fix, ignores stationary repeats when averaging course, smooths the course
around the circle, tracks staleness and records source failures, so the
renderer's location indicator, a follow camera and the `Navigator` read one
consistent stream. A controller built before any fix answers `latest_fix` as
null; an invalid fix throws `SdkError::InvalidCoordinateInput` with the
validator's detail and leaves the previous state in force.

The wasm seam exposes the same class (`feedFix`, `sourceFailed`, `latestFix`,
`smoothedCourseDegrees`, `isStale`, `sourceStatus`); the browser demo's
Location settings use it with `navigator.geolocation`.
